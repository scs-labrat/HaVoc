//! Veilid node lifecycle management.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::info;
use veilid_core::{
    CryptoSystem, VeilidAPI, VeilidConfig, VeilidUpdate, CRYPTO_KIND_VLD0,
};

use crate::sync::SyncEvent;
use crate::VeilidError;

pub struct HvocNode {
    pub api: VeilidAPI,
    pub update_tx: broadcast::Sender<VeilidUpdate>,
    pub sync_tx: broadcast::Sender<SyncEvent>,
}

impl HvocNode {
    pub async fn start(data_dir: PathBuf) -> Result<Arc<Self>, VeilidError> {
        let (update_tx, _) = broadcast::channel(512);
        let (sync_tx, _) = broadcast::channel(512);

        let update_tx_cb = update_tx.clone();
        let sync_tx_cb = sync_tx.clone();

        let config = Self::build_config(&data_dir);

        // Sync callback — no async, just channel sends.
        let callback = Arc::new(move |update: VeilidUpdate| {
            let _ = update_tx_cb.send(update.clone());

            match &update {
                VeilidUpdate::ValueChange(vc) => {
                    let event = SyncEvent::DhtValueChanged {
                        record_key: format!("{}", vc.key),
                        subkeys: vc.subkeys.iter().collect(),
                    };
                    let _ = sync_tx_cb.send(event);
                }
                VeilidUpdate::AppMessage(msg) => {
                    let event = SyncEvent::AppMessageReceived {
                        sender: msg.sender().map(|s| s.to_string()),
                        payload: msg.message().to_vec(),
                    };
                    let _ = sync_tx_cb.send(event);
                }
                VeilidUpdate::Attachment(att) => {
                    let event = SyncEvent::AttachmentChanged {
                        state: format!("{:?}", att.state),
                        public_internet_ready: att.public_internet_ready,
                    };
                    let _ = sync_tx_cb.send(event);
                }
                VeilidUpdate::RouteChange(rc) => {
                    if !rc.dead_routes.is_empty() || !rc.dead_remote_routes.is_empty() {
                        let mut dead = Vec::new();
                        for r in &rc.dead_routes {
                            dead.push(r.to_string());
                        }
                        for r in &rc.dead_remote_routes {
                            dead.push(r.to_string());
                        }
                        let _ = sync_tx_cb.send(SyncEvent::RouteDied { dead_routes: dead });
                    }
                }
                _ => {}
            }
        });

        let api = veilid_core::api_startup(callback, config)
            .await
            .map_err(|e| VeilidError::Core(e.to_string()))?;

        api.attach().await?;
        info!("Veilid node attached");

        Ok(Arc::new(HvocNode {
            api,
            update_tx,
            sync_tx,
        }))
    }

    pub fn subscribe_updates(&self) -> broadcast::Receiver<VeilidUpdate> {
        self.update_tx.subscribe()
    }

    pub fn subscribe_sync(&self) -> broadcast::Receiver<SyncEvent> {
        self.sync_tx.subscribe()
    }

    pub fn routing_context(&self) -> Result<veilid_core::RoutingContext, VeilidError> {
        Ok(self.api.routing_context()?)
    }

    /// Execute a closure with access to the VLD0 crypto system.
    pub fn with_crypto<F, R>(&self, f: F) -> Result<R, VeilidError>
    where
        F: FnOnce(&(dyn CryptoSystem + Send + Sync)) -> Result<R, VeilidError>,
    {
        let crypto = self.api.crypto()?;
        let cs = crypto
            .get(CRYPTO_KIND_VLD0)
            .ok_or_else(|| VeilidError::Crypto("VLD0 crypto system not available".into()))?;
        f(&*cs)
    }

    pub async fn shutdown(self: Arc<Self>) -> Result<(), VeilidError> {
        self.api.detach().await?;
        self.api.clone().shutdown().await;
        info!("Veilid node shut down");
        Ok(())
    }

    fn build_config(data_dir: &PathBuf) -> VeilidConfig {
        let storage_dir = data_dir.to_string_lossy().to_string();
        let mut cfg = VeilidConfig::new("hvoc", "", "", Some(&storage_dir), None);
        cfg.protected_store.allow_insecure_fallback = true;
        cfg.protected_store.always_use_insecure_storage = true;
        cfg
    }
}
