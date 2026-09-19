//! SpaceStorage nginx-style configuration crate (no Tokio).

pub mod ast;
pub mod error;
pub mod lexer;
pub mod model;
pub mod parser;
pub mod reload_class;
pub mod resolve;
pub mod validate;

pub use error::{ConfigError, ErrorCode};
pub use model::{buffer_spec, NodeConfig, BUILTIN_BUFFERS};
pub use reload_class::{ConfigDiff, ReloadClass};
pub use validate::{parse_validate, ValidateOptions};

use std::path::Path;

/// Parse a config file to AST then resolve+validate.
pub fn load_file(
    path: &Path,
    overrides: &[String],
    handler_names: &[&str],
    opts: ValidateOptions,
) -> Result<(NodeConfig, Vec<ConfigError>), Vec<ConfigError>> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        vec![ConfigError::simple(
            path.display().to_string(),
            0,
            0,
            "file",
            ErrorCode::Syntax,
            format!("cannot read config: {e}"),
        )]
    })?;
    parse_validate(&text, path, overrides, handler_names, opts)
}
