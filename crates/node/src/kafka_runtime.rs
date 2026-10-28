//! Ready-node Kafka consumer supervisor (009 T053 / T057–T058 / T061).

use crate::ingest_append;
use crate::Node;
use spacestorage_ingest::{
    KafkaConsumer, KafkaIngest, KafkaTls, KafkaWireError, LiveKafkaClient, APPLICATION_INGEST,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

struct Running {
    cancel: tokio_util::sync::CancellationToken,
}

pub struct KafkaSupervisor {
    node: Arc<Node>,
    running: Mutex<HashMap<Uuid, Running>>,
}

impl KafkaSupervisor {
    pub fn new(node: Arc<Node>) -> Arc<Self> {
        Arc::new(Self {
            node,
            running: Mutex::new(HashMap::new()),
        })
    }

    pub async fn reconcile(self: &Arc<Self>, decls: &[KafkaIngest]) {
        let want: HashMap<Uuid, KafkaIngest> = decls.iter().map(|d| (d.id, d.clone())).collect();
        {
            let mut run = self.running.lock().await;
            let stop: Vec<Uuid> = run.keys().copied().filter(|id| !want.contains_key(id)).collect();
            for id in stop {
                if let Some(r) = run.remove(&id) {
                    r.cancel.cancel();
                    info!(%id, application=%APPLICATION_INGEST, "kafka ingest consumer stopped");
                }
            }
        }
        for (id, decl) in want {
            let mut run = self.running.lock().await;
            if run.contains_key(&id) {
                continue;
            }
            let cancel = tokio_util::sync::CancellationToken::new();
            run.insert(id, Running { cancel: cancel.clone() });
            drop(run);
            let node = Arc::clone(&self.node);
            let this = Arc::clone(self);
            let decl_c = decl.clone();
            self.node.tasks.spawn(async move {
                run_consumer(node, decl_c, cancel).await;
                this.running.lock().await.remove(&id);
            });
        }
    }
}

async fn run_consumer(
    node: Arc<Node>,
    decl: KafkaIngest,
    cancel: tokio_util::sync::CancellationToken,
) {
    info!(
        id=%decl.id,
        name=%decl.name,
        topic=%decl.topic,
        plaintext=%decl.plaintext,
        application=%APPLICATION_INGEST,
        "kafka ingest consumer starting"
    );
    let append = ingest_append::append_fn(
        Arc::clone(&node.catalog),
        decl.namespace.clone(),
        decl.container.clone(),
    );
    let live = match LiveKafkaClient::from_decl(
        decl.brokers.clone(),
        decl.topic.clone(),
        decl.group.clone(),
        decl.plaintext,
        decl.tls.clone(),
    ) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            warn!(error=%e, name=%decl.name, "kafka transport config rejected");
            cancel.cancelled().await;
            return;
        }
    };
    if let Err(e) = live.bootstrap().await {
        warn!(error=%e, name=%decl.name, "kafka live bootstrap failed; consumer idle until restart");
        cancel.cancelled().await;
        return;
    }
    let decode_gate = node.buffers.gate("ingest.kafka.decode");
    let consumer = KafkaConsumer::with_live(decl.clone(), Arc::clone(&live), append)
        .with_buffer_gate(decode_gate)
        .with_runtime(Arc::clone(&node.ingest));

    // Heartbeat well before session timeout so the member stays in the group.
    let hb_every = live.session_timeout() / 3;
    let mut hb = tokio::time::interval(hb_every);
    hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = node.cancel.cancelled() => break,
            _ = hb.tick() => {
                match live.heartbeat().await {
                    Ok(()) => {}
                    Err(KafkaWireError::RejoinRequired) => {
                        if let Err(e) = live.rejoin().await {
                            warn!(error=%e, name=%decl.name, "kafka rejoin after heartbeat failed");
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                    Err(e) => {
                        warn!(error=%e, name=%decl.name, "kafka heartbeat error");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
            r = consumer.poll_one_async() => {
                match r {
                    Ok(true) => debug!(name=%decl.name, "kafka ingest committed"),
                    Ok(false) => {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                    Err(e) => {
                        warn!(error=%e, name=%decl.name, "kafka poll error");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}

/// Install cluster-log path, load, bootstrap from config, and wire reconcile hook.
pub async fn bootstrap_kafka(node: &Arc<Node>) -> Arc<KafkaSupervisor> {
    let supervisor = KafkaSupervisor::new(Arc::clone(node));
    let data_dir = node
        .config
        .load()
        .storage_data_dir
        .clone()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            node.config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .join("data")
        });
    let path = spacestorage_ingest::KafkaIngestStore::cluster_log_default_path(&data_dir);
    node.ingest.kafka.set_cluster_log_path(path);
    if let Err(e) = node.ingest.kafka.load_from_cluster_log() {
        warn!(error=%e, "kafka cluster log load failed");
    }
    // Bootstrap decls from node config.
    let boot: Vec<KafkaIngest> = node
        .config
        .load()
        .kafka_ingests
        .iter()
        .map(|d| KafkaIngest {
            id: uuid::Uuid::now_v7(),
            name: d.name.clone(),
            namespace: d.namespace.clone(),
            container: d.container.clone(),
            type_name: d.type_name.clone(),
            brokers: d.brokers.clone(),
            topic: d.topic.clone(),
            group: d.group.clone(),
            format: match d.format.as_str() {
                "json" => spacestorage_ingest::PayloadFormat::Json,
                _ => spacestorage_ingest::PayloadFormat::Raw,
            },
            plaintext: d.plaintext,
            tls: d.tls.as_ref().map(|t| KafkaTls {
                certificate: t.certificate.clone(),
                key: t.key.clone(),
            }),
            created_by: None,
            state: "running".into(),
        })
        .collect();
    node.ingest.kafka.seed_bootstrap(&boot);

    let sup = Arc::clone(&supervisor);
    node.ingest.kafka.set_on_change(Arc::new(move |decls| {
        let sup = Arc::clone(&sup);
        let decls = decls.to_vec();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                sup.reconcile(&decls).await;
            });
        }
    }));

    if let Ok(list) = node.ingest.kafka.list() {
        supervisor.reconcile(&list).await;
    }
    supervisor
}
