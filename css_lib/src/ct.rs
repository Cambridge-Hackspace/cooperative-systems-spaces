//! Constant-time comparison for secret-bearing equality checks.
//!
//! #120 (#14 / L1): API-key and card-digest checks compared secrets with `==`,
//! which returns on the first differing byte. That leaks, through timing, how
//! many leading bytes of a guess were correct -- enough, in principle, to
//! recover a secret one byte at a time. The oracle is impractical to build in a
//! test, so this is a fix on principle; the code is what carries the guarantee.

/// Constant-time byte-slice equality.
///
/// Unlike `==`, comparison time does not depend on *where* two equal-length
/// inputs first differ. Length is allowed to leak (a key's length is not the
/// secret): unequal lengths return `false` immediately. The final read is put
/// through [`std::hint::black_box`] so the optimizer cannot reintroduce an early
/// exit.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    std::hint::black_box(diff) == 0
}

/// Constant-time equality for two strings, over their UTF-8 bytes.
pub fn constant_time_str_eq(a: &str, b: &str) -> bool {
    constant_time_eq(a.as_bytes(), b.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_slices_match() {
        assert!(constant_time_eq(b"a-secret-key", b"a-secret-key"));
        assert!(constant_time_str_eq("token123", "token123"));
    }

    #[test]
    fn any_difference_fails() {
        // Differ at the end, at the start, and in the middle -- all rejected.
        assert!(!constant_time_eq(b"a-secret-key", b"a-secret-keY"));
        assert!(!constant_time_eq(b"a-secret-key", b"A-secret-key"));
        assert!(!constant_time_eq(b"a-secret-key", b"a-secXet-key"));
    }

    #[test]
    fn different_lengths_fail() {
        assert!(!constant_time_eq(b"short", b"shorter"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
