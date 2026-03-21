use std::collections::{HashMap, VecDeque};
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

    /// Rolling aggregate throughput history (capacity: 20).
    pub throughput_history: VecDeque<f64>,
    /// Rolling aggregate cache hit rate history (capacity: 20).
    pub cache_hit_history: VecDeque<f64>,
    /// Per-worker throughput history.
    pub per_worker_throughput: HashMap<String, VecDeque<f64>>,
    /// Per-worker cache hit rate history.
    pub per_worker_cache_hit: HashMap<String, VecDeque<f64>>,

    /// Previous total request count from Prometheus (for computing req/s).
    pub prev_request_count: Option<u64>,
    /// Rolling requests-per-second history (for external workers without gen_throughput).
    pub requests_per_sec_history: VecDeque<f64>,
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
            poll_once(&client, &state, interval_secs).await;
        }
    });
}

/// Parse total request count from Prometheus metrics text.
fn parse_request_count(metrics_text: &str) -> u64 {
    metrics_text
        .lines()
        .filter(|line| line.starts_with("smg_router_requests_total{"))
        .filter_map(|line| {
            line.rsplit_once(' ')
                .and_then(|(_, v)| v.parse::<u64>().ok())
        })
        .sum()
}

async fn poll_once(client: &SmgClient, state: &SharedState, interval_secs: u64) {
    // Fire all requests concurrently.
    let (health, workers, loads, cluster, mesh, rates, models, metrics) = tokio::join!(
        client.check_health(),
        client.list_workers(),
        client.get_loads(),
        client.get_cluster_status(),
        client.get_mesh_health(),
        client.get_rate_limit_stats(),
        client.list_models(),
        client.fetch_metrics(),
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

    const SPARKLINE_CAP: usize = 20;

    // Extract per-worker metrics before mutably borrowing sparkline buffers.
    let worker_metrics: Vec<(String, f64, f64)> = if let Some(ref loads_resp) = s.loads {
        loads_resp
            .workers
            .iter()
            .filter_map(|wl| {
                wl.details.as_ref().map(|details| {
                    let throughput: f64 = details.loads.iter().map(|s| s.gen_throughput).sum();
                    let cache_hit: f64 = if details.loads.is_empty() {
                        0.0
                    } else {
                        details.loads.iter().map(|s| s.cache_hit_rate).sum::<f64>()
                            / details.loads.len() as f64
                    };
                    (wl.worker.clone(), throughput, cache_hit)
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    let has_worker_metrics = !worker_metrics.is_empty();
    if has_worker_metrics {
        let mut total_throughput = 0.0_f64;
        let mut total_cache_hits = 0.0_f64;
        let worker_count = worker_metrics.len() as u32;

        for (worker_name, throughput, cache_hit) in worker_metrics {
            total_throughput += throughput;
            total_cache_hits += cache_hit;

            let th = s.per_worker_throughput
                .entry(worker_name.clone())
                .or_insert_with(|| VecDeque::with_capacity(SPARKLINE_CAP));
            if th.len() >= SPARKLINE_CAP { th.pop_front(); }
            th.push_back(throughput);

            let ch = s.per_worker_cache_hit
                .entry(worker_name)
                .or_insert_with(|| VecDeque::with_capacity(SPARKLINE_CAP));
            if ch.len() >= SPARKLINE_CAP { ch.pop_front(); }
            ch.push_back(cache_hit);
        }

        if s.throughput_history.len() >= SPARKLINE_CAP { s.throughput_history.pop_front(); }
        s.throughput_history.push_back(total_throughput);

        let avg_cache = if worker_count > 0 { total_cache_hits / worker_count as f64 } else { 0.0 };
        if s.cache_hit_history.len() >= SPARKLINE_CAP { s.cache_hit_history.pop_front(); }
        s.cache_hit_history.push_back(avg_cache);
    }

    // Compute requests/sec from Prometheus counter (works for external workers).
    if let Ok(metrics_text) = metrics {
        let current_count = parse_request_count(&metrics_text);
        if let Some(prev) = s.prev_request_count {
            let delta = current_count.saturating_sub(prev);
            let rps = delta as f64 / interval_secs as f64;
            if s.requests_per_sec_history.len() >= SPARKLINE_CAP {
                s.requests_per_sec_history.pop_front();
            }
            s.requests_per_sec_history.push_back(rps);

            // If no gen_throughput data, use req/s as throughput fallback.
            if !has_worker_metrics {
                if s.throughput_history.len() >= SPARKLINE_CAP {
                    s.throughput_history.pop_front();
                }
                s.throughput_history.push_back(rps);
            }
        }
        s.prev_request_count = Some(current_count);
    }
}
