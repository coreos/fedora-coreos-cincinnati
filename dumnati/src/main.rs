extern crate actix;
extern crate actix_web;
extern crate env_logger;
extern crate failure;
extern crate futures;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;
extern crate serde;
extern crate serde_derive;
extern crate serde_json;
extern crate structopt;
#[macro_use]
extern crate prometheus;

mod graph;
mod metadata;
mod metrics;
mod policy;
mod scraper;

use actix::prelude::*;
use actix_web::{http::Method, middleware::Logger, server, App};
use actix_web::{HttpRequest, HttpResponse};
use failure::{Error, Fallible};
use futures::prelude::*;
use prometheus::{Histogram, IntCounter};
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
}

fn main() -> Fallible<()> {
    env_logger::Builder::from_default_env().try_init()?;

    let opts = CliOptions::from_args();
    trace!("started with CLI options: {:#?}", opts);

    let sys = actix::System::new("dumnati");

    let scraper_addr = scraper::Scraper::new("testing")?.start();

    let node_population = Arc::new(cbloom::Filter::new(10 * 1024 * 1024, 1_000_000));
    let service_state = AppState {
        scraper_addr,
        population: Arc::clone(&node_population),
    };
    let gb_service = service_state.clone();
    let gb_status = service_state.clone();
    let pe_service = service_state.clone();
    let pe_status = service_state.clone();

    // Graph-builder service.
    server::new(move || {
        App::with_state(gb_service.clone())
            .middleware(Logger::default())
            .route("/v1/graph", Method::GET, gb_serve_graph)
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 8080))?
    .start();

    // Graph-builder status service.
    server::new(move || {
        App::with_state(gb_status.clone())
            .middleware(Logger::default())
            .route("/metrics", Method::GET, metrics::serve_metrics)
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 9080))?
    .start();

    // Policy-engine service.
    server::new(move || {
        App::with_state(pe_service.clone())
            .middleware(Logger::default())
            .route("/v1/graph", Method::GET, pe_serve_graph)
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 8081))?
    .start();

    // Policy-engine status service.
    server::new(move || {
        App::with_state(pe_status.clone())
            .middleware(Logger::default())
            .route("/metrics", Method::GET, metrics::serve_metrics)
    })
    .bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), 9081))?
    .start();

    sys.run();
    Ok(())
}

#[derive(Clone, Debug)]
pub(crate) struct AppState {
    scraper_addr: Addr<scraper::Scraper>,
    population: Arc<cbloom::Filter>,
}

pub(crate) fn gb_serve_graph(
    req: HttpRequest<AppState>,
) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    let basearch = req
        .query()
        .get("basearch")
        .map(String::from)
        .unwrap_or_default();
    let stream = req
        .query()
        .get("stream")
        .map(String::from)
        .unwrap_or_default();

    let cached_graph = req
        .state()
        .scraper_addr
        .send(scraper::GetCachedGraph { stream })
        .flatten();

    let resp = cached_graph
        .and_then(|graph| policy::pick_basearch(graph, basearch))
        .map(|graph| policy::filter_deadends(graph))
        .and_then(|graph| {
            serde_json::to_string_pretty(&graph).map_err(|e| failure::format_err!("{}", e))
        })
        .map(|json| {
            HttpResponse::Ok()
                .content_type("application/json")
                .body(json)
        });

    Box::new(resp)
}

pub(crate) fn pe_serve_graph(
    req: HttpRequest<AppState>,
) -> Box<dyn Future<Item = HttpResponse, Error = Error>> {
    pe_record_metrics(&req);

    let basearch = req
        .query()
        .get("basearch")
        .map(String::from)
        .unwrap_or_default();
    let stream = req
        .query()
        .get("stream")
        .map(String::from)
        .unwrap_or_default();

    let wariness = compute_wariness(&req.query());
    ROLLOUT_WARINESS.observe(wariness);

    let cached_graph = req
        .state()
        .scraper_addr
        .send(scraper::GetCachedGraph { stream })
        .flatten();

    let resp = cached_graph
        .and_then(|graph| policy::pick_basearch(graph, basearch))
        .map(move |graph| policy::throttle_rollouts(graph, wariness))
        .map(|graph| policy::filter_deadends(graph))
        .and_then(|graph| {
            serde_json::to_string_pretty(&graph).map_err(|e| failure::format_err!("{}", e))
        })
        .map(|json| {
            HttpResponse::Ok()
                .content_type("application/json")
                .body(json)
        });

    Box::new(resp)
}

fn compute_wariness(params: &HashMap<String, String>) -> f64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    if let Ok(input) = params
        .get("rollout_wariness")
        .map(String::from)
        .unwrap_or_default()
        .parse::<f64>()
    {
        let wariness = input.max(0.0).min(1.0);
        return wariness;
    }

    let uuid = params
        .get("node_uuid")
        .map(String::from)
        .unwrap_or_default();
    let wariness = {
        // Left limit not included in range.
        const COMPUTED_MIN: f64 = 0.0 + 0.000001;
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

pub(crate) fn pe_record_metrics(req: &HttpRequest<AppState>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    V1_GRAPH_INCOMING_REQS.inc();

    let population = &req.state().population;
    if let Some(uuid) = req.query().get("node_uuid") {
        let mut hasher = DefaultHasher::default();
        uuid.hash(&mut hasher);
        let client_uuid = hasher.finish();
        if !population.maybe_contains(client_uuid) {
            population.insert(client_uuid);
            UNIQUE_IDS.inc();
        }
    }
}

#[derive(Debug, StructOpt)]
pub(crate) struct CliOptions {
    /// Path to configuration file.
    #[structopt(short = "c")]
    pub config_path: Option<String>,
}
