//! Classify-gated ClickHouse SQL verbs via shared `PlannerEngine` (005 T085).

use std::sync::{Arc, RwLock};

use spacestorage_compat::{
    classify_outcome, record_must_not, ClassifyOutcome, CompatError, DialectProfile, LimitServices,
    PrincipalHold, ProtocolId,
};
use spacestorage_query::{
    AggFn, CancelToken, LogicalRequest, PlannerEngine, QueryOptions, QueryResult,
};
use spacestorage_tenancy::{NamespaceControl, Result as TenancyResult};
use spacestorage_types::ContainerCatalog;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickHouseReply {
    Ok,
    Rows(Vec<(String, String)>),
    Error { code: String, message: String },
}

pub struct SessionState {
    pub catalog: Arc<RwLock<ContainerCatalog>>,
    pub namespace: String,
    pub profile: DialectProfile,
    pub protocol: ProtocolId,
    pub limit_services: LimitServices,
    pub principal_hold: Option<PrincipalHold>,
    pub tenancy: Option<Arc<spacestorage_tenancy::Tenancy>>,
    pub namespace_control: Option<Arc<dyn NamespaceControl>>,
    pub connection_quota: Option<spacestorage_tenancy::ConnectionQuotaHold>,
    /// Namespace currently holding the `connections` quota (when tenancy is attached).
    pub connection_charged_namespace: Option<String>,
    pub principal: Option<String>,
    /// Set once when hello/auth passes registry check; data SQL does not re-query the registry.
    pub registry_accepted: bool,
}

impl SessionState {
    pub fn demo_native(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouse,
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: None,
            registry_accepted: false,
        }
    }

    pub fn demo_http(catalog: Arc<RwLock<ContainerCatalog>>) -> Self {
        Self {
            catalog,
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouseHttp,
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: None,
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: None,
            registry_accepted: false,
        }
    }

    /// One session, one connections hold — admit on `namespace` first, then release the previous hold (T096).
    pub fn reconcile_connection_quota_for_namespace(
        &mut self,
        namespace: &str,
    ) -> TenancyResult<()> {
        if self.tenancy.is_none() {
            return Ok(());
        }
        if self.connection_charged_namespace.as_deref() == Some(namespace) {
            return Ok(());
        }
        let Some(t) = self.tenancy.as_ref() else {
            return Ok(());
        };
        let new_hold = t.admit_connection(namespace)?;
        self.connection_quota = None;
        self.connection_charged_namespace = None;
        if let Some(hold) = new_hold {
            self.connection_charged_namespace = Some(namespace.to_string());
            self.connection_quota = Some(hold);
        }
        Ok(())
    }

    pub(crate) fn refuse_data_without_registry_accept(&self) -> Option<ClickHouseReply> {
        if self.tenancy.is_none() || self.registry_accepted {
            return None;
        }
        Some(ClickHouseReply::Error {
            code: "81".into(),
            message: spacestorage_tenancy::TenancyError::NamespaceNotFound {
                name: self.namespace.clone(),
            }
            .to_string(),
        })
    }

    /// Native hello / HTTP `database=`: accept and charge when the presented name is registered (T096).
    pub fn try_registry_accept_presented_database(
        &mut self,
        name: &str,
    ) -> TenancyResult<()> {
        if self.registry_accepted {
            return Ok(());
        }
        let Some(t) = self.tenancy.as_ref() else {
            return Ok(());
        };
        if spacestorage_tenancy::session::ensure_namespace_registered(t, name).is_err() {
            return Ok(());
        }
        match t.admit_connection(name)? {
            Some(hold) => {
                self.connection_quota = Some(hold);
                self.connection_charged_namespace = Some(name.to_string());
                self.namespace = name.to_string();
                self.registry_accepted = true;
            }
            None => {
                self.namespace = name.to_string();
                self.registry_accepted = true;
            }
        }
        Ok(())
    }

    /// `USE` against the cluster registry: charge the new name before releasing the previous hold (T096).
    pub fn try_use_registry_namespace(&mut self, name: &str) -> TenancyResult<()> {
        if self.tenancy.is_none() {
            self.namespace = name.to_string();
            return Ok(());
        }
        let Some(t) = self.tenancy.as_ref() else {
            self.namespace = name.to_string();
            return Ok(());
        };
        spacestorage_tenancy::session::ensure_namespace_registered(t, name)?;
        if self.registry_accepted && self.namespace == name {
            return Ok(());
        }
        self.reconcile_connection_quota_for_namespace(name)?;
        self.namespace = name.to_string();
        self.registry_accepted = true;
        Ok(())
    }

    fn engine(&self) -> PlannerEngine {
        let distributed = matches!(self.profile, DialectProfile::CompleteProduct);
        let mut engine =
            PlannerEngine::with_distributed(Arc::clone(&self.catalog), distributed);
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

    fn exec(&self, req: LogicalRequest) -> Result<QueryResult, ClickHouseReply> {
        self.engine()
            .execute_blocking(
                &self.namespace,
                "ch",
                "clickhouse",
                req,
                self.exec_opts(),
                CancelToken::new(),
            )
            .map_err(|e| ClickHouseReply::Error {
                code: "server_error".into(),
                message: e.to_string(),
            })
    }

    fn ensure_table(&self, table: &str) -> Result<(), ClickHouseReply> {
        self.exec(LogicalRequest::TypeOp {
            container: table.into(),
            op: "ensure_table".into(),
        })?;
        Ok(())
    }
}

fn refuse(protocol: ProtocolId, verb: &str, err: CompatError) -> ClickHouseReply {
    record_must_not(protocol.as_str(), verb);
    ClickHouseReply::Error {
        code: err.code().into(),
        message: err.to_string(),
    }
}

fn rows_from(result: QueryResult) -> ClickHouseReply {
    let mut rows = Vec::new();
    for r in result.rows {
        let k = r.first().and_then(|x| x.clone()).unwrap_or_default();
        let v = r.get(1).and_then(|x| x.clone()).unwrap_or_default();
        rows.push((k, v));
    }
    ClickHouseReply::Rows(rows)
}

pub fn dispatch(session: &mut SessionState, verb: &str, args: &[&str]) -> ClickHouseReply {
    let upper = verb.to_ascii_uppercase().replace('-', "_");
    match classify_outcome(session.profile, session.protocol, &upper) {
        ClassifyOutcome::Must => {}
        ClassifyOutcome::MustNot(err) => return refuse(session.protocol, &upper, err),
    }

    if let Some(reply) = session.refuse_data_without_registry_accept() {
        return reply;
    }

    match upper.as_str() {
        "CREATE" => {
            let table = args
                .iter()
                .find(|a| {
                    !matches!(
                        a.to_ascii_uppercase().as_str(),
                        "TABLE" | "IF" | "NOT" | "EXISTS"
                    )
                })
                .copied()
                .unwrap_or("t");
            match session.ensure_table(table) {
                Ok(()) => ClickHouseReply::Ok,
                Err(e) => e,
            }
        }
        "INSERT" => {
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
                Ok(_) => ClickHouseReply::Ok,
                Err(e) => e,
            }
        }
        "SELECT" | "SELECT_WHERE" => {
            let table = args.first().copied().unwrap_or("t");
            let filter = args.get(1).map(|s| (*s).to_string());
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Scan {
                container: table.into(),
                filter,
            }) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        "GROUP_BY" | "SELECT_GROUP_BY" => {
            let table = args.first().copied().unwrap_or("t");
            let group_col = args.get(1).copied().unwrap_or("id");
            let agg_col = args.get(2).copied().unwrap_or(group_col);
            if let Err(e) = session.ensure_table(table) {
                return e;
            }
            match session.exec(LogicalRequest::Aggregate {
                container: table.into(),
                func: AggFn::Count,
                group_by: Some(group_col.into()),
                column: Some(agg_col.into()),
                filter: None,
            }) {
                Ok(r) => rows_from(r),
                Err(e) => e,
            }
        }
        other => refuse(
            session.protocol,
            other,
            CompatError::CompatMustNot {
                protocol: session.protocol.as_str().into(),
                verb: other.into(),
            },
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_where_smoke() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_native(cat);
        assert!(matches!(
            dispatch(&mut s, "CREATE", &["TABLE", "t"]),
            ClickHouseReply::Ok
        ));
        assert!(matches!(
            dispatch(&mut s, "INSERT", &["t", "1", "x"]),
            ClickHouseReply::Ok
        ));
        match dispatch(&mut s, "SELECT_WHERE", &["t", "1"]) {
            ClickHouseReply::Rows(r) => assert_eq!(r.len(), 1),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn data_sql_refuses_unregistered_database_when_tenancy_attached() {
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
            catalog: cat,
            namespace: "ghost".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouse,
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: Some("demo".into()),
            registry_accepted: false,
        };
        match dispatch(&mut s, "SELECT", &["t"]) {
            ClickHouseReply::Error { message, .. } => {
                assert!(message.contains("NamespaceNotFound"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    fn tenancy_session(tenancy: Arc<spacestorage_tenancy::Tenancy>) -> SessionState {
        SessionState {
            catalog: Arc::new(RwLock::new(ContainerCatalog::new())),
            namespace: "demo".into(),
            profile: DialectProfile::HandlersComplete,
            protocol: ProtocolId::ClickHouse,
            limit_services: LimitServices::built_in(),
            principal_hold: None,
            tenancy: Some(tenancy),
            namespace_control: None,
            connection_quota: None,
            connection_charged_namespace: None,
            principal: Some("demo".into()),
            registry_accepted: false,
        }
    }

    fn register_with_connection_quota(
        tenancy: &spacestorage_tenancy::Tenancy,
        name: &str,
        limit: u64,
    ) {
        use spacestorage_authz::Verb;
        use spacestorage_tenancy::{QuotaSpec, QuotaUnit};

        let rec = tenancy
            .registry
            .lock()
            .create(Verb::CLUSTER_ADMIN.bits(), name)
            .unwrap();
        tenancy
            .registry
            .lock()
            .replace_quotas(
                Verb::CLUSTER_ADMIN.bits(),
                rec.id,
                vec![QuotaSpec {
                    unit: QuotaUnit::Connections,
                    limit,
                    datatype: None,
                }],
            )
            .unwrap();
    }

    #[test]
    fn presented_database_charged_demo_full_does_not_refuse_hello() {
        use spacestorage_tenancy::{QuotaUnit, Tenancy};

        let tenancy = Arc::new(Tenancy::new());
        register_with_connection_quota(&tenancy, "demo", 1);
        register_with_connection_quota(&tenancy, "acme", 1);
        let demo_id = tenancy.registry.lock().get_by_name("demo").unwrap().id;
        tenancy
            .usage
            .set(demo_id, QuotaUnit::Connections, &None, 1);

        let mut s = tenancy_session(Arc::clone(&tenancy));
        s.try_registry_accept_presented_database("acme").unwrap();
        assert_eq!(s.connection_charged_namespace.as_deref(), Some("acme"));
        assert!(s.connection_quota.is_some());
        assert!(s.registry_accepted);
        assert_eq!(s.namespace, "acme");
    }

    #[test]
    fn unregistered_presented_database_takes_no_connection_hold() {
        use spacestorage_tenancy::Tenancy;

        let tenancy = Arc::new(Tenancy::new());
        register_with_connection_quota(&tenancy, "demo", 10);

        let mut s = tenancy_session(Arc::clone(&tenancy));
        s.try_registry_accept_presented_database("ghost").unwrap();
        assert!(!s.registry_accepted);
        assert!(s.connection_quota.is_none());
        assert!(s.connection_charged_namespace.is_none());
    }

    #[test]
    fn use_quota_exceeded_keeps_previous_hold_and_namespace() {
        use spacestorage_tenancy::{QuotaUnit, Tenancy, TenancyError};

        let tenancy = Arc::new(Tenancy::new());
        register_with_connection_quota(&tenancy, "acme", 1);
        register_with_connection_quota(&tenancy, "beta", 1);
        let beta_id = tenancy.registry.lock().get_by_name("beta").unwrap().id;
        tenancy
            .usage
            .set(beta_id, QuotaUnit::Connections, &None, 1);

        let mut s = tenancy_session(Arc::clone(&tenancy));
        s.try_registry_accept_presented_database("acme").unwrap();
        assert_eq!(s.namespace, "acme");
        let charged = s.connection_charged_namespace.clone();
        assert_eq!(charged.as_deref(), Some("acme"));

        let err = s.try_use_registry_namespace("beta").unwrap_err();
        assert!(matches!(err, TenancyError::QuotaExceeded { .. }));
        assert_eq!(s.namespace, "acme");
        assert!(s.registry_accepted);
        assert_eq!(s.connection_charged_namespace, charged);
        assert!(s.connection_quota.is_some());
    }

    #[test]
    fn group_by_and_dictionary_refused() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        let mut s = SessionState::demo_http(cat);
        for v in ["GROUP_BY", "DICTIONARY"] {
            match dispatch(&mut s, v, &[]) {
                ClickHouseReply::Error { .. } => {}
                other => panic!("{v} must refuse, got {other:?}"),
            }
        }
    }

    #[cfg(feature = "query-distributed")]
    #[test]
    fn complete_product_group_by_both_protocols() {
        let cat = Arc::new(RwLock::new(ContainerCatalog::new()));
        for mut s in [
            SessionState::demo_native(Arc::clone(&cat)),
            SessionState::demo_http(Arc::clone(&cat)),
        ] {
            s.profile = DialectProfile::CompleteProduct;
            assert!(matches!(
                dispatch(&mut s, "CREATE", &["TABLE", "t"]),
                ClickHouseReply::Ok
            ));
            assert!(matches!(
                dispatch(&mut s, "INSERT", &["t", "1", r#"{"id":"a","n":1}"#]),
                ClickHouseReply::Ok
            ));
            match dispatch(&mut s, "GROUP_BY", &["t", "id", "n"]) {
                ClickHouseReply::Rows(_) | ClickHouseReply::Ok => {}
                other => panic!("{:?} group_by: {other:?}", s.protocol),
            }
        }
    }
}
