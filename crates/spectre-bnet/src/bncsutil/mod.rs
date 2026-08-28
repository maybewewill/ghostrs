pub mod cdkey;
pub mod check_revision;
pub mod exe_info;
pub mod libinfo;
pub mod mpq_num;
pub mod nls;
pub mod oldauth;
pub mod pe;
pub mod xsha1;

pub use cdkey::{
    CdKeyDecoder, CdKeyError, CdKeyInfo, KeyType, create_key_info, decode_cd_key, kd_quick,
};
pub use check_revision::{
    DEFAULT_MPQ_SEEDS, check_revision, check_revision_flat, get_mpq_seed, set_mpq_seed,
};
pub use exe_info::{
    BNCSUTIL_PLATFORM_MAC, BNCSUTIL_PLATFORM_OSX, BNCSUTIL_PLATFORM_PPC, BNCSUTIL_PLATFORM_WIN,
    BNCSUTIL_PLATFORM_WINDOWS, BNCSUTIL_PLATFORM_X86, ExeInfo, get_exe_info,
};
pub use libinfo::{
    BNCSUTIL_VERSION, BNCSUTIL_VERSION_STRING, bncsutil_get_version, bncsutil_get_version_string,
    get_version, get_version_string,
};
pub use mpq_num::extract_mpq_number;
pub use nls::{
    NLS_G, NLS_I, NLS_PRIME_BYTES, NLS_SIG_N, NLS_SIGNATURE_KEY, NlsAccountCreatePacket,
    NlsAccountLogonPacket, NlsChangeProofPacket, NlsError, NlsSession, check_signature,
};
pub use oldauth::{double_hash_password, hash_password as oldauth_hash_password};
pub use pe::{
    PeFixedFileInfo, extract_pe_fixed_file_info, extract_pe_version, extract_pe_version_from_file,
};
pub use xsha1::{calc_hash_buf, hash_password, xsha1};
