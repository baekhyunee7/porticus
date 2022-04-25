use crate::channel::{Inner, Receiver, Sender};
use crate::error::SendError;
use std::sync::Arc;

mod channel;
mod error;
mod lock;

pub fn bound<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
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

pub type SendResult<T> = std::result::Result<T, SendError>;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
