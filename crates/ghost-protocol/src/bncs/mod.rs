pub mod ids;

use crate::frame::HeaderCodec;

pub const BNCS_HEADER: u8 = 0xFF;
pub type BncsCodec = HeaderCodec<BNCS_HEADER>;
