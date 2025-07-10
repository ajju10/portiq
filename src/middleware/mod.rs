use hyper::header::HeaderName;

const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

mod access_logger;
mod request_id;

pub use access_logger::AccessLogger;
pub use request_id::RequestID;
