//! `017` T014 — `classify_redis` per profile and the closed not-supported walk.

use spacestorage_compat::{
    classify, classify_redis_verb, redis_capability, redis_is_not_supported, redis_option_supported,
    ClassifyOutcome, DialectProfile, VerbClass, REDIS_CAPABILITY_TABLE, REDIS_NOT_SUPPORTED,
};
use std::collections::BTreeSet;

#[test]
fn every_capability_command_is_must_on_complete_product() {
    for cmd in REDIS_CAPABILITY_TABLE {
        assert_eq!(
            classify_redis_verb(DialectProfile::CompleteProduct, cmd.name),
            ClassifyOutcome::Must,
            "capability command {} should be Must on CompleteProduct",
            cmd.name
        );
    }
}

const FB_STRING_LIST: &[&str] = &[
    "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "PTTL", "EXPIRE",
];

#[test]
fn every_new_capability_command_is_must_not_before_complete() {
    for cmd in REDIS_CAPABILITY_TABLE {
        if FB_STRING_LIST.contains(&cmd.name) {
            continue; // shared `016` string list stays Must on every profile
        }
        for profile in [DialectProfile::FirstBinary, DialectProfile::HandlersComplete] {
            let outcome = classify_redis_verb(profile, cmd.name);
            assert_eq!(
                outcome.class(),
                VerbClass::MustNot,
                "new capability command {} should be MustNot on {profile:?}",
                cmd.name
            );
            assert!(matches!(outcome, ClassifyOutcome::MustNot(_)));
        }
    }
}

#[test]
fn every_not_supported_command_is_must_not_on_every_profile() {
    for verb in REDIS_NOT_SUPPORTED {
        for profile in [
            DialectProfile::FirstBinary,
            DialectProfile::HandlersComplete,
            DialectProfile::CompleteProduct,
        ] {
            assert!(
                matches!(
                    classify(profile, spacestorage_compat::ProtocolId::Redis, verb),
                    VerbClass::MustNot
                ),
                "not-supported {verb} must stay MustNot on {profile:?}"
            );
        }
    }
}

#[test]
fn not_supported_matches_canonical_families() {
    // Sample of each family named in contracts/not-supported.md.
    for verb in [
        "GEOADD",
        "GEOSEARCHSTORE",
        "XAUTOCLAIM",
        "EVAL",
        "FUNCTION",
        "FCALL",
        "MODULE",
        "CLUSTER",
        "SENTINEL",
        "ACL",
        "DEBUG",
        "SHUTDOWN",
        "MIGRATE",
        "REPLICAOF",
        "RESTORE",
        "DUMP",
        "RENAME",
        "HELLO",
        "SWAPDB",
    ] {
        assert!(redis_is_not_supported(verb), "{verb} should be not-supported");
    }
}

#[test]
fn capability_table_has_no_duplicates() {
    let mut seen = BTreeSet::new();
    for cmd in REDIS_CAPABILITY_TABLE {
        assert!(seen.insert(cmd.name), "duplicate capability row {}", cmd.name);
    }
}

#[test]
fn first_binary_string_list_unchanged() {
    for v in [
        "AUTH", "PING", "GET", "SET", "DEL", "EXISTS", "SCAN", "SELECT", "TTL", "PTTL", "EXPIRE",
    ] {
        assert_eq!(
            classify(DialectProfile::FirstBinary, spacestorage_compat::ProtocolId::Redis, v),
            VerbClass::Must,
            "{v}"
        );
    }
    // `017` T067: the ten complete-product verbs named by the task, plus a few
    // more later capability rows, must stay MustNot on both pre-complete
    // profiles — `016` gains no new MUST verb (SC-006 / FR-005, research R13).
    for v in [
        "HGET", "XADD", "MULTI", "SUBSCRIBE", "PFADD", "SETBIT", "INFO", "COMMAND", "TYPE",
        "DBSIZE", "ECHO", "QUIT", "TIME", "CLIENT", "OBJECT", "CONFIG",
    ] {
        assert_eq!(
            classify(DialectProfile::FirstBinary, spacestorage_compat::ProtocolId::Redis, v),
            VerbClass::MustNot,
            "{v} must not grow the first binary"
        );
        assert_eq!(
            classify(
                DialectProfile::HandlersComplete,
                spacestorage_compat::ProtocolId::Redis,
                v
            ),
            VerbClass::MustNot,
            "{v} must not grow handlers-complete"
        );
    }
}

#[test]
fn unknown_verb_is_must_not() {
    assert_eq!(
        classify(
            DialectProfile::CompleteProduct,
            spacestorage_compat::ProtocolId::Redis,
            "NO_SUCH_VERB"
        ),
        VerbClass::MustNot
    );
}

#[test]
fn capability_lookup_and_options() {
    assert!(redis_capability("HSET").is_some());
    assert!(redis_capability("hset").is_some());
    assert!(redis_capability("COMMAND").is_some());
    assert!(redis_capability("COMMAND INFO").is_some());
    assert!(redis_capability("NOPE").is_none());
    // Unknown option of a listed command → not supported.
    assert!(redis_option_supported("SET", "NX"));
    assert!(!redis_option_supported("SET", "BOGUS"));
}
