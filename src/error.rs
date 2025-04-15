use flexi_logger::FlexiLoggerError;
use glib::BoolError;
use reqwest::{header::InvalidHeaderValue, Error as ReqwestError};
use std::io::Error as IoError;
use thiserror::Error;
use tokio::{
    sync::{mpsc::error::SendError, AcquireError},
    task::JoinError,
};
use toml::de::Error as TomlError;
use zbus::Error as ZbusError;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Header value error: {0}")]
    HeaderValue(#[from] InvalidHeaderValue),
    #[error("Semaphore acquire error")]
    SemaphoreAcquire(#[from] AcquireError),
    #[error("Join task error")]
    JoinTask(#[from] JoinError),
    #[error("GStreamer initialization error: {0}")]
    Init(String),
    #[error("TOML parsing error")]
    TomlParsing(#[from] TomlError),
    #[error("Fetch error: {0}")]
    Fetch(String),
    #[error("Logger initialization error")]
    Logger(#[from] FlexiLoggerError),
    #[error("Channel send error: {0}")]
    Send(String),
    #[error("GStreamer element error: {0}")]
    Element(String),
    #[error("GStreamer pipeline error: {0}")]
    Pipeline(String),
    #[error("GStreamer link error: {0}")]
    Link(String),
    #[error("GStreamer state error: {0}")]
    State(String),
    #[error("HTTP request failed")]
    HttpRequest(#[from] ReqwestError),
    #[error("I/O operation failed")]
    Io(#[from] IoError),
    #[error("Data parsing error: {0}")]
    DataParsing(String),
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Environment variable error")]
    EnvVar(#[from] std::env::VarError),
    #[error("UTF-8 conversion error")]
    Utf8Conversion(#[from] std::string::FromUtf8Error),
    #[error("Oneshot channel receive error")]
    OneshotRecv(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("Zbus error")]
    Zbus(#[from] ZbusError),
}

impl From<BoolError> for AppError {
    fn from(_: BoolError) -> Self {
        AppError::Init("Failed to perform an operation on GStreamer pipeline".to_string())
    }
}

impl<T> From<SendError<T>> for AppError {
    fn from(error: SendError<T>) -> Self {
        AppError::Send(error.to_string())
    }
}
