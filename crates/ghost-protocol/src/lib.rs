//! Pure wire-format codecs for W3GS, GPS and BNCS. No I/O, no async.
#![forbid(unsafe_code)]

pub mod bncs;
pub mod bytes_ext;
pub mod error;
pub mod frame;
pub mod gps;
pub mod w3gs;

pub use bytes_ext::{BufExt, decode_statstring, encode_statstring, put_cstring};
pub use bncs::outgoing::GameVisibility;
pub use error::ProtoError;
pub use frame::{Frame, HeaderCodec};
