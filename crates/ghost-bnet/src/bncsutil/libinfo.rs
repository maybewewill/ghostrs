//! BNCSutil Library Information
//!
//! Provides version numbers and version strings matching the original BNCSutil C API.

/// BNCSutil version constant (e.g. 10405 for version 1.4.5: (major * 10000) + (minor * 100) + rev).
pub const BNCSUTIL_VERSION: u32 = 10405;

/// BNCSutil version string literal.
pub const BNCSUTIL_VERSION_STRING: &str = "1.4.5";

/// Retrieves the integer version of the BNCSutil implementation.
#[inline]
pub fn get_version() -> u32 {
    BNCSUTIL_VERSION
}

/// Canonical C alias for `get_version` matching `bncsutil_getVersion`.
#[inline]
pub fn bncsutil_get_version() -> u32 {
    BNCSUTIL_VERSION
}

/// Retrieves the version string of the BNCSutil implementation.
#[inline]
pub fn get_version_string() -> &'static str {
    BNCSUTIL_VERSION_STRING
}

/// Canonical C alias for `get_version_string` matching `bncsutil_getVersionString`.
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
