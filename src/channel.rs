use crate::lock::guard;
use crate::SendResult;
use atomic_option::AtomicOption;
use crossbeam::utils::CachePadded;
use futures::future::Either;
use futures::task::AtomicWaker;
use pin_project::pin_project;
use spin::mutex::{SpinMutex, SpinMutexGuard};
use std::collections::VecDeque;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

pub struct Handle<T> {
    pub item: AtomicOption<T>,
    pub waker: AtomicWaker,
}

pub struct Inner<T> {
    pub cap: Option<usize>,
    pub sender_cnt: CachePadded<AtomicUsize>,
    pub receiver_cnt: CachePadded<AtomicUsize>,
    pub shutdown: AtomicBool,
    pub phan: PhantomData<T>,
    pub data: SpinMutex<Data<T>>,
}

pub struct Data<T> {
    pub queue: VecDeque<T>,
    pub blocking_senders: VecDeque<Arc<Handle<T>>>,
    pub blocking_receiver: VecDeque<Arc<AtomicWaker>>,
}

impl<T> Default for Data<T> {
    fn default() -> Self {
        Self {
            queue: VecDeque::new(),
            blocking_receiver: VecDeque::new(),
            blocking_senders: VecDeque::new(),
        }
    }
}

impl<T> Inner<T> {
    pub fn new(cap: Option<usize>) -> Self {
        Self {
            cap,
            sender_cnt: CachePadded::new(AtomicUsize::new(1)),
            receiver_cnt: CachePadded::new(AtomicUsize::new(1)),
            shutdown: AtomicBool::new(false),
            phan: PhantomData,
            data: SpinMutex::new(Data::default()),
        }
    }

    pub fn send(&self, data: &mut Option<T>, cx: &mut Context) -> bool {
        let mut guard = guard(&self.data);
        let mut pending = false;
        self.grab_pending(&mut guard);
        if !guard.blocking_receiver.is_empty()
            || self.cap.map(|x| x <= guard.queue.len()).unwrap_or(false)
        {
            let handle = Handle {
                item: AtomicOption::new(Box::new(data.take().unwrap())),
                waker: {
                    let waker = AtomicWaker::new();
                    waker.register(cx.waker());
                    waker
                },
            };
            let handle = Arc::new(handle);
            guard.blocking_senders.push_back(handle);
            pending = true;
        } else {
            guard.queue.push_back(data.take().unwrap())
        }
        Self::wakeup(&mut guard);
        !pending
    }

    fn wakeup(data: &mut SpinMutexGuard<Data<T>>) {
        while let Some(waker) = data.blocking_receiver.pop_front() {
            if !data.queue.is_empty() {
                waker.wake();
            } else {
                break;
            }
        }
    }

    fn grab_pending(&self, data: &mut SpinMutexGuard<Data<T>>) {
        while let Some(handle) = data.blocking_senders.pop_front() {
            if let Some(item) = handle.item.take(Ordering::Release) {
                if self.cap.map(|x| x < data.queue.len()).unwrap_or(true) {
                    data.queue.push_back(*item);
                } else {
                    handle.item.replace(Some(item), Ordering::Release);
                    break;
                }
            }
            handle.waker.wake()
        }
    }
}

pub struct Sender<T> {
    pub inner: Arc<Inner<T>>,
}

impl<T> Sender<T> {
    pub fn send_future(&self, item: T) -> SendFuture<T> {
        SendFuture {
            inner: self.inner.clone(),
            item: Either::Left(Some(item)),
        }
    }
}

pub struct Receiver<T> {
    pub inner: Arc<Inner<T>>,
}

#[pin_project]
pub struct SendFuture<T> {
    pub inner: Arc<Inner<T>>,
    pub item: Either<Option<T>, Arc<AtomicOption<Handle<T>>>>,
}

// 把sending转移
// sending还有剩余放sending
// 放到sending，不行就sending
// 唤醒receiver
impl<T> Future for SendFuture<T> {
    type Output = SendResult<()>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        match &mut this.item {
            Either::Left(item) if item.is_some() => {
                if this.inner.send(item, cx) {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
            Either::Right(handle) => Poll::Ready(Ok(())),
            _ => unreachable!(),
        }
    }
}
