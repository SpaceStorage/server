//! Namespace name rules: `^[A-Za-z][A-Za-z0-9_]*$`, length 1–63.

use crate::error::{Result, TenancyError};

pub fn validate_namespace_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 63 {
        return Err(TenancyError::NamespaceNameInvalid {
            name: name.into(),
        });
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(TenancyError::NamespaceNameInvalid {
            name: name.into(),
        });
    };
    if !first.is_ascii_alphabetic() {
        return Err(TenancyError::NamespaceNameInvalid {
            name: name.into(),
        });
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(TenancyError::NamespaceNameInvalid {
            name: name.into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_acme() {
        validate_namespace_name("acme").unwrap();
        validate_namespace_name("A_1").unwrap();
    }

    #[test]
    fn rejects_bad() {
        assert!(validate_namespace_name("").is_err());
        assert!(validate_namespace_name("1acme").is_err());
        assert!(validate_namespace_name("acme-x").is_err());
        assert!(validate_namespace_name(&"a".repeat(64)).is_err());
    }
}
