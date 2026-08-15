use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProtoError {
    #[error("truncated: need {need} bytes, have {have}")]
    Truncated { need: usize, have: usize },
    #[error("string is not NUL-terminated")]
    UnterminatedString,
    #[error("bad value: {0}")]
    BadValue(&'static str),
    #[error("payload too large for u16 length field: {0} bytes")]
    TooLarge(usize),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

impl Clone for ProtoError {
    fn clone(&self) -> Self {
        match self {
            Self::Truncated { need, have } => Self::Truncated {
                need: *need,
                have: *have,
            },
            Self::UnterminatedString => Self::UnterminatedString,
            Self::BadValue(s) => Self::BadValue(s),
            Self::TooLarge(l) => Self::TooLarge(*l),
            Self::Io(e) => Self::Io(std::io::Error::new(e.kind(), e.to_string())),
        }
    }
}

impl PartialEq for ProtoError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Truncated { need: n1, have: h1 }, Self::Truncated { need: n2, have: h2 }) => {
                n1 == n2 && h1 == h2
            }
            (Self::UnterminatedString, Self::UnterminatedString) => true,
            (Self::BadValue(s1), Self::BadValue(s2)) => s1 == s2,
            (Self::TooLarge(l1), Self::TooLarge(l2)) => l1 == l2,
            (Self::Io(e1), Self::Io(e2)) => e1.kind() == e2.kind(),
            _ => false,
        }
    }
}

impl Eq for ProtoError {}
