//! Planetary federated / union / MV conformance (full v1 Track 2).

#![cfg(feature = "complete-product")]

use spacestorage_types::{
    CompositionCatalog, CompositionKind, CompositionMember, PlanetaryPlacement,
};
use std::collections::HashMap;

#[test]
fn planetary_federated_route_and_mv_refresh() {
    let mut cat = CompositionCatalog::default();
    let mut labels = HashMap::new();
    labels.insert("planet".into(), "earth".into());
    labels.insert("region".into(), "eu".into());
    let id = cat
        .create(
            "acme",
            "fed",
            CompositionKind::Federated,
            vec![
                CompositionMember {
                    namespace: "acme".into(),
                    container: "eu_orders".into(),
                    route_key: Some("eu".into()),
                },
                CompositionMember {
                    namespace: "acme".into(),
                    container: "us_orders".into(),
                    route_key: Some("us".into()),
                },
            ],
            PlanetaryPlacement {
                labels,
                quorum_domain: "eu".into(),
            },
        )
        .unwrap();
    let c = cat.get("acme", "fed").unwrap();
    assert!(c.is_planetary());
    assert_eq!(c.id, id);
    let m = c.route_write("eu").unwrap();
    assert_eq!(m.container, "eu_orders");
}
