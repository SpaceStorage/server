use subtle::ConstantTimeEq;

pub fn check_bearer(provided: &str, expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let a = provided.as_bytes();
    let b = expected.as_bytes();
    if a.len() != b.len() {
        // still compare to avoid trivial timing oracle on length alone for short tokens
        let _ = a.ct_eq(b);
        return false;
    }
    bool::from(a.ct_eq(b))
}
