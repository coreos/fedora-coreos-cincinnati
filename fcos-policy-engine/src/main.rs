#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;

mod cli;
mod config;
mod settings;
mod utils;

use actix_web::{web, App, HttpResponse};
use clap::{crate_name, crate_version, Parser};
use commons::{graph, metrics, policy};
use failure::{Error, Fallible, ResultExt};
use prometheus::{Histogram, IntCounter, IntGauge};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

/// Top-level log target for this application.
static APP_LOG_TARGET: &str = "fcos_policy_engine";

lazy_static::lazy_static! {
    static ref V1_GRAPH_INCOMING_REQS: IntCounter = register_int_counter!(opts!(
        "fcos_cincinnati_pe_v1_graph_incoming_requests_total",
        "Total number of incoming HTTP client request to /v1/graph"
    ))
    .unwrap();
    static ref UNIQUE_IDS: IntCounter = register_int_counter!(opts!(
        "fcos_cincinnati_pe_v1_graph_unique_uuids_total",
        "Total number of unique node UUIDs (per-instance Bloom filter)."
    ))
    .unwrap();
    static ref ROLLOUT_WARINESS: Histogram = register_histogram!(
        "fcos_cincinnati_pe_v1_graph_rollout_wariness",
        "Per-request rollout wariness.",
        prometheus::linear_buckets(0.0, 0.1, 11).unwrap()
    )
    .unwrap();
    // NOTE(lucab): alternatively this could come from the runtime library, see
    // https://prometheus.io/docs/instrumenting/writing_clientlibs/#process-metrics
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();
}

fn main() -> Fallible<()> {
    // Parse command-line options.
    let cli_opts = cli::CliOptions::parse();

    // Setup logging.
    env_logger::Builder::from_default_env()
        .format_timestamp_secs()
        .format_module_path(false)
        .filter(Some(APP_LOG_TARGET), cli_opts.loglevel())
        .try_init()
        .context("failed to initialize logging")?;

    // Parse config file and validate settings.
    let (service_settings, status_settings) = {
        debug!("config file location: {}", cli_opts.config_path.display());
        let cfg = config::FileConfig::parse_file(cli_opts.config_path)?;
        let settings = settings::PolicyEngineSettings::validate_config(cfg)?;
        (settings.service, settings.status)
    };

    let sys = actix::System::new("fcos_cincinnati_pe");

    let node_population = Arc::new(cbloom::Filter::new(
        service_settings.bloom_size,
        service_settings.bloom_max_population,
    ));
    let service_state = AppState {
        // TODO(lucab): get allowed scopes from config file.
        scope_filter: None,
        population: Arc::clone(&node_population),
        upstream_endpoint: service_settings.upstream_base.clone(),
        upstream_req_timeout: service_settings.upstream_req_timeout,
    };
    debug!(
        "upstream graph endpoint: {}",
        service_settings.upstream_base
    );

    let start_timestamp = chrono::Utc::now();
    PROCESS_START_TIME.set(start_timestamp.timestamp());
    info!("starting server ({} {})", crate_name!(), crate_version!());

    // Policy-engine main service.
    let service_socket = service_settings.socket_addr();
    debug!("main service address: {}", service_socket);
    actix_web::HttpServer::new(move || {
        App::new()
            .wrap(commons::web::build_cors_middleware(
                &service_settings.origin_allowlist,
            ))
            .data(service_state.clone())
            .route("/v1/graph", web::get().to(pe_serve_graph))
    })
    .bind(service_socket)?
    .run();

    // Policy-engine status service.
    let status_socket = status_settings.socket_addr();
    debug!("status service address: {}", status_socket);
    actix_web::HttpServer::new(move || {
        App::new().route("/metrics", web::get().to(metrics::serve_metrics))
    })
    .bind(status_socket)?
    .run();

    sys.run()?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    scope_filter: Option<HashSet<graph::GraphScope>>,
    population: Arc<cbloom::Filter>,
    upstream_endpoint: reqwest::Url,
    upstream_req_timeout: Duration,
}

/// Mandatory parameters for querying a graph from policy-engine.
#[derive(Serialize, Deserialize)]
pub struct GraphQuery {
    basearch: Option<String>,
    stream: Option<String>,
    rollout_wariness: Option<String>,
    node_uuid: Option<String>,
    oci: Option<bool>,
}

pub(crate) async fn pe_serve_graph(
    data: web::Data<AppState>,
    web::Query(query): web::Query<GraphQuery>,
) -> Result<HttpResponse, Error> {
    pe_record_metrics(&data, &query);

    let scope = match commons::web::validate_scope(
        query.basearch.clone(),
        query.stream.clone(),
        query.oci,
        &data.scope_filter,
    ) {
        Err(e) => {
            log::error!("graph request with invalid scope: {}", e);
            return Ok(HttpResponse::BadRequest().finish());
        }
        Ok(s) => {
            log::trace!("graph query stream: {:#?}", s);
            s
        }
    };

    let wariness = compute_wariness(&query);
    ROLLOUT_WARINESS.observe(wariness);

    let cached_graph = utils::fetch_graph_from_gb(
        data.upstream_endpoint.clone(),
        scope.stream,
        scope.basearch,
        scope.oci,
        data.upstream_req_timeout,
    )
    .await?;

    let throttled_graph = policy::throttle_rollouts(cached_graph, wariness);
    let final_graph = policy::filter_deadends(throttled_graph);

    let json =
        serde_json::to_string_pretty(&final_graph).map_err(|e| failure::format_err!("{}", e))?;
    let resp = HttpResponse::Ok()
        .content_type("application/json")
        .body(json);
    Ok(resp)
}

#[allow(clippy::let_and_return)]
fn compute_wariness(params: &GraphQuery) -> f64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    if let Ok(input) = params
        .rollout_wariness
        .as_ref()
        .map(String::from)
        .unwrap_or_default()
        .parse::<f64>()
    {
        let wariness = input.max(0.0).min(1.0);
        return wariness;
    }

    let uuid = params
        .node_uuid
        .as_ref()
        .map(String::from)
        .unwrap_or_default();
    let wariness = {
        // Left limit not included in range.
        const COMPUTED_MIN: f64 = 0.0 + 0.000_001;
        const COMPUTED_MAX: f64 = 1.0;
        let mut hasher = DefaultHasher::new();
        uuid.hash(&mut hasher);
        let digest = hasher.finish();
        // Scale down.
        let scaled = (digest as f64) / (std::u64::MAX as f64);
        // Clamp within limits.
        scaled.max(COMPUTED_MIN).min(COMPUTED_MAX)
    };

    wariness
}

pub(crate) fn pe_record_metrics(data: &AppState, query: &GraphQuery) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    V1_GRAPH_INCOMING_REQS.inc();

    if let Some(uuid) = &query.node_uuid {
        let mut hasher = DefaultHasher::default();
        uuid.hash(&mut hasher);
        let client_uuid = hasher.finish();
        if !data.population.maybe_contains(client_uuid) {
            data.population.insert(client_uuid);
            UNIQUE_IDS.inc();
        }
    }
}
