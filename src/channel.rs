use std::collections::VecDeque;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};
use crossbeam::utils::CachePadded;
use futures::task::AtomicWaker;
use spin::mutex::SpinMutex;
use spin::RwLock;
use crate::{Either, SendResult};
use pin_project::pin_project;

// operation should in mutex context
pub struct Handle<T>{
    pub item:Option<T>,
    pub waker: Waker
}

pub struct Inner<T>{
    pub cap: Option<AtomicUsize>,
    pub sender_cnt: CachePadded<AtomicUsize>,
    pub receiver_cnt: CachePadded<AtomicUsize>,
    pub shutdown: AtomicBool,
    pub phan:PhantomData<T>,
    pub queue: RwLock<VecDeque<T>>,
    pub blocking_senders: SpinMutex<VecDeque<Arc<Handle<T>>>>,
    pub blocking_receiver: SpinMutex<VecDeque<Arc<Waker>>>
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
            blocking_senders: SpinMutex::new(VecDeque::new()),
            blocking_receiver:SpinMutex::new(VecDeque::new()),
        }
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
    pub item: Either<T,Arc<Handle<T>>>
}

impl<T> Future for SendFuture<T>{
    type Output = SendResult<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // let this = self.project().unpinned;
        match self.item{
            Either::Left(item)=>{
                let queue = self.inner.queue.upgradeable_read();
                if self.inner.cap.map(|x|x.load(Ordering::Acquire)< queue.len()).unwrap_or(true){
                    let mut upgrade = queue.upgrade();
                    upgrade.push_back(item);
                    Poll::Ready(Ok(()))
                }else{

                }
            }
            Either::Right(handle)=>{
                Poll::Ready(Ok(())
            }
        }
    }
}
