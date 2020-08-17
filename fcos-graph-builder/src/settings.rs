use super::config::FileConfig;
use failure::Fallible;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Runtime settings for the graph-builder.
#[derive(Clone, Debug, Default)]
pub struct GraphBuilderSettings {
    pub(crate) service: ServiceSettings,
    pub(crate) status: StatusSettings,
}

impl GraphBuilderSettings {
    pub fn validate_config(_cfg: FileConfig) -> Fallible<Self> {
        // TODO(lucab): translate config entries.
        let settings = GraphBuilderSettings::default();
        Ok(settings)
    }
}

/// Runtime settings for the main service (graph endpoint) server.
#[derive(Clone, Debug)]
pub struct ServiceSettings {
    pub(crate) allowed_origins: Vec<String>,
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
    pub(crate) streams: BTreeSet<String>,
}

impl ServiceSettings {
    /// Default allowed CORS origin.
    const DEFAULT_CORS_URL: &'static str = "https://builds.coreos.fedoraproject.org";
    /// Default IP address for graph-builder main service.
    const DEFAULT_GB_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for graph-builder main service.
    const DEFAULT_GB_SERVICE_PORT: u16 = 8080;
    /// Default streams to process.
    const DEFAULT_STREAMS: [&'static str; 3] = ["next", "stable", "testing"];

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            allowed_origins: vec![Self::DEFAULT_CORS_URL.to_string()],
            ip_addr: Self::DEFAULT_GB_SERVICE_ADDR.into(),
            port: Self::DEFAULT_GB_SERVICE_PORT,
            streams: Self::DEFAULT_STREAMS
                .iter()
                .map(ToString::to_string)
                .collect(),
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
    /// Default IP address for graph-builder main service.
    const DEFAULT_GB_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for graph-builder status.
    const DEFAULT_GB_STATUS_PORT: u16 = 9080;

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for StatusSettings {
    fn default() -> Self {
        Self {
            ip_addr: Self::DEFAULT_GB_SERVICE_ADDR.into(),
            port: Self::DEFAULT_GB_STATUS_PORT,
        }
    }
}
