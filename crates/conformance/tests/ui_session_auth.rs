//! UI `/ui/*` uses `014` SessionToken (AuthLogin), not interim static-token→admin.

#![cfg(feature = "complete-product")]

use spacestorage_authz::SessionTokenStore;
use spacestorage_admin_ui::{AdminUiState, UiPrincipal};
use std::sync::Arc;
use std::time::Duration;

#[test]
fn session_token_resolves_cluster_admin_for_ui() {
    let mut store = SessionTokenStore::new();
    let (tok, _) = store.issue(uuid::Uuid::now_v7(), "admin", true, 1, Duration::from_secs(60));
    let store = Arc::new(std::sync::RwLock::new(store));
    let store2 = Arc::clone(&store);
    let ui = AdminUiState {
        slice11_enabled: true,
        map: spacestorage_admin_ui::MapComposer::empty(),
        console: spacestorage_admin_ui::ConsoleService::new(true),
        resolve_principal: Arc::new(move |t| {
            let Ok(s) = store2.read() else {
                return None;
            };
            s.resolve(t).map(|rec| {
                if rec.cluster_admin {
                    UiPrincipal::cluster_admin(rec.login)
                } else {
                    UiPrincipal::unprivileged(rec.login)
                }
            })
        }),
    };
    assert!(ui.resolve_principal.as_ref()(&tok).unwrap().cluster_admin);
    assert!(ui.resolve_principal.as_ref()("lab-admin-token").is_none());
}
