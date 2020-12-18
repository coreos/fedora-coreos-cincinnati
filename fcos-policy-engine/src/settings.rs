use super::config::FileConfig;
use failure::Fallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

/// Runtime settings for the policy-engine.
#[derive(Clone, Debug, Default)]
pub struct PolicyEngineSettings {
    pub(crate) service: ServiceSettings,
    pub(crate) status: StatusSettings,
}

impl PolicyEngineSettings {
    pub fn validate_config(_cfg: FileConfig) -> Fallible<Self> {
        // TODO(lucab): translate config entries.
        let settings = PolicyEngineSettings::default();
        Ok(settings)
    }
}

/// Runtime settings for the main service (graph endpoint) server.
#[derive(Clone, Debug)]
pub struct ServiceSettings {
    pub(crate) origin_allowlist: Option<Vec<String>>,
    pub(crate) bloom_max_population: usize,
    pub(crate) bloom_size: usize,
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
    pub(crate) upstream_base: reqwest::Url,
    pub(crate) upstream_req_timeout: Duration,
}

impl ServiceSettings {
    /// Default maximum expected unique IDs to track in the Bloom filter.
    const DEFAULT_BLOOM_MAX_MEMBERS: usize = 1_000_000;
    /// Default size of the Bloom filter for unique IDs tracking.
    const DEFAULT_BLOOM_SIZE: usize = 10 * 1024 * 1024; // 10 MiB
    /// Default IP address for policy-engine main service.
    const DEFAULT_PE_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for policy-engine main service.
    const DEFAULT_PE_SERVICE_PORT: u16 = 8081;
    /// Default address of the upstream graph endpoint. This is usually
    /// a graph-builder running in the same pod.
    const DEFAULT_UP_ENDPOINT: &'static str = "http://127.0.0.1:8080/v1/graph";
    /// Default timeout for HTTP requests (30 minutes).
    const DEFAULT_UP_REQ_TIMEOUT: Duration = Duration::from_secs(30 * 60);

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            origin_allowlist: None,
            bloom_max_population: Self::DEFAULT_BLOOM_MAX_MEMBERS,
            bloom_size: Self::DEFAULT_BLOOM_SIZE,
            ip_addr: Self::DEFAULT_PE_SERVICE_ADDR.into(),
            port: Self::DEFAULT_PE_SERVICE_PORT,
            upstream_base: reqwest::Url::parse(Self::DEFAULT_UP_ENDPOINT)
                .expect("invalid default upstream base endpoint"),
            upstream_req_timeout: Self::DEFAULT_UP_REQ_TIMEOUT,
        }
    }
}

/// Runtime settings for the status server.
#[derive(Clone, Debug)]
pub struct StatusSettings {
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
}

impl StatusSettings {
    /// Default IP address for policy-engine main service.
    const DEFAULT_PE_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for policy-engine status.
    const DEFAULT_PE_STATUS_PORT: u16 = 9081;

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for StatusSettings {
    fn default() -> Self {
        Self {
            ip_addr: Self::DEFAULT_PE_SERVICE_ADDR.into(),
            port: Self::DEFAULT_PE_STATUS_PORT,
        }
    }
}
