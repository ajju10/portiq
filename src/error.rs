use hyper::StatusCode;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RouterError {
    #[error("Route not found")]
    NotFound,
    #[error("Method not allowed")]
    MethodNotAllowed,
    #[error("No upstream available")]
    NoUpstream,
}

impl RouterError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            RouterError::NotFound => StatusCode::NOT_FOUND,
            RouterError::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            RouterError::NoUpstream => StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}
