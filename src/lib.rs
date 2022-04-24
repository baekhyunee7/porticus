use std::sync::Arc;
use crate::channel::{Inner, Receiver, Sender};
use crate::error::SendError;

mod channel;
mod error;

pub fn bound<T>(cap:usize)-> (Sender<T>,Receiver<T>){
    let inner = Arc::new(Inner::new(Some(cap)));
    (Sender{inner:inner.clone()},Receiver{inner:inner.clone()})
}

pub type SendResult<T> = std::result::Result<T,SendError>;

pub enum Either<L,R>{
    Left(L),
    Right(R)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}