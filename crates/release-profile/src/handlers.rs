//! Handler inventories per release profile.

use crate::profile::ReleaseProfile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandlerBuildSet {
    pub required: &'static [&'static str],
    pub forbidden: &'static [&'static str],
}

impl HandlerBuildSet {
    pub fn for_profile(profile: ReleaseProfile) -> Self {
        match profile {
            ReleaseProfile::FirstBinary => Self {
                required: &[
                    "admin",
                    "admin-http",
                    "internode",
                    "replication",
                    "postgresql",
                    "redis",
                ],
                forbidden: &[
                    "cassandra",
                    "elasticsearch",
                    "clickhouse",
                    "clickhouse-http",
                    "s3",
                    "webdav",
                ],
            },
            ReleaseProfile::CompleteProduct => Self {
                required: &[
                    "admin",
                    "admin-http",
                    "internode",
                    "replication",
                    "postgresql",
                    "redis",
                    "cassandra",
                    "elasticsearch",
                    "clickhouse",
                    "clickhouse-http",
                    "s3",
                    "webdav",
                ],
                // syslog is not required in either profile until slice 11
                forbidden: &[],
            },
        }
    }

    pub fn is_required(&self, name: &str) -> bool {
        self.required.contains(&name)
    }

    pub fn is_forbidden(&self, name: &str) -> bool {
        self.forbidden.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_binary_forbids_cassandra() {
        let set = HandlerBuildSet::for_profile(ReleaseProfile::FirstBinary);
        assert!(set.is_forbidden("cassandra"));
        assert!(set.is_required("postgresql"));
        assert!(set.is_required("redis"));
        assert!(set.is_required("internode"));
    }
}
