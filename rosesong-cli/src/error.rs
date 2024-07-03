use flexi_logger::FlexiLoggerError;
use reqwest::header::InvalidHeaderValue;
use std::io;
use thiserror::Error;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::AcquireError;
use tokio::task::JoinError;

#[derive(Error, Debug, Clone)]
pub enum ApplicationError {
    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("Data parsing error: {0}")]
    DataParsingError(String),

    #[error("Header value error: {0}")]
    HeaderValueError(String),

    #[error("Semaphore acquire error: {0}")]
    SemaphoreAcquireError(String),

    #[error("Join task error: {0}")]
    JoinTaskError(String),

    #[error("GStreamer initialization error: {0}")]
    InitError(String),

    #[error("TOML parsing error: {0}")]
    TomlParsingError(String),

    #[error("Fetch error: {0}")]
    FetchError(String),

    #[error("Logger initialization error: {0}")]
    LoggerError(String),

    #[error("Channel send error: {0}")]
    SendError(String),

    #[error("Mutex lock error: {0}")]
    MutexLockError(String),
}

impl From<reqwest::Error> for ApplicationError {
    fn from(error: reqwest::Error) -> Self {
        ApplicationError::NetworkError(error.to_string())
    }
}

impl From<io::Error> for ApplicationError {
    fn from(error: io::Error) -> Self {
        ApplicationError::IoError(error.to_string())
    }
}

impl From<InvalidHeaderValue> for ApplicationError {
    fn from(error: InvalidHeaderValue) -> Self {
        ApplicationError::HeaderValueError(error.to_string())
    }
}

impl From<AcquireError> for ApplicationError {
    fn from(error: AcquireError) -> Self {
        ApplicationError::SemaphoreAcquireError(error.to_string())
    }
}

impl From<JoinError> for ApplicationError {
    fn from(error: JoinError) -> Self {
        ApplicationError::JoinTaskError(error.to_string())
    }
}

impl From<toml::de::Error> for ApplicationError {
    fn from(error: toml::de::Error) -> Self {
        ApplicationError::TomlParsingError(error.to_string())
    }
}

impl From<FlexiLoggerError> for ApplicationError {
    fn from(error: FlexiLoggerError) -> Self {
        ApplicationError::LoggerError(error.to_string())
    }
}

impl<T> From<SendError<T>> for ApplicationError {
    fn from(error: SendError<T>) -> Self {
        ApplicationError::SendError(error.to_string())
    }
}
