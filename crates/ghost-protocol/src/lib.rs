//! Pure wire-format codecs for W3GS, GPS and BNCS. No I/O, no async.
#![forbid(unsafe_code)]

pub mod bytes_ext;
pub mod error;
pub mod w3gs;

pub use bytes_ext::{BufExt, decode_statstring, encode_statstring, put_cstring};
pub use error::ProtoError;
