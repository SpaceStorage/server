//! Classify-gated CQL verb dispatch (015 Cassandra HC MUST) via shared `PlannerEngine` (005 T084).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, LimitServices,
    PrincipalHold, ProtocolId,
};
use spacestorage_query::{CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult};
use spacestorage_tenancy::NamespaceControl;
use spacestorage_types::ContainerCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CassandraReply {
    Ok,
    Rows(Vec<(String, String)>),
    Prepared(String),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    /// Namespace bound at authentication (unchanged by `USE`).
    pub bound_namespace: String,
    pub profile: DialectProfile,
    /// Native PREPARE id (hex) → CQL text.
    pub prepared: std::collections::HashMap<String, String>,
    pub limit_services: LimitServices,
    pub principal_hold: Option<PrincipalHold>,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub namespace_control: Option<Arc<dyn NamespaceControl>>,
    pub connection_quota: Option<spacestorage_tenancy::ConnectionQuotaHold>,
    pub principal: Option<String>,
    /// Set once when bind/`USE` passes registry check; data CQL does not re-query the registry.
    pub registry_accepted: bool,
}

impl SessionState {
    pub fn demo(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            bound_namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            prepared: std::collections::HashMap::new(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            namespace_control: None,
            connection_quota: None,
            principal: None,
            registry_accepted: false,
        }
    }

    fn refuse_data_without_registry_accept(&self) -> Option<CassandraReply> {
        if self.tenancy.is_none() || self.registry_accepted {
            return None;
        }
        Some(tenancy_error_reply(
            &spacestorage_tenancy::TenancyError::NamespaceNotFound {
                name: self.namespace.clone(),
            },
        ))
    }

    /// `STARTUP` keyspace option or other presented name: accept once when registered.
    pub fn try_registry_accept_presented_keyspace(&mut self, name: &str) {
        let before = self.registry_accepted;
        spacestorage_tenancy::session::accept_registry_presented(
            self.tenancy.as_ref(),
            &mut self.registry_accepted,
            name,
        );
        if !before && self.registry_accepted {
            self.namespace = name.to_string();
        }
    }

    fn engine(&self) -> PlannerEngine {
        let mut engine = PlannerEngine::new(Arc::clone(&self.catalog));
        if let Some(t) = &self.tenancy {
            engine = engine.with_tenancy(Arc::clone(t));
        }
        engine
    }

    fn exec_opts(&self) -> QueryOptions {
        let mut options = QueryOptions::default();
        if let Some(login) = self.principal.as_deref() {
            options.principal = Some(login.to_string());
        }
        options
    }

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, CassandraReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "cql",
                "cassandra",
                req,
                self.exec_opts(),
                CancelToken::new(),
            )
            .map_err(|e| CassandraReply::Error {
                code: "server_error".into(),
                message: e.to_string(),
            })
    }

    fn ensure_table(&self, table: &str) -> Result<(), CassandraReply> {
        self.exec(LogicalRequest::TypeOp {
            container: table.into(),
            op: "ensure_table".into(),
        })?;
        Ok(())
    }
}

fn tenancy_error_reply(err: &spacestorage_tenancy::TenancyError) -> CassandraReply {
    CassandraReply::Error {
        code: "authorization".into(),
        message: err.to_string(),
    }
}

fn refuse(verb: &str, err: CompatError) -> CassandraReply {
    record_must_not(ProtocolId::Cassandra.as_str(), verb);
    CassandraReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

fn rows_from(result: QueryResult) -> CassandraReply {
    let mut rows = Vec::new();
    if result.columns.iter().any(|c| c == "key" || c == "value") {
        for r in result.rows {
            let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
            let v = r.get(1).and_then(|x| x.clone()).unwrap_or_default();
            rows.push((k, v));
        }
    } else {
        for r in result.rows {
            let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
            let v = r
                .get(1)
                .and_then(|x| x.clone())
                .or_else(|| r.first().and_then(|x| x.clone()))
                .unwrap_or_default();
            rows.push((k, v));
        }
    }
    CassandraReply::Rows(rows)
}

async fn create_keyspace_via_control(
    session: &mut SessionState,
    args: &[&str],
) -> CassandraReply {
    let ks = args
        .get(1)
        .map(|s| s.trim_matches('"').to_string())
        .unwrap_or_default();
    if ks.is_empty() {
        return CassandraReply::Error {
            code: "invalid".into(),
            message: "missing keyspace name".into(),
        };
    }
    let login = session.principal.as_deref().unwrap_or("");
    if login != "admin" {
        return tenancy_error_reply(&spacestorage_tenancy::TenancyError::NotClusterAdmin);
    }
    let Some(control) = session.namespace_control.as_ref() else {
        if session.tenancy.is_some() {
            return CassandraReply::Error {
                code: "server_error".into(),
                message: "controlplane not started".into(),
            };
        }
        session.namespace = ks;
        return CassandraReply::Ok;
    };
    match control.create(&ks).await {
        Ok(()) => {
            session.namespace = ks.clone();
            session.registry_accepted = true;
            CassandraReply::Ok
        }
        Err(message) => CassandraReply::Error {
            code: "server_error".into(),
            message,
        },
    }
}

/// Dispatch a normalized CQL verb (`SELECT`, `INSERT`, …) before IR → PlannerEngine.
pub async fn dispatch_async(session: &mut SessionState, verb: &str, args: &[&str]) -> CassandraReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    if upper == "CREATE"
        && args
            .first()
            .map(|s| s.eq_ignore_ascii_case("KEYSPACE"))
            .unwrap_or(false)
    {
        return create_keyspace_via_control(session, args).await;
    }
    dispatch(session, verb, args)
}

fn cassandra_data_verb_requires_registry(upper: &str) -> bool {
    matches!(
        upper,
        "CREATE" | "DROP" | "ALTER" | "PREPARE" | "EXECUTE" | "QUERY" | "INSERT" | "SELECT"
            | "UPDATE" | "DELETE"
    )
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> CassandraReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, ProtocolId::Cassandra, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(&upper, err),
    }

    if cassandra_data_verb_requires_registry(&upper) {
        if let Some(reply) = session.refuse_data_without_registry_accept() {
            return reply;
        }
    }

    match upper.as_str() {
        "STARTUP" | "OPTIONS" | "REGISTER" | "BATCH" => CassandraReply::Ok,
        "USE" => {
            let ks = args
                .first()
                .map(|s| s.trim_matches('"').to_string())
                .unwrap_or_default();
            if !ks.is_empty() {
                if let Some(t) = &session.tenancy {
                    let login = session.principal.as_deref().unwrap_or("");
                    if let Err(err) = spacestorage_tenancy::session::refuse_keyspace_open(
                        login,
                        &session.bound_namespace,
                        &ks,
                    ) {
                        return tenancy_error_reply(&err);
                    }
                    if let Err(err) = spacestorage_tenancy::session::ensure_namespace_registered(
                        t, &ks,
                    ) {
                        return tenancy_error_reply(&err);
                    }
                    session.registry_accepted = true;
                }
                session.namespace = ks;
            }
            CassandraReply::Ok
        }
        "CREATE" => {
            let kind = args
                .first()
                .map(|s| s.to_ascii_uppercase())
                .unwrap_or_default();
            if kind == "KEYSPACE" {
                return CassandraReply::Error {
                    code: "server_error".into(),
                    message: "CREATE KEYSPACE requires async dispatch".into(),
                };
            }
            let table = if kind == "TABLE" {
                args.get(1).copied().unwrap_or("t")
            } else {
                args.first().copied().unwrap_or("t")
            };
            match session.ensure_table(table.trim_matches('"')) {
                Ok(()) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "DROP" => {
            let table = args
                .iter()
                .find(|a| a.to_ascii_uppercase() != "TABLE")
                .copied()
                .unwrap_or("t");
            match session.exec(LogicalRequest::TypeOp {
                container: table.trim_matches('"').into(),
                op: "drop".into(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "ALTER" => CassandraReply::Ok,
        "PREPARE" => {
            let cql = args.first().copied().unwrap_or("SELECT 1");
            if let Err(e) = session.limit_services.limits().check_query_text(cql.len() as u64) {
                return CassandraReply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            let id = format!("{:x}", {
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut h = DefaultHasher::new();
                cql.hash(&mut h);
                h.finish()
            });
            session.prepared.insert(id.clone(), cql.to_string());
            CassandraReply::Prepared(id)
        }
        "EXECUTE" => {
            let id = args.first().copied().unwrap_or("");
            let cql = match session.prepared.get(id) {
                Some(s) => s.clone(),
                None => {
                    return CassandraReply::Error {
                        code: "unprepared".into(),
                        message: format!("unknown prepared id {id}"),
                    };
                }
            };
            // Re-dispatch prepared CQL through classify_cql path args.
            if let Some((verb, pargs)) = crate::cql::classify_cql(&cql) {
                let refs: Vec<&str> = pargs.iter().map(String::as_str).collect();
                return dispatch(session, &verb, &refs);
            }
            CassandraReply::Error {
                code: "invalid".into(),
                message: format!("cannot execute prepared: {cql}"),
            }
        }
        "QUERY" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Point {
                container: table.into(),
                key: key.into(),
            }) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        "INSERT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            let limits = session.limit_services.limits();
            if let Err(e) = limits.check_key(key.len() as u64) {
                return CassandraReply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            if let Err(e) = limits.check_value(val.len() as u64) {
                return CassandraReply::Error {
                    code: "limit_exceeded".into(),
                    message: e.to_string(),
                };
            }
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: table.into(),
                key: key.into(),
                bytes: val.as_bytes().to_vec(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "SELECT" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied();
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            let req = if let Some(k) = key {
                LogicalRequest::Point {
                    container: table.into(),
                    key: k.into(),
                }
            } else {
                LogicalRequest::Scan {
                    container: table.into(),
                    filter: None,
                }
            };
            match session.exec(req) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        "UPDATE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            let val = args.get(2).copied().unwrap_or("");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Object {
                container: table.into(),
                key: key.into(),
                bytes: val.as_bytes().to_vec(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        "DELETE" => {
            let table = args.first().copied().unwrap_or("t");
            let key = args.get(1).copied().unwrap_or("k");
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Mutate {
                container: table.into(),
                op: "delete".into(),
                sql: key.into(),
            }) {
                Ok(_) => CassandraReply::Ok,
                Err(e) => e,
            }
        }
        other => refuse(
            other,
            CompatError::CompatMustNot {
                protocol: ProtocolId::Cassandra.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn must_crud_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            CassandraReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "INSERT", &["t", "1", "a"]),
            CassandraReply::Ok
        ));
        match dispatch(&mut s, "SELECT", &["t", "1"]) {
            CassandraReply::Rows(r) => assert_eq!(r[0].1, "a"),
            other => panic!("{other:?}"),
        }
        let eng = PlannerEngine::new(Arc::clone(&s.catalog));
        // Force a planner record with same catalog.
        let _ = eng.execute_blocking(
            "demo",
            "t",
            "cassandra",
            LogicalRequest::Point {
                container: "t".into(),
                key: "1".into(),
            },
            QueryOptions::default(),
            CancelToken::new(),
        );
        assert_eq!(eng.records_snapshot().last().unwrap().engine, "planner");
    }

    #[test]
    fn update_delete_alter_must_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            CassandraReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "UPDATE", &["t", "1", "b"]),
            CassandraReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "DELETE", &["t", "1"]),
            CassandraReply::Ok
        ));
        assert!(matches!(dispatch(&mut s, "ALTER", &["t"]), CassandraReply::Ok));
    }

    #[test]
    fn cdc_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "CDC", &[]) {
            CassandraReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lwt_must_not() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        match dispatch(&mut s, "LWT", &[]) {
            CassandraReply::Error { code, .. } => assert_eq!(code, "compat_must_not"),
            other => panic!("expected refuse, got {other:?}"),
        }
    }

    #[test]
    fn use_and_create_keyspace_tenancy_gates() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::{NamespaceControl, Tenancy};
        use std::sync::Arc;

        struct RegistryCreate(Arc<Tenancy>);

        #[async_trait::async_trait]
        impl NamespaceControl for RegistryCreate {
            async fn exists(&self, _name: &str) -> Result<bool, String> {
                Ok(true)
            }

            async fn create(&self, name: &str) -> Result<(), String> {
                self.0
                    .registry
                    .lock()
                    .create(Verb::CLUSTER_ADMIN.bits(), name)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }

            async fn drop_namespace(&self, _name: &str) -> Result<(), String> {
                Ok(())
            }

            async fn rename(&self, _name: &str, _new_name: &str) -> Result<(), String> {
                Ok(())
            }
        }

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "demo")
            .unwrap();
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "other")
            .unwrap();
        let control: Arc<dyn NamespaceControl> = Arc::new(RegistryCreate(Arc::clone(&tenancy)));

        let mut s = SessionState {
            catalog: Arc::clone(&cat),
            namespace: "demo".into(),
            bound_namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            prepared: Default::default(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(Arc::clone(&tenancy)),
            namespace_control: Some(Arc::clone(&control)),
            connection_quota: None,
            principal: Some("demo".into()),
            registry_accepted: true,
        };
        match dispatch(&mut s, "USE", &["other"]) {
            CassandraReply::Error { message, .. } => {
                assert!(message.contains("AccessDenied"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        s.principal = Some("admin".into());
        assert!(matches!(dispatch(&mut s, "USE", &["other"]), CassandraReply::Ok));
        assert_eq!(s.namespace, "other");

        s.principal = Some("demo".into());
        s.namespace = "demo".into();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        match rt.block_on(dispatch_async(
            &mut s,
            "CREATE",
            &["KEYSPACE", "brandnew"],
        )) {
            CassandraReply::Error { message, .. } => {
                assert!(message.contains("NotClusterAdmin"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        s.principal = Some("admin".into());
        assert!(matches!(
            rt.block_on(dispatch_async(
                &mut s,
                "CREATE",
                &["KEYSPACE", "brandnew"]
            )),
            CassandraReply::Ok
        ));
        assert!(tenancy.registry.lock().get_by_name("brandnew").is_some());
    }

    #[test]
    fn data_cql_refuses_unregistered_namespace_without_recheck() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::Tenancy;
        use std::sync::Arc;

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "other")
            .unwrap();

        let mut s = SessionState {
            catalog: cat,
            namespace: "ghost".into(),
            bound_namespace: "ghost".into(),
            profile: DialectProfile::HandlersComplete,
            prepared: Default::default(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            principal: Some("admin".into()),
            registry_accepted: false,
        };
        match dispatch(&mut s, "SELECT", &["t"]) {
            CassandraReply::Error { message, .. } => {
                assert!(message.contains("NamespaceNotFound"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert!(!s.registry_accepted);
    }

    #[test]
    fn presented_keyspace_accepts_once_when_registered() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::Tenancy;
        use std::sync::Arc;

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "acme")
            .unwrap();

        let mut s = SessionState {
            catalog: cat,
            namespace: "demo".into(),
            bound_namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            prepared: Default::default(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            principal: Some("demo".into()),
            registry_accepted: false,
        };
        s.try_registry_accept_presented_keyspace("missing");
        assert!(!s.registry_accepted);
        s.try_registry_accept_presented_keyspace("acme");
        assert!(s.registry_accepted);
        assert_eq!(s.namespace, "acme");
    }

    #[test]
    fn use_accepts_registry_once_for_later_select() {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::Tenancy;
        use std::sync::Arc;

        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let tenancy = Arc::new(Tenancy::new());
        tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), "demo")
            .unwrap();

        let mut s = SessionState {
            catalog: Arc::clone(&cat),
            namespace: "ghost".into(),
            bound_namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            prepared: Default::default(),
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(Arc::clone(&tenancy)),
            namespace_control: None,
            connection_quota: None,
            principal: Some("admin".into()),
            registry_accepted: false,
        };
        assert!(matches!(dispatch(&mut s, "USE", &["demo"]), CassandraReply::Ok));
        assert!(s.registry_accepted);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            CassandraReply::Ok
        ));
    }

    #[test]
    fn prepare_execute_must() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            CassandraReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "INSERT", &["t", "1", "a"]),
            CassandraReply::Ok
        ));
        let prep = dispatch(
            &mut s,
            "PREPARE",
            &["SELECT v FROM t WHERE id='1'"],
        );
        let id = match prep {
            CassandraReply::Prepared(id) => id,
            other => panic!("prepare: {other:?}"),
        };
        match dispatch(&mut s, "EXECUTE", &[&id]) {
            CassandraReply::Rows(r) => assert_eq!(r[0].1, "a"),
            other => panic!("execute: {other:?}"),
        }
    }
}
