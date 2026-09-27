//! External KMS live unwrap conformance (full v1 Track 2).

#![cfg(feature = "complete-product")]

use spacestorage_crypto::{ExternalKmsProvider, MasterProvider};

#[tokio::test]
async fn external_kms_local_cache_unwrap() {
    let kms = ExternalKmsProvider::new("http://127.0.0.1:8200/v1/transit", "cluster-master", "s.t");
    let material = [3u8; 32];
    kms.put_key("cluster-master", material).await;
    let got = kms.resolve_master().await.unwrap();
    assert_eq!(got.as_slice(), &material);
    assert_eq!(kms.provider_kind(), "external_kms");
}
