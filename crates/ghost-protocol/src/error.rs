use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    #[error("truncated: need {need} bytes, have {have}")]
    Truncated { need: usize, have: usize },
    #[error("string is not NUL-terminated")]
    UnterminatedString,
    #[error("bad value: {0}")]
    BadValue(&'static str),
    #[error("payload too large for u16 length field: {0} bytes")]
    TooLarge(usize),
}
