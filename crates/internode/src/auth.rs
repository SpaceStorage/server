//! Join-secret constant-time verify before cluster protocol.

use subtle::ConstantTimeEq;

pub fn verify_join_secret(expected: &[u8], presented: &[u8]) -> bool {
    // Empty expected (pre-inject) must not authenticate empty↔empty peers (FR-003 / T073).
    if expected.is_empty() {
        let _ = presented.ct_eq(presented);
        return false;
    }
    if expected.len() != presented.len() {
        // Touch both sides to reduce timing oracle; still refuse.
        let _ = expected.ct_eq(expected);
        let _ = presented.ct_eq(presented);
        return false;
    }
    bool::from(expected.ct_eq(presented))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_refuses() {
        assert!(verify_join_secret(b"abcdef0123456789abcdef0123456789", b"abcdef0123456789abcdef0123456789"));
        assert!(!verify_join_secret(b"abcdef0123456789abcdef0123456789", b"ffffffffffffffffffffffffffffffff"));
    }
}
