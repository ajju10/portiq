use hyper::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Route not found")]
    NotFound,
    #[error("No upstream available")]
    NoUpstream,
}

impl RouterError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            RouterError::NotFound => StatusCode::NOT_FOUND,
            RouterError::NoUpstream => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
