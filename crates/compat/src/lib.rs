//! Compatibility and limits — first-binary classify/admission/size rejects (015).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbClass {
    Must,
    MustNot,
    Unspecified,
}

pub fn classify_pg(verb: &str) -> VerbClass {
    let u = verb.to_ascii_uppercase();
    match u.as_str() {
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "CREATE" | "DROP" => VerbClass::Must,
        "COPY" | "BEGIN" | "COMMIT" | "ROLLBACK" => VerbClass::MustNot,
        _ => VerbClass::Unspecified,
    }
}

pub fn reject_oversized(size: u64, limit: u64) -> bool {
    size > limit
}
