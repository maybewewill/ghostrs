pub const BNCSUTIL_VERSION: u32 = 10405;

pub const BNCSUTIL_VERSION_STRING: &str = "1.4.5";

#[inline]
pub fn get_version() -> u32 {
    BNCSUTIL_VERSION
}

#[inline]
pub fn bncsutil_get_version() -> u32 {
    BNCSUTIL_VERSION
}

#[inline]
pub fn get_version_string() -> &'static str {
    BNCSUTIL_VERSION_STRING
}

#[inline]
pub fn bncsutil_get_version_string() -> &'static str {
    BNCSUTIL_VERSION_STRING
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_info_matches_expected_format() {
        assert_eq!(get_version(), 10405);
        assert_eq!(get_version_string(), "1.4.5");
    }
}
