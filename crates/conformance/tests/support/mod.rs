pub mod s3_live;

/// `017` T013: multi-connection in-process harness for the complete-product
/// Redis dialect. Serves the real `RedisHandler` on an ephemeral loopback port
/// over the shared `ContainerCatalog`, so an unmodified stock Redis client can
/// drive the documented command surface.
#[cfg(feature = "redis-dialect")]
pub mod redis_live {
    use std::sync::{Arc, RwLock};

    use spacestorage_compat::DialectProfile;
    use spacestorage_handler_redis::RedisHandler;
    use spacestorage_types::ContainerCatalog;
    use tokio::net::TcpListener;
    use tokio_util::sync::CancellationToken;

    /// A live complete-product Redis endpoint backed by one shared catalog.
    pub struct RedisLab {
        pub port: u16,
        pub cancel: CancellationToken,
        pub catalog: Arc<RwLock<ContainerCatalog>>,
    }

    impl RedisLab {
        /// URL with the demo credential embedded, so both explicit `AUTH` and
        /// the `PubSub` connection setup (which only AUTHs from the URL)
        /// authenticate successfully.
        pub fn url(&self) -> String {
            format!("redis://demo:demo@127.0.0.1:{}", self.port)
        }

        /// Connect a stock client and authenticate as the demo principal.
        pub async fn connect(&self) -> redis::aio::MultiplexedConnection {
            let client = redis::Client::open(self.url()).expect("redis url");
            let mut conn = client
                .get_multiplexed_async_connection()
                .await
                .expect("connect");
            let ok: String = redis::cmd("AUTH")
                .arg("demo")
                .arg("demo")
                .query_async(&mut conn)
                .await
                .expect("auth");
            assert_eq!(ok, "OK", "AUTH reply");
            conn
        }

        pub fn stop(&self) {
            self.cancel.cancel();
        }
    }

    /// Boot a complete-product Redis endpoint (fresh catalog, ephemeral port).
    pub async fn boot_redis_complete_product() -> RedisLab {
        boot_redis_with_profile(DialectProfile::CompleteProduct).await
    }

    pub async fn boot_redis_with_profile(profile: DialectProfile) -> RedisLab {
        boot_redis_named_profile("demo", profile).await
    }

    /// `017` US3: a complete-product endpoint bound to a specific namespace, so
    /// two labs can demonstrate cross-namespace pub/sub isolation.
    pub async fn boot_redis_named(namespace: &str) -> RedisLab {
        boot_redis_named_profile(namespace, DialectProfile::CompleteProduct).await
    }

    pub async fn boot_redis_named_profile(namespace: &str, profile: DialectProfile) -> RedisLab {
        let catalog = Arc::new(RwLock::new(ContainerCatalog::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        let cancel = CancellationToken::new();
        let mut handler = RedisHandler::with_demo(Arc::clone(&catalog)).with_profile(profile);
        handler.namespace = namespace.to_string();
        let handler = Arc::new(handler);
        let c = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = c.cancelled() => break,
                    accepted = listener.accept() => {
                        let (stream, _) = match accepted {
                            Ok(v) => v,
                            Err(_) => break,
                        };
                        let h = Arc::clone(&handler);
                        let cc = c.clone();
                        tokio::spawn(async move { h.serve(stream, cc).await; });
                    }
                }
            }
        });
        RedisLab {
            port,
            cancel,
            catalog,
        }
    }
}
