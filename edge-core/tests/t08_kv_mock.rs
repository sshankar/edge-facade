//! T8 (mock): KV round-trip on the mock platform — pre-validates the core KV
//! API before the real platform adapters land in M4.

use bytes::Bytes;
use edge_core::testing::MockContextBuilder;
use edge_core::{Error, KvError};

#[tokio::test]
async fn kv_round_trip() {
    let mock = MockContextBuilder::new()
        .kv_entry("default", "k1", "v1")
        .build();
    let ctx = mock.context();
    let kv = ctx.kv();

    let v = kv.get("k1").await.unwrap().expect("seeded value");
    assert_eq!(v.text().await.unwrap().as_deref(), Some("v1"));

    kv.put("k2", "hello 世界").await.unwrap();
    let v = kv.get("k2").await.unwrap().unwrap();
    assert_eq!(v.text().await.unwrap().as_deref(), Some("hello 世界"));

    kv.delete("k1").await.unwrap();
    assert!(kv.get("k1").await.unwrap().is_none());

    // Ops recorded with store name for the conformance suite.
    assert_eq!(mock.records().kv_ops[0], "get:default:k1");
    assert!(mock
        .records()
        .kv_ops
        .contains(&"put:default:k2".to_string()));
}

#[tokio::test]
async fn kv_binary_values_and_invalid_utf8() {
    let mock = MockContextBuilder::new().build();
    let ctx = mock.context();
    let kv = ctx.kv();

    kv.put("bin", Bytes::from_static(&[0xff, 0x00]))
        .await
        .unwrap();
    let v = kv.get("bin").await.unwrap().unwrap();
    assert_eq!(v.bytes().await.unwrap(), Bytes::from_static(&[0xff, 0x00]));

    let v = kv.get("bin").await.unwrap().unwrap();
    assert_eq!(v.text().await.unwrap(), None);
}

#[tokio::test]
async fn kv_json_round_trip() {
    let mock = MockContextBuilder::new().build();
    let ctx = mock.context();
    let kv = ctx.kv();

    kv.put(
        "cfg",
        serde_json::to_vec(&serde_json::json!({ "a": 1 })).unwrap(),
    )
    .await
    .unwrap();

    let v = kv.get("cfg").await.unwrap().unwrap();
    let parsed: serde_json::Value = v.json().await.unwrap().unwrap();
    assert_eq!(parsed["a"], 1);
}

#[tokio::test]
async fn kv_fault_injection() {
    let mock = MockContextBuilder::new().fail_kv().build();
    let ctx = mock.context();
    let kv = ctx.kv();

    assert!(matches!(
        kv.put("k", "v").await,
        Err(Error::Kv(KvError::Platform(_)))
    ));
    assert!(matches!(kv.get("k").await, Err(Error::Kv(_))));
}
