use thiserror::Error;

#[derive(Debug, Error)]
pub enum SendError {
    #[error("UnknownError")]
    UnknownError,
    #[error("DisConnected")]
    DisConnected,
}

#[derive(Debug, Error)]
pub enum ReceiveError {
    #[error("DisConnected")]
    DisConnected,
}
