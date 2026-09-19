use crate::lifecycle::NodeState;
use crate::Node;
use spacestorage_admin_proto::{ErrorBody, ReloadReport};
use spacestorage_config::reload_class::{diff, ReloadClass};
use spacestorage_config::{parse_validate, ValidateOptions};
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
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let text = std::fs::read_to_string(&node.config_path).map_err(|e| ErrorBody {
        code: "config_invalid".into(),
        message: format!("cannot read config: {e}"),
        details: serde_json::json!({}),
    })?;
    let (incoming, _) = parse_validate(
        &text,
        &node.config_path,
        &[],
        &name_refs,
        ValidateOptions {
            check_secrets_readable: true,
            ..ValidateOptions::default()
        },
    )
    .map_err(|errs| ErrorBody {
        code: "config_invalid".into(),
        message: "config invalid".into(),
        details: serde_json::json!({
            "errors": errs
        }),
    })?;

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
    node.buffers.apply_capacities(&incoming);
    node.config.store(Arc::new(incoming));
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
