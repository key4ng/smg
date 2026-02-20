use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};
use openai_protocol::messages::ListModelsResponse;

use crate::client::{
    ClusterStatusResponse, LoadsResponse, MeshHealthResponse, RateLimitStats, SmgClient,
    WorkersResponse,
};

/// Cached gateway state from the most recent poll cycle.
#[derive(Debug, Default)]
pub struct GatewayState {
    pub connected: bool,
    pub healthy: bool,
    pub last_updated: Option<DateTime<Utc>>,
    pub last_error: Option<String>,

    pub workers: Option<WorkersResponse>,
    pub loads: Option<LoadsResponse>,
    pub cluster: Option<ClusterStatusResponse>,
    pub mesh_health: Option<MeshHealthResponse>,
    pub rate_limits: Option<RateLimitStats>,
    pub models: Option<ListModelsResponse>,
}

/// Thread-safe shared handle to the gateway state.
pub type SharedState = Arc<RwLock<GatewayState>>;

/// Spawn a background poller that periodically fetches data from all SMG
/// endpoints and updates [`SharedState`].
pub fn spawn_poller(client: SmgClient, state: SharedState, interval_secs: u64) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            poll_once(&client, &state).await;
        }
    });
}

async fn poll_once(client: &SmgClient, state: &SharedState) {
    // Fire all requests concurrently.
    let (health, workers, loads, cluster, mesh, rates, models) = tokio::join!(
        client.check_health(),
        client.list_workers(),
        client.get_loads(),
        client.get_cluster_status(),
        client.get_mesh_health(),
        client.get_rate_limit_stats(),
        client.list_models(),
    );

    let mut s = state.write().unwrap();

    // Connection / readiness
    match health {
        Ok(()) => {
            s.connected = true;
            s.healthy = true;
            s.last_error = None;
        }
        Err(e) => {
            s.connected = false;
            s.healthy = false;
            s.last_error = Some(e.to_string());
        }
    }

    // Each endpoint: update on success, keep stale data on failure.
    if let Ok(w) = workers {
        s.workers = Some(w);
    }
    if let Ok(l) = loads {
        s.loads = Some(l);
    }
    if let Ok(c) = cluster {
        s.cluster = Some(c);
    }
    if let Ok(m) = mesh {
        s.mesh_health = Some(m);
    }
    if let Ok(r) = rates {
        s.rate_limits = Some(r);
    }
    if let Ok(m) = models {
        s.models = Some(m);
    }

    s.last_updated = Some(Utc::now());
}
