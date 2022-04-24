use std::borrow::Borrow;
use std::collections::VecDeque;
use std::future::Future;
use std::intrinsics::unreachable;
use std::marker::PhantomData;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use crossbeam::utils::CachePadded;
use futures::task::{ArcWake, AtomicWaker};
use spin::mutex::SpinMutex;
use spin::{RwLock, RwLockWriteGuard};
use crate::{Either, SendResult};
use pin_project::pin_project;
use crate::Either::Right;
use crate::lock::guard;

// operation should in mutex context
pub struct Handle<T>{
    pub item:Option<T>,
    pub waker: ArcWake
}

pub struct Inner<T>{
    pub cap: Option<AtomicUsize>,
    pub sender_cnt: CachePadded<AtomicUsize>,
    pub receiver_cnt: CachePadded<AtomicUsize>,
    pub shutdown: AtomicBool,
    pub phan:PhantomData<T>,
    pub queue: RwLock<VecDeque<T>>,
    pub blocking_senders: RwLock<VecDeque<Arc<Handle<T>>>>,
    pub blocking_receiver: RwLock<VecDeque<Arc<ArcWaker>>>
}

impl<T> Inner<T>{
    pub fn new(cap:Option<usize>)->Self{
        Self{
            cap:cap.map(AtomicUsize::new),
            sender_cnt:CachePadded::new(AtomicUsize::new(1)),
            receiver_cnt:CachePadded::new(AtomicUsize::new(1)),
            shutdown:AtomicBool::new(false),
            phan:PhantomData,
            queue: RwLock::new(VecDeque::new()),
            blocking_senders: RwLock::new(VecDeque::new()),
            blocking_receiver:RwLock::new(VecDeque::new()),
        }
    }

    pub fn grab_pending(&self){
        let mut queue_guard = guard(&self.queue,RwLock::try_upgradeable_read);
        let mut sending_guard = guard(&self.blocking_senders,RwLock::try_upgradeable_read);
        let cap = self.cap.map(|x|x.load(Ordering::Relaxed));
        if  cap > queue_guard.len() && sending_guard.len() > 0{
            let queue_guard =  queue_guard.upgrade();
            let sending_guard = sending_guard.upgrade();
            while cap > queue_guard.len() && sending_guard.len() > 0{
                let handle:Handle<T> = sending_guard.pop_front().unwrap();
                // todo: remove option?
                queue_guard.push_back(handle.item.unwrap());
                handle.waker.wake()
            }
        }
    }

    pub fn try_push(&self,item:&mut Option<T>)->bool{
        let mut queue_guard = guard(&self.queue,RwLock::try_upgradeable_read);
        let cap = self.cap.map(|x|x.load(Ordering::Relaxed));
        if cap > queue_guard.len(){
            let queue_guard = queue_guard.upgrade();
            queue_guard.push_back(item.take().unwrap());
            true
        }else{
            false
        }
    }

    pub fn new_sending(&self,item:&mut Option<T>,cx:&mut Context)->Arc<Handle<T>>{
        let mut guard = guard(&self.blocking_senders,RwLock::try_write);
        let handle = Arc::new(Handle{
            item:item.take().unwrap(),
            waker: {
                let waker = AtomicWaker::new();
                waker.register(cx.waker());
                waker
            }
        });
        guard.push_back(handle.clone());
        handle
    }
}

pub struct Sender<T>{
    pub inner:Arc<Inner<T>>
}

impl<T> Sender<T>{
    pub fn send_future(&self,item:T)->SendFuture<T>{
        SendFuture{
            inner:self.inner.clone(),
            item
        }
    }
}

pub struct Receiver<T>{
    pub inner: Arc<Inner<T>>
}


#[pin_project]
pub struct SendFuture<T>{
    pub inner:Arc<Inner<T>>,
    pub item: Either<Option<T>,Arc<Handle<T>>>
}

// 把sending转移
// 放到queue，不行就sending
// 唤醒receiver
impl<T> Future for SendFuture<T>{
    type Output = SendResult<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // let this = self.project().unpinned;
        match &mut self.item{
            Either::Left(item)if item.is_some()=>{
                self.inner.grab_pending();
                if !self.inner.try_push(item){
                    let handle = self.inner.new_sending(item,cx);
                    self.item = Right(handle);
                }
                // wake receiver
                Poll::Ready(Ok(()))
            }
            Either::Right(handle)=>{
                Poll::Ready(Ok(()))
            }
            _ => unreachable!()
        }
    }
}
