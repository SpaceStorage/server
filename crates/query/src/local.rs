//! LocalEngine — RF=1 / unit-test path without internodes.

use crate::cancel::CancelToken;
use crate::catalog_exec::{execute_adhoc_sql, QueryResult, SharedCatalog};
use crate::engine::QueryEngine;
use crate::error::ExecError;
use crate::options::QueryOptions;
use crate::request::LogicalRequest;

pub struct LocalEngine {
    catalog: SharedCatalog,
}

impl LocalEngine {
    pub fn new(catalog: SharedCatalog) -> Self {
        Self { catalog }
    }
}

#[async_trait::async_trait]
impl QueryEngine for LocalEngine {
    async fn execute(
        &self,
        namespace: &str,
        _session: &str,
        _protocol: &str,
        request: LogicalRequest,
        _options: QueryOptions,
        cancel: CancelToken,
    ) -> Result<QueryResult, ExecError> {
        cancel.check()?;
        match request {
            LogicalRequest::AdHocSql { sql }
            | LogicalRequest::Ddl { sql, .. }
            | LogicalRequest::Mutate { sql, .. } => execute_adhoc_sql(&self.catalog, namespace, &sql),
            LogicalRequest::Scan { container, filter } => {
                let sql = match filter {
                    Some(f) => format!("SELECT * FROM {container} WHERE {f}"),
                    None => format!("SELECT * FROM {container}"),
                };
                execute_adhoc_sql(&self.catalog, namespace, &sql)
            }
            other => Err(ExecError::NotSupported(format!("local:{other:?}"))),
        }
    }
}
