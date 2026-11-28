//! US1 (`017` T015–T019): complete-product collection + expiry conformance
//! against an unmodified stock Redis client. Test names carry `collections` so
//! `cargo test -p spacestorage-conformance --features redis-dialect -- collections`
//! selects this suite.
#![cfg(feature = "redis-dialect")]

mod support;

use std::time::Duration;

use support::redis_live::boot_redis_complete_product;

type Conn = redis::aio::MultiplexedConnection;

/// Run `parts` (command + string args) on a stock client.
async fn q<T: redis::FromRedisValue>(c: &mut Conn, parts: &[&str]) -> redis::RedisResult<T> {
    let mut cmd = redis::cmd(parts[0]);
    for p in &parts[1..] {
        cmd.arg(*p);
    }
    cmd.query_async(c).await
}

#[tokio::test]
async fn collections_smoke_ping_set_get() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;
    let pong: String = q(&mut c, &["PING"]).await.unwrap();
    assert_eq!(pong, "PONG");
    let _: String = q(&mut c, &["SET", "k", "v"]).await.unwrap();
    let v: Option<String> = q(&mut c, &["GET", "k"]).await.unwrap();
    assert_eq!(v.as_deref(), Some("v"));
    lab.stop();
}

#[tokio::test]
async fn collections_hash_family() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let n: i64 = q(&mut c, &["HSET", "h", "f1", "v1", "f2", "v2"]).await.unwrap();
    assert_eq!(n, 2);
    assert_eq!(q::<Option<String>>(&mut c, &["HGET", "h", "f1"]).await.unwrap().as_deref(), Some("v1"));
    assert_eq!(q::<i64>(&mut c, &["HLEN", "h"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["HEXISTS", "h", "f2"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["HEXISTS", "h", "nope"]).await.unwrap(), 0);
    let all: Vec<String> = q(&mut c, &["HGETALL", "h"]).await.unwrap();
    assert_eq!(all, vec!["f1", "v1", "f2", "v2"]);

    let ok: String = q(&mut c, &["HMSET", "h", "f3", "v3"]).await.unwrap();
    assert_eq!(ok, "OK");
    let m: Vec<Option<String>> = q(&mut c, &["HMGET", "h", "f1", "f3", "nope"]).await.unwrap();
    assert_eq!(m, vec![Some("v1".into()), Some("v3".into()), None]);

    assert_eq!(q::<i64>(&mut c, &["HSETNX", "h", "f1", "x"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["HSETNX", "h", "f4", "v4"]).await.unwrap(), 1);

    assert_eq!(q::<i64>(&mut c, &["HINCRBY", "h", "cnt", "5"]).await.unwrap(), 5);
    assert_eq!(q::<i64>(&mut c, &["HINCRBY", "h", "cnt", "3"]).await.unwrap(), 8);
    let cf: String = q(&mut c, &["HINCRBYFLOAT", "h", "cf", "1.5"]).await.unwrap();
    assert_eq!(cf, "1.5");
    assert_eq!(q::<i64>(&mut c, &["HSTRLEN", "h", "f1"]).await.unwrap(), 2);

    let keys: Vec<String> = q(&mut c, &["HKEYS", "h"]).await.unwrap();
    assert!(keys.contains(&"f1".to_string()));
    let vals: Vec<String> = q(&mut c, &["HVALS", "h"]).await.unwrap();
    assert!(vals.contains(&"v1".to_string()));
    assert_eq!(q::<i64>(&mut c, &["HDEL", "h", "f1"]).await.unwrap(), 1);

    // `HSCAN` returns [cursor, items].
    let scan: Vec<redis::Value> = q(&mut c, &["HSCAN", "h", "0"]).await.unwrap();
    assert_eq!(scan.len(), 2);
    let rnd: Option<String> = q(&mut c, &["HRANDFIELD", "h"]).await.unwrap();
    assert!(rnd.is_some());

    // Hash-field expiry (7.4): set, inspect, read+delete.
    assert_eq!(q::<i64>(&mut c, &["HSET", "e", "a", "1"]).await.unwrap(), 1);
    let ex: Vec<i64> = q(&mut c, &["HEXPIRE", "e", "100", "FIELDS", "1", "a"]).await.unwrap();
    assert_eq!(ex, vec![1]);
    let ttl: Vec<i64> = q(&mut c, &["HTTL", "e", "FIELDS", "1", "a"]).await.unwrap();
    assert!(ttl[0] > 0 && ttl[0] <= 100, "{ttl:?}");
    let et: Vec<i64> = q(&mut c, &["HEXPIRETIME", "e", "FIELDS", "1", "a"]).await.unwrap();
    assert!(et[0] > 0, "{et:?}");
    let got: Vec<Option<String>> = q(&mut c, &["HGETDEL", "e", "FIELDS", "1", "a"]).await.unwrap();
    assert_eq!(got, vec![Some("1".into())]);
    assert_eq!(q::<Option<String>>(&mut c, &["HGET", "e", "a"]).await.unwrap(), None);

    lab.stop();
}

#[tokio::test]
async fn collections_list_family() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["RPUSH", "l", "a", "b", "c"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["LPUSH", "l", "z"]).await.unwrap(), 4);
    assert_eq!(
        q::<Vec<String>>(&mut c, &["LRANGE", "l", "0", "-1"]).await.unwrap(),
        vec!["z", "a", "b", "c"]
    );
    assert_eq!(q::<i64>(&mut c, &["LLEN", "l"]).await.unwrap(), 4);
    assert_eq!(q::<Option<String>>(&mut c, &["LINDEX", "l", "0"]).await.unwrap().as_deref(), Some("z"));
    assert_eq!(q::<Option<String>>(&mut c, &["LINDEX", "l", "-1"]).await.unwrap().as_deref(), Some("c"));

    let ok: String = q(&mut c, &["LSET", "l", "0", "Z"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert_eq!(q::<i64>(&mut c, &["LINSERT", "l", "BEFORE", "a", "X"]).await.unwrap(), 5);
    assert_eq!(
        q::<Vec<String>>(&mut c, &["LRANGE", "l", "0", "-1"]).await.unwrap(),
        vec!["Z", "X", "a", "b", "c"]
    );

    assert_eq!(q::<Option<String>>(&mut c, &["LPOP", "l"]).await.unwrap().as_deref(), Some("Z"));
    assert_eq!(q::<Option<String>>(&mut c, &["RPOP", "l"]).await.unwrap().as_deref(), Some("c"));
    assert_eq!(
        q::<Vec<String>>(&mut c, &["LRANGE", "l", "0", "-1"]).await.unwrap(),
        vec!["X", "a", "b"]
    );
    assert_eq!(q::<i64>(&mut c, &["LPUSHX", "missing", "x"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["RPUSHX", "missing", "x"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["LREM", "l", "0", "a"]).await.unwrap(), 1);
    let ok: String = q(&mut c, &["LTRIM", "l", "0", "0"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert_eq!(q::<Vec<String>>(&mut c, &["LRANGE", "l", "0", "-1"]).await.unwrap(), vec!["X"]);

    // RPOPLPUSH + LMOVE across two keys.
    assert_eq!(q::<i64>(&mut c, &["RPUSH", "s1", "a", "b", "c"]).await.unwrap(), 3);
    assert_eq!(q::<Option<String>>(&mut c, &["RPOPLPUSH", "s1", "d1"]).await.unwrap().as_deref(), Some("c"));
    assert_eq!(q::<Vec<String>>(&mut c, &["LRANGE", "s1", "0", "-1"]).await.unwrap(), vec!["a", "b"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["LRANGE", "d1", "0", "-1"]).await.unwrap(), vec!["c"]);

    assert_eq!(q::<i64>(&mut c, &["RPUSH", "m1", "a", "b"]).await.unwrap(), 2);
    assert_eq!(
        q::<Option<String>>(&mut c, &["LMOVE", "m1", "m2", "LEFT", "RIGHT"]).await.unwrap().as_deref(),
        Some("a")
    );
    assert_eq!(q::<Vec<String>>(&mut c, &["LRANGE", "m1", "0", "-1"]).await.unwrap(), vec!["b"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["LRANGE", "m2", "0", "-1"]).await.unwrap(), vec!["a"]);

    lab.stop();
}

#[tokio::test]
async fn collections_list_blocking() {
    let lab = boot_redis_complete_product().await;
    let mut c1 = lab.connect().await;

    // Timeout path: empty key, named 1s timeout → null.
    let r: Option<(String, String)> = q(&mut c1, &["BLPOP", "nope", "1"]).await.unwrap();
    assert!(r.is_none());

    // Release-on-arrival: a second connection pushes after a delay.
    let mut c2 = lab.connect().await;
    let pusher = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let n: i64 = q(&mut c2, &["RPUSH", "bq", "hello"]).await.unwrap();
        assert_eq!(n, 1);
    });
    let r: Option<(String, String)> = q(&mut c1, &["BLPOP", "bq", "3"]).await.unwrap();
    pusher.await.unwrap();
    assert_eq!(r, Some(("bq".to_string(), "hello".to_string())));

    // BRPOP timeout path.
    let r: Option<(String, String)> = q(&mut c1, &["BRPOP", "nope", "1"]).await.unwrap();
    assert!(r.is_none());

    // BRPOPLPUSH immediate: data already present.
    assert_eq!(q::<i64>(&mut c1, &["RPUSH", "src", "a"]).await.unwrap(), 1);
    assert_eq!(
        q::<Option<String>>(&mut c1, &["BRPOPLPUSH", "src", "dst", "1"]).await.unwrap().as_deref(),
        Some("a")
    );
    assert_eq!(q::<Vec<String>>(&mut c1, &["LRANGE", "dst", "0", "-1"]).await.unwrap(), vec!["a"]);

    // BLMOVE immediate: data already present.
    assert_eq!(q::<i64>(&mut c1, &["RPUSH", "m1", "x", "y"]).await.unwrap(), 2);
    assert_eq!(
        q::<Option<String>>(&mut c1, &["BLMOVE", "m1", "m2", "LEFT", "LEFT", "1"]).await.unwrap().as_deref(),
        Some("x")
    );
    assert_eq!(q::<Vec<String>>(&mut c1, &["LRANGE", "m2", "0", "-1"]).await.unwrap(), vec!["x"]);

    lab.stop();
}

#[tokio::test]
async fn collections_set_family() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["SADD", "s", "a", "b", "c"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["SCARD", "s"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["SISMEMBER", "s", "a"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["SISMEMBER", "s", "x"]).await.unwrap(), 0);
    assert_eq!(q::<Vec<String>>(&mut c, &["SMEMBERS", "s"]).await.unwrap(), vec!["a", "b", "c"]);
    assert_eq!(q::<i64>(&mut c, &["SREM", "s", "a"]).await.unwrap(), 1);
    assert_eq!(q::<Vec<String>>(&mut c, &["SMEMBERS", "s"]).await.unwrap(), vec!["b", "c"]);
    assert_eq!(q::<Option<String>>(&mut c, &["SPOP", "s"]).await.unwrap().as_deref(), Some("b"));
    assert_eq!(q::<Vec<String>>(&mut c, &["SMEMBERS", "s"]).await.unwrap(), vec!["c"]);
    assert_eq!(q::<Option<String>>(&mut c, &["SRANDMEMBER", "s"]).await.unwrap().as_deref(), Some("c"));

    assert_eq!(q::<i64>(&mut c, &["SADD", "s2", "c", "d", "e"]).await.unwrap(), 3);
    assert_eq!(q::<Vec<String>>(&mut c, &["SINTER", "s", "s2"]).await.unwrap(), vec!["c"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["SUNION", "s", "s2"]).await.unwrap(), vec!["c", "d", "e"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["SDIFF", "s2", "s"]).await.unwrap(), vec!["d", "e"]);
    assert_eq!(q::<i64>(&mut c, &["SINTERSTORE", "dest", "s", "s2"]).await.unwrap(), 1);
    assert_eq!(q::<Vec<String>>(&mut c, &["SMEMBERS", "dest"]).await.unwrap(), vec!["c"]);

    assert_eq!(q::<i64>(&mut c, &["SADD", "s3", "a", "b"]).await.unwrap(), 2);
    let scan: Vec<redis::Value> = q(&mut c, &["SSCAN", "s3", "0"]).await.unwrap();
    assert_eq!(scan.len(), 2);

    lab.stop();
}

#[tokio::test]
async fn collections_zset_family() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    assert_eq!(q::<i64>(&mut c, &["ZADD", "z", "1", "a", "2", "b", "3", "c"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["ZCARD", "z"]).await.unwrap(), 3);
    assert_eq!(q::<Option<String>>(&mut c, &["ZSCORE", "z", "b"]).await.unwrap().as_deref(), Some("2"));
    let ms: Vec<Option<String>> = q(&mut c, &["ZMSCORE", "z", "a", "c", "x"]).await.unwrap();
    assert_eq!(ms, vec![Some("1".into()), Some("3".into()), None]);

    assert_eq!(q::<Vec<String>>(&mut c, &["ZRANGE", "z", "0", "-1"]).await.unwrap(), vec!["a", "b", "c"]);
    assert_eq!(
        q::<Vec<String>>(&mut c, &["ZRANGE", "z", "0", "-1", "WITHSCORES"]).await.unwrap(),
        vec!["a", "1", "b", "2", "c", "3"]
    );
    assert_eq!(q::<Vec<String>>(&mut c, &["ZREVRANGE", "z", "0", "-1"]).await.unwrap(), vec!["c", "b", "a"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZRANGEBYSCORE", "z", "2", "3"]).await.unwrap(), vec!["b", "c"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZREVRANGEBYSCORE", "z", "3", "2"]).await.unwrap(), vec!["c", "b"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZRANGE", "z", "(1", "3", "BYSCORE"]).await.unwrap(), vec!["b", "c"]);
    assert_eq!(q::<i64>(&mut c, &["ZCOUNT", "z", "2", "3"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["ZCOUNT", "z", "(1", "+inf"]).await.unwrap(), 2);

    assert_eq!(q::<i64>(&mut c, &["ZRANK", "z", "a"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["ZREVRANK", "z", "a"]).await.unwrap(), 2);
    assert_eq!(q::<Option<i64>>(&mut c, &["ZRANK", "z", "x"]).await.unwrap(), None);
    assert_eq!(q::<String>(&mut c, &["ZINCRBY", "z", "5", "a"]).await.unwrap(), "6");

    // NX/XX/GT/CH + INCR.
    assert_eq!(q::<i64>(&mut c, &["ZADD", "z", "NX", "10", "d"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["ZADD", "z", "XX", "10", "e"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["ZADD", "z", "GT", "CH", "1", "a"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["ZCARD", "z"]).await.unwrap(), 4);

    // BYLEX on an equal-score set.
    assert_eq!(q::<i64>(&mut c, &["ZADD", "lex", "0", "a", "0", "b", "0", "c"]).await.unwrap(), 3);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZRANGE", "lex", "-", "+", "BYLEX"]).await.unwrap(), vec!["a", "b", "c"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZRANGE", "lex", "[a", "[b", "BYLEX"]).await.unwrap(), vec!["a", "b"]);

    // Pops: lowest then highest.
    assert_eq!(q::<Vec<String>>(&mut c, &["ZPOPMIN", "z"]).await.unwrap(), vec!["b", "2"]);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZPOPMAX", "z"]).await.unwrap(), vec!["d", "10"]);
    assert_eq!(q::<i64>(&mut c, &["ZREM", "z", "a"]).await.unwrap(), 1);

    // Store forms.
    assert_eq!(q::<i64>(&mut c, &["ZADD", "zs", "1", "a", "2", "b"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["ZADD", "zs2", "2", "b", "3", "c"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["ZUNIONSTORE", "zdst", "2", "zs", "zs2"]).await.unwrap(), 3);
    assert_eq!(
        q::<Vec<String>>(&mut c, &["ZRANGE", "zdst", "0", "-1", "WITHSCORES"]).await.unwrap(),
        vec!["a", "1", "c", "3", "b", "4"]
    );
    assert_eq!(q::<i64>(&mut c, &["ZINTERSTORE", "zint", "2", "zs", "zs2"]).await.unwrap(), 1);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZDIFF", "2", "zs", "zs2"]).await.unwrap(), vec!["a"]);
    assert_eq!(q::<i64>(&mut c, &["ZDIFFSTORE", "zdiff", "2", "zs", "zs2"]).await.unwrap(), 1);
    assert_eq!(q::<Vec<String>>(&mut c, &["ZUNION", "2", "zs", "zs2"]).await.unwrap(), vec!["a", "c", "b"]);
    assert_eq!(q::<i64>(&mut c, &["ZRANGESTORE", "zrs", "zs", "0", "-1"]).await.unwrap(), 2);
    assert_eq!(q::<i64>(&mut c, &["ZCARD", "zrs"]).await.unwrap(), 2);

    // Remove by score / rank.
    assert_eq!(q::<i64>(&mut c, &["ZADD", "zr", "1", "a", "2", "b", "3", "c"]).await.unwrap(), 3);
    assert_eq!(q::<i64>(&mut c, &["ZREMRANGEBYSCORE", "zr", "2", "2"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["ZREMRANGEBYRANK", "zr", "0", "0"]).await.unwrap(), 1);

    let scan: Vec<redis::Value> = q(&mut c, &["ZSCAN", "zs", "0"]).await.unwrap();
    assert_eq!(scan.len(), 2);

    lab.stop();
}

#[tokio::test]
async fn collections_zset_blocking() {
    let lab = boot_redis_complete_product().await;
    let mut c1 = lab.connect().await;

    let r: Option<(String, String, String)> = q(&mut c1, &["BZPOPMIN", "nope", "1"]).await.unwrap();
    assert!(r.is_none());

    let mut c2 = lab.connect().await;
    let pusher = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let n: i64 = q(&mut c2, &["ZADD", "bz", "5", "m"]).await.unwrap();
        assert_eq!(n, 1);
    });
    let r: Option<(String, String, String)> = q(&mut c1, &["BZPOPMIN", "bz", "3"]).await.unwrap();
    pusher.await.unwrap();
    assert_eq!(r, Some(("bz".to_string(), "m".to_string(), "5".to_string())));
    assert_eq!(q::<i64>(&mut c1, &["ZCARD", "bz"]).await.unwrap(), 0);

    // BZPOPMAX timeout path and immediate highest pop.
    let r: Option<(String, String, String)> = q(&mut c1, &["BZPOPMAX", "nope", "1"]).await.unwrap();
    assert!(r.is_none());
    assert_eq!(q::<i64>(&mut c1, &["ZADD", "bz2", "1", "m1", "2", "m2"]).await.unwrap(), 2);
    let r: Option<(String, String, String)> = q(&mut c1, &["BZPOPMAX", "bz2", "1"]).await.unwrap();
    assert_eq!(r, Some(("bz2".to_string(), "m2".to_string(), "2".to_string())));

    lab.stop();
}

#[tokio::test]
async fn collections_key_expiry() {
    let lab = boot_redis_complete_product().await;
    let mut c = lab.connect().await;

    let ok: String = q(&mut c, &["SETEX", "k", "100", "v"]).await.unwrap();
    assert_eq!(ok, "OK");
    let ttl: i64 = q(&mut c, &["TTL", "k"]).await.unwrap();
    assert!(ttl > 0 && ttl <= 100, "{ttl}");
    let pttl: i64 = q(&mut c, &["PTTL", "k"]).await.unwrap();
    assert!(pttl > 0, "{pttl}");

    let ok: String = q(&mut c, &["PSETEX", "k2", "100000", "v"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert!(q::<i64>(&mut c, &["TTL", "k2"]).await.unwrap() > 0);

    let ok: String = q(&mut c, &["SET", "k3", "v", "EX", "100"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert!(q::<i64>(&mut c, &["TTL", "k3"]).await.unwrap() > 0);
    let ok: String = q(&mut c, &["SET", "k4", "v", "PX", "100000"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert!(q::<i64>(&mut c, &["PTTL", "k4"]).await.unwrap() > 0);

    let ok: String = q(&mut c, &["SET", "k5", "v"]).await.unwrap();
    assert_eq!(ok, "OK");
    assert_eq!(q::<i64>(&mut c, &["EXPIRE", "k5", "100"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["PEXPIRE", "k5", "100000"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["EXPIREAT", "k5", "4000000000"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["EXPIRETIME", "k5"]).await.unwrap(), 4_000_000_000);
    assert_eq!(q::<i64>(&mut c, &["PEXPIREAT", "k5", "4000000000000"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["PEXPIRETIME", "k5"]).await.unwrap(), 4_000_000_000_000);
    assert_eq!(q::<i64>(&mut c, &["PERSIST", "k5"]).await.unwrap(), 1);
    assert_eq!(q::<i64>(&mut c, &["TTL", "k5"]).await.unwrap(), -1);
    assert_eq!(q::<i64>(&mut c, &["PERSIST", "k5"]).await.unwrap(), 0);
    assert_eq!(q::<i64>(&mut c, &["TTL", "missing"]).await.unwrap(), -2);

    // Key absent after expiry; reads behave missing.
    let ok: String = q(&mut c, &["SET", "kx", "v", "PX", "50"]).await.unwrap();
    assert_eq!(ok, "OK");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(q::<Option<String>>(&mut c, &["GET", "kx"]).await.unwrap(), None);
    assert_eq!(q::<i64>(&mut c, &["TTL", "kx"]).await.unwrap(), -2);
    assert_eq!(q::<i64>(&mut c, &["EXISTS", "kx"]).await.unwrap(), 0);

    lab.stop();
}
