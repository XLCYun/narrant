use super::{CredentialStore, EphemeralCredentialStore};

/// WDIO 凭据存储只在当前进程内完成完整生命周期，不依赖系统钥匙串。
#[test]
fn ephemeral_store_round_trip_stays_in_process() {
    let store = EphemeralCredentialStore::default();

    assert_eq!(store.load().unwrap(), None);
    store.save("credentials").unwrap();
    assert_eq!(store.load().unwrap().as_deref(), Some("credentials"));
    store.delete().unwrap();
    assert_eq!(store.load().unwrap(), None);
}
