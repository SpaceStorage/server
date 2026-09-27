use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "spacestorage", about = "SpaceStorage admin CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Validate {
        config: PathBuf,
        #[arg(long = "set")]
        set: Vec<String>,
    },
    Status {
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// List recent query executions (005).
    Executions {
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Explain a SQL statement without executing (005).
    Explain {
        sql: String,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// List / wait / cancel async query jobs (005 slice 8).
    Jobs {
        #[command(subcommand)]
        action: Option<JobsAction>,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Data migration / transform / backup job control (010).
    Job {
        #[command(subcommand)]
        action: JobAction,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Evacuate / copy / move a container (010).
    Migrate {
        #[arg(long = "from")]
        from: String,
        #[arg(long = "to-node")]
        to_node: Option<String>,
        #[arg(long = "to-namespace")]
        to_namespace: Option<String>,
        #[arg(long = "name")]
        name: Option<String>,
        #[arg(long = "policy", default_value = "copy")]
        policy: String,
        #[arg(long = "strategy", default_value = "live")]
        strategy: String,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Rewrite a container into a new incomplete target (010).
    Transform {
        #[arg(long = "from")]
        from: String,
        #[arg(long = "rewrite")]
        rewrite: String,
        #[arg(long = "to-type")]
        to_type: Option<String>,
        #[arg(long = "mapping-file")]
        mapping_file: Option<PathBuf>,
        #[arg(long = "swap", default_value_t = true)]
        swap: bool,
        #[arg(long = "no-swap")]
        no_swap: bool,
        #[arg(long = "retain-source", default_value_t = false)]
        retain_source: bool,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Snapshot backup job (010 → 013).
    Backup {
        #[arg(long = "namespace")]
        namespace: Option<String>,
        #[arg(long = "container")]
        container: Option<String>,
        #[arg(long)]
        pitr: bool,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Restore from snapshot (010 → 013).
    Restore {
        #[arg(long = "snapshot")]
        snapshot: String,
        #[arg(long = "pitr")]
        pitr: Option<String>,
        #[arg(long = "namespace")]
        namespace: Option<String>,
        #[arg(long = "key-ref")]
        key_ref: Option<String>,
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Query-processing counters (008 FR-006 producer side).
    QueryStats {
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Print admin-http UI paths (009).
    Ui {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// Kafka / syslog ingest control (009).
    Ingest {
        #[command(subcommand)]
        action: IngestAction,
    },
    /// L0 creatable workflow (full v1).
    L0 {
        #[command(subcommand)]
        action: L0Action,
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// Legal-hold / GDPR erase (full v1).
    Legal {
        #[command(subcommand)]
        action: LegalAction,
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// Named CDC streams (full v1).
    Cdc {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// Planetary compositions (full v1).
    Composition {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// Billing estimate (no invoicing).
    Billing {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
    /// External KMS / master provider status.
    Kms {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        endpoint: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum IngestAction {
    Kafka {
        #[command(subcommand)]
        action: IngestKafkaAction,
    },
    /// Prints that syslog bind is node entrypoint config (does not bind a port).
    Syslog,
}

#[derive(Subcommand, Debug)]
pub enum IngestKafkaAction {
    List,
    Add {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        container: String,
        #[arg(long)]
        brokers: String,
        #[arg(long)]
        topic: String,
        #[arg(long)]
        group: String,
        #[arg(long, default_value = "raw")]
        format: String,
        #[arg(long)]
        name: Option<String>,
    },
    Delete {
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum JobsAction {
    Wait { id: String },
    Cancel { id: String },
}

#[derive(Subcommand, Debug)]
pub enum JobAction {
    List,
    Status { id: String },
    Cancel { id: String },
    Resume { id: String },
}

#[derive(Subcommand, Debug)]
pub enum L0Action {
    Create {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        name: String,
        #[arg(long = "type")]
        type_name: String,
    },
    Get {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        name: String,
    },
    List,
}

#[derive(Subcommand, Debug)]
pub enum LegalAction {
    Hold {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        container: String,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        key: Option<String>,
    },
    Erase {
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        container: String,
        #[arg(long)]
        key: String,
    },
}
