#[macro_use]
extern crate log;
#[macro_use]
extern crate prometheus;

mod graph;
mod metadata;
mod metrics;
mod policy;
mod scraper;

use actix::prelude::*;
use actix_cors::CorsFactory;
use actix_web::{web, App, HttpResponse};
use failure::{Error, Fallible};
use prometheus::{Histogram, IntCounter, IntCounterVec, IntGauge, IntGaugeVec};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use structopt::StructOpt;

lazy_static::lazy_static! {
    static ref V1_GRAPH_INCOMING_REQS: IntCounter = register_int_counter!(opts!(
        "dumnati_pe_v1_graph_incoming_requests_total",
        "Total number of incoming HTTP client request to /v1/graph"
    ))
    .unwrap();
    static ref UNIQUE_IDS: IntCounter = register_int_counter!(opts!(
        "dumnati_pe_v1_graph_unique_uuids_total",
        "Total number of unique node UUIDs (per-instance Bloom filter)."
    ))
    .unwrap();
    static ref ROLLOUT_WARINESS: Histogram = register_histogram!(
        "dumnati_pe_v1_graph_rollout_wariness",
        "Per-request rollout wariness.",
        prometheus::linear_buckets(0.0, 0.1, 11).unwrap()
    )
    .unwrap();
    static ref GRAPH_FINAL_EDGES: IntGaugeVec = register_int_gauge_vec!(
        "dumnati_gb_scraper_graph_final_edges",
        "Number of edges in the cached graph, after processing",
        &["stream"]
    ).unwrap();
    static ref GRAPH_FINAL_RELEASES: IntGaugeVec = register_int_gauge_vec!(
        "dumnati_gb_scraper_graph_final_releases",
        "Number of releases in the cached graph, after processing",
        &["stream"]
    ).unwrap();
    static ref LAST_REFRESH: IntGaugeVec = register_int_gauge_vec!(
       "dumnati_gb_scraper_graph_last_refresh_timestamp",
        "UTC timestamp of last graph refresh",
        &["stream"]
    ).unwrap();
    static ref UPSTREAM_SCRAPES: IntCounterVec = register_int_counter_vec!(
       "dumnati_gb_scraper_upstream_scrapes_total",
       "Total number of upstream scrapes",
        &["stream"]
    ).unwrap();
    // NOTE(lucab): alternatively this could come from the runtime library, see
    // https://prometheus.io/docs/instrumenting/writing_clientlibs/#process-metrics
    static ref PROCESS_START_TIME: IntGauge = register_int_gauge!(opts!(
        "process_start_time_seconds",
        "Start time of the process since unix epoch in seconds."
    )).unwrap();

}

fn main() -> Fallible<()> {
    env_logger::Builder::from_default_env().try_init()?;

    let opts = CliOptions::from_args();
    trace!("started with CLI options: {:#?}", opts);

    let sys = actix::System::new("dumnati");

    // TODO(lucab): figure out all configuration params.
    let gb_allowed_origins = vec!["https://builds.coreos.fedoraproject.org"];
    let pe_allowed_origins = vec!["https://builds.coreos.fedoraproject.org"];
    let streams_cfg = maplit::btreeset!["next", "stable", "testing"];
    let mut scrapers = HashMap::with_capacity(streams_cfg.len());
    for stream in streams_cfg {
        let addr = scraper::Scraper::new(stream)?.start();
        scrapers.insert(stream.to_string(), addr);
    }

    let node_population = Arc::new(cbloom::Filter::new(10 * 1024 * 1024, 1_000_000));
    let service_state = AppState {
        scrapers,
        population: Arc::clone(&node_population),
    };

    let start_timestamp = chrono::Utc::now();
    PROCESS_START_TIME.set(start_timestamp.timestamp());

    // Graph-builder service.
    let gb_service = service_state.clone();
    actix_web::HttpServer::new(move || {
        App::new()
            .wrap(build_cors_middleware(&gb_allowed_origins))
            .data(gb_service.clone())
            .route("/v1/graph", web::get().to(gb_serve_graph))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 8080))?
    .run();

    // Graph-builder status service.
    let gb_status = service_state.clone();
    actix_web::HttpServer::new(move || {
        App::new()
            .data(gb_status.clone())
            .route("/metrics", web::get().to(metrics::serve_metrics))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 9080))?
    .run();

    // Policy-engine service.
    let pe_service = service_state.clone();
    actix_web::HttpServer::new(move || {
        App::new()
            .wrap(build_cors_middleware(&pe_allowed_origins))
            .data(pe_service.clone())
            .route("/v1/graph", web::get().to(pe_serve_graph))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 8081))?
    .run();

    // Policy-engine status service.
    let pe_status = service_state;
    actix_web::HttpServer::new(move || {
        App::new()
            .data(pe_status.clone())
            .route("/metrics", web::get().to(metrics::serve_metrics))
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 9081))?
    .run();

    sys.run()?;
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    scrapers: HashMap<String, Addr<scraper::Scraper>>,
    population: Arc<cbloom::Filter>,
}

#[derive(Deserialize)]
pub struct GraphQuery {
    basearch: Option<String>,
    stream: Option<String>,
    rollout_wariness: Option<String>,
    node_uuid: Option<String>,
}

pub(crate) async fn gb_serve_graph(
    data: actix_web::web::Data<AppState>,
    query: actix_web::web::Query<GraphQuery>,
) -> Result<HttpResponse, failure::Error> {
    let basearch = query
        .basearch
        .as_ref()
        .map(String::from)
        .unwrap_or_default();
    let stream = query.stream.as_ref().map(String::from).unwrap_or_default();

    let addr = match data.scrapers.get(&stream) {
        None => return Ok(HttpResponse::NotFound().finish()),
        Some(addr) => addr,
    };

    let cached_graph = addr.send(scraper::GetCachedGraph { stream }).await??;

    let arch_graph = policy::pick_basearch(cached_graph, basearch)?;
    let final_graph = policy::filter_deadends(arch_graph);

    let json =
        serde_json::to_string_pretty(&final_graph).map_err(|e| failure::format_err!("{}", e))?;
    let resp = HttpResponse::Ok()
        .content_type("application/json")
        .body(json);
    Ok(resp)
}

pub(crate) async fn pe_serve_graph(
    data: actix_web::web::Data<AppState>,
    actix_web::web::Query(query): actix_web::web::Query<GraphQuery>,
) -> Result<HttpResponse, Error> {
    pe_record_metrics(&data, &query);

    let basearch = query
        .basearch
        .as_ref()
        .map(String::from)
        .unwrap_or_default();
    let stream = query.stream.as_ref().map(String::from).unwrap_or_default();

    let addr = match data.scrapers.get(&stream) {
        None => return Ok(HttpResponse::NotFound().finish()),
        Some(addr) => addr,
    };

    let wariness = compute_wariness(&query);
    ROLLOUT_WARINESS.observe(wariness);

    let cached_graph = addr.send(scraper::GetCachedGraph { stream }).await??;

    let arch_graph = policy::pick_basearch(cached_graph, basearch)?;
    let throttled_graph = policy::throttle_rollouts(arch_graph, wariness);
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

/// Provide a CORS middleware allowing given origins.
pub(crate) fn build_cors_middleware(allowed_origins: &[&str]) -> CorsFactory {
    let mut builder = actix_cors::Cors::new();
    for origin in allowed_origins {
        builder = builder.allowed_origin(origin);
    }
    builder.finish()
}

#[derive(Debug, StructOpt)]
pub(crate) struct CliOptions {
    /// Path to configuration file.
    #[structopt(short = "c")]
    pub config_path: Option<String>,
}
