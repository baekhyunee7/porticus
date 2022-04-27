use crate::channel::{Inner, Receiver, Sender};
use crate::error::{ReceiveError, SendError};
use std::sync::Arc;

mod channel;
mod error;
mod lock;

pub fn bounded<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Inner::new(Some(cap)));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver {
            inner: inner.clone(),
        },
    )
}

pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Arc::new(Inner::new(None));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver {
            inner: inner.clone(),
        },
    )
}

pub type SendResult<T> = std::result::Result<T, SendError>;
pub type ReceiveResult<T> = std::result::Result<T, ReceiveError>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
