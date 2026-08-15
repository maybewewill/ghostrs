pub mod ids;
pub mod incoming;
pub mod outgoing;

use crate::frame::HeaderCodec;

pub const BNCS_HEADER: u8 = 0xFF;
pub type BncsCodec = HeaderCodec<BNCS_HEADER>;
