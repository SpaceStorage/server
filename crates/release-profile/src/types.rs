//! Required creatable L3 types per release profile.

use crate::profile::ReleaseProfile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRequirement {
    pub required_creatable_l3: &'static [&'static str],
}

impl TypeRequirement {
    pub fn for_profile(profile: ReleaseProfile) -> Self {
        match profile {
            ReleaseProfile::FirstBinary | ReleaseProfile::CompleteProduct => Self {
                required_creatable_l3: &["K/V Store", "Relational Table", "Document Store"],
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_binary_three_l3() {
        let t = TypeRequirement::for_profile(ReleaseProfile::FirstBinary);
        assert_eq!(t.required_creatable_l3.len(), 3);
        assert!(t.required_creatable_l3.contains(&"Document Store"));
    }
}
