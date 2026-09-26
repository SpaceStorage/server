use crate::lifecycle::NodeState;
use crate::{logging, Node};
use spacestorage_admin_proto::{ErrorBody, ReloadReport};
use spacestorage_config::reload_class::{diff, ReloadClass};
use spacestorage_config::{parse_validate, ConfigError, NodeConfig, ValidateOptions};
use std::path::PathBuf;
use std::sync::Arc;

pub async fn reload(node: &Arc<Node>) -> Result<ReloadReport, ErrorBody> {
    let _guard = node.reload_lock.lock().await;
    if node.state.get() != NodeState::Ready {
        return Err(ErrorBody {
            code: "invalid_state".into(),
            message: "reload requires ready".into(),
            details: serde_json::json!({"state": node.state.get().as_str()}),
        });
    }
    let names: Vec<String> = node.handler_names();
    let path = node.config_path.clone();
    let text = tokio::fs::read_to_string(&path).await.map_err(|e| ErrorBody {
        code: "config_invalid".into(),
        message: format!("cannot read config: {e}"),
        details: serde_json::json!({}),
    })?;

    // Secret/filesystem readability checks are blocking I/O — off worker (T085).
    let (incoming, _) = parse_validate_blocking(text, path, names).await?;

    let running = node.config.load();
    let d = diff(&running, &incoming);
    let mut applied_live = Vec::new();
    let mut pending_restart = Vec::new();
    let mut changed = Vec::new();
    for c in &d.changed {
        changed.push(serde_json::json!({
            "setting": c.setting,
            "class": match c.class {
                ReloadClass::Live => "live",
                ReloadClass::RestartRequired => "restart_required",
            }
        }));
        match c.class {
            ReloadClass::Live => applied_live.push(c.setting.clone()),
            ReloadClass::RestartRequired => pending_restart.push(c.setting.clone()),
        }
    }

    // FR-014 / T075: apply only live fields; keep running structural settings.
    let mut next = (**running).clone();
    next.drain_timeout = incoming.drain_timeout;
    next.log_level = incoming.log_level.clone();
    next.admin_token_file = incoming.admin_token_file.clone();
    next.buffers = incoming.buffers.clone();
    // threads / entrypoints / log_format / disable_* stay from `running`.

    // Live log.level → tracing subscriber (FR-013 / T086).
    if running.log_level != next.log_level {
        logging::apply_level(&next.log_level);
    }

    node.buffers.apply_capacities(&next);
    node.config.store(Arc::new(next));
    node.effective.set_pending_restart(pending_restart.clone());
    Ok(ReloadReport {
        ok: true,
        changed,
        applied_live,
        pending_restart,
        warnings: vec![],
        errors: vec![],
    })
}

async fn parse_validate_blocking(
    text: String,
    path: PathBuf,
    names: Vec<String>,
) -> Result<(NodeConfig, Vec<ConfigError>), ErrorBody> {
    tokio::task::spawn_blocking(move || {
        let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        parse_validate(
            &text,
            &path,
            &[],
            &name_refs,
            ValidateOptions {
                check_secrets_readable: true,
                ..ValidateOptions::default()
            },
        )
    })
    .await
    .map_err(|e| ErrorBody {
        code: "config_invalid".into(),
        message: format!("reload validate join error: {e}"),
        details: serde_json::json!({}),
    })?
    .map_err(|errs| ErrorBody {
        code: "config_invalid".into(),
        message: "config invalid".into(),
        details: serde_json::json!({
            "errors": errs
        }),
    })
}
