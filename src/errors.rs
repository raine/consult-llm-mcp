use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Invalid request parameters: {0}")]
    InvalidParams(String),
}
