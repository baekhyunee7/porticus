use crate::lock::guard;
use crate::{ReceiveError, ReceiveResult, SendError, SendResult};
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
    pub item: SpinMutex<Option<T>>,
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

    pub fn send(&self, data: &mut Option<T>, cx: &mut Context) -> Option<Arc<Handle<T>>> {
        let mut guard = guard(&self.data);
        let mut pending = None;
        self.grab_pending(&mut guard);
        if !guard.blocking_receiver.is_empty()
            || self.cap.map(|x| x <= guard.queue.len()).unwrap_or(false)
        {
            let handle = Handle {
                item: SpinMutex::new(Some(data.take().unwrap())),
                waker: {
                    let waker = AtomicWaker::new();
                    waker.register(cx.waker());
                    waker
                },
            };
            let handle = Arc::new(handle);
            guard.blocking_senders.push_back(handle.clone());
            pending = Some(handle);
        } else {
            guard.queue.push_back(data.take().unwrap())
        }
        Self::wakeup(&mut guard);
        pending
    }

    pub fn receive(
        &self,
        waker_opt: &mut Option<Arc<AtomicWaker>>,
        cx: &mut Context,
    ) -> std::result::Result<Option<T>, ()> {
        let mut guard = guard(&self.data);
        self.grab_pending(&mut guard);
        if !guard.blocking_receiver.is_empty() {
            // receivers still pending but disconnected
            if self.shutdown.load(Ordering::Acquire) {
                return Err(());
            }
            Self::pending_receiver(&mut guard, waker_opt, cx);
            Ok(None)
        } else {
            if let Some(item) = guard.queue.pop_front() {
                Ok(Some(item))
            } else {
                if self.shutdown.load(Ordering::Acquire) {
                    return Err(());
                }
                Self::pending_receiver(&mut guard, waker_opt, cx);
                Ok(None)
            }
        }
    }

    fn pending_receiver(
        guard: &mut SpinMutexGuard<Data<T>>,
        waker_opt: &mut Option<Arc<AtomicWaker>>,
        cx: &mut Context,
    ) {
        // still pending
        if let Some(waker) = waker_opt {
            waker.register(cx.waker());
            guard.blocking_receiver.push_back(waker.clone());
        } else {
            // new waker
            let waker = Arc::new(AtomicWaker::new());
            waker.register(cx.waker());
            guard.blocking_receiver.push_back(waker.clone());
            *waker_opt = Some(waker);
        }
    }

    fn wakeup(data: &mut SpinMutexGuard<Data<T>>) {
        for _ in 0..data.queue.len() {
            if let Some(waker) = data.blocking_receiver.pop_front() {
                waker.wake();
            } else {
                break;
            }
        }
    }

    fn grab_pending(&self, data: &mut SpinMutexGuard<Data<T>>) {
        while let Some(handle) = data.blocking_senders.pop_front() {
            let mut guard = guard(&handle.item);
            if let Some(item) = guard.take() {
                if self.cap.map(|x| x < data.queue.len()).unwrap_or(true) {
                    drop(guard);
                    data.queue.push_back(item);
                } else {
                    *guard = Some(item);
                    drop(guard);
                    break;
                }
            }
            handle.waker.wake()
        }
    }

    fn clear_all_pending(&self) {
        let mut guard = guard(&self.data);
        self.grab_pending(&mut guard);
        while let Some(waker) = guard.blocking_receiver.pop_front() {
            waker.wake()
        }
        while let Some(handle) = guard.blocking_senders.pop_front() {
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
    pub item: Either<Option<T>, Arc<Handle<T>>>,
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
                // if not in pending senders, no chance to send
                if this.inner.shutdown.load(Ordering::Acquire) {
                    return Poll::Ready(Err(SendError::DisConnected));
                }
                if let Some(handle) = this.inner.send(item, cx) {
                    *this.item = Either::Right(handle);
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            Either::Right(handle) => {
                // todo: RwLock?
                let guard = guard(&handle.item);
                // sender still pending
                if guard.is_some() {
                    // disconnected
                    if this.inner.shutdown.load(Ordering::Acquire) {
                        return Poll::Ready(Err(SendError::DisConnected));
                    }
                    drop(guard);
                    handle.waker.register(cx.waker());
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            _ => Poll::Ready(Err(SendError::UnknownError)),
        }
    }
}

impl<T> Receiver<T> {
    pub fn receive_future(&self) -> ReceiveFuture<T> {
        ReceiveFuture {
            inner: self.inner.clone(),
            waker: None,
        }
    }
}

#[pin_project]
pub struct ReceiveFuture<T> {
    pub inner: Arc<Inner<T>>,
    pub waker: Option<Arc<AtomicWaker>>,
}

impl<T> Future for ReceiveFuture<T> {
    type Output = ReceiveResult<T>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        if let Ok(item_opt) = this.inner.receive(&mut this.waker, cx) {
            if let Some(item) = item_opt {
                Poll::Ready(Ok(item))
            } else {
                Poll::Pending
            }
        } else {
            Poll::Ready(Err(ReceiveError::DisConnected))
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.inner.sender_cnt.fetch_add(1, Ordering::Release);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Clone for Receiver<T> {
    fn clone(&self) -> Self {
        self.inner.receiver_cnt.fetch_add(1, Ordering::Release);
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        if self.inner.sender_cnt.fetch_sub(1, Ordering::Release) <= 1 {
            self.inner.shutdown.store(true, Ordering::Release);
            self.inner.clear_all_pending();
        }
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        if self.inner.receiver_cnt.fetch_sub(1, Ordering::Release) <= 1 {
            self.inner.shutdown.store(true, Ordering::Release);
            self.inner.clear_all_pending()
        }
    }
}
