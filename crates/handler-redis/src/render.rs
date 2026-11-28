//! RESP2 reply shapes and error forms for the complete-product dialect
//! (`017` T007). Construction only — wire encoding stays in [`crate::resp`].

use crate::commands::RedisReply;

pub const WRONGTYPE: &str = "WRONGTYPE Operation against a key holding the wrong kind of value";

pub fn error(message: impl Into<String>) -> RedisReply {
    RedisReply::Error(message.into())
}

pub fn wrongtype() -> RedisReply {
    RedisReply::Error(WRONGTYPE.into())
}

pub fn arity(cmd: &str) -> RedisReply {
    RedisReply::Error(format!("ERR wrong number of arguments for '{}'", cmd.to_ascii_lowercase()))
}

pub fn unknown_command(cmd: &str) -> RedisReply {
    RedisReply::Error(format!("ERR unknown command '{cmd}'"))
}

pub fn unknown_subcommand() -> RedisReply {
    RedisReply::Error("ERR unknown subcommand or wrong number of arguments".into())
}

/// A `015` limit rejection, naming the limit.
pub fn limit(message: &str) -> RedisReply {
    RedisReply::Error(format!("ERR {message}"))
}

pub fn execabort() -> RedisReply {
    RedisReply::Error("EXECABORT Transaction discarded because of previous errors.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_forms() {
        assert_eq!(
            arity("SET"),
            RedisReply::Error("ERR wrong number of arguments for 'set'".into())
        );
        assert_eq!(
            unknown_command("GEOADD"),
            RedisReply::Error("ERR unknown command 'GEOADD'".into())
        );
        assert!(matches!(wrongtype(), RedisReply::Error(e) if e.starts_with("WRONGTYPE")));
    }
}
