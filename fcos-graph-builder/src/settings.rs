use crate::config::FileConfig;
use failure::Fallible;
use std::collections::BTreeMap;
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
    pub(crate) origin_allowlist: Option<Vec<String>>,
    pub(crate) ip_addr: IpAddr,
    pub(crate) port: u16,
    // stream --> set of valid arches for it
    pub(crate) streams: BTreeMap<&'static str, &'static [&'static str]>,
}

impl ServiceSettings {
    /// Default IP address for graph-builder main service.
    const DEFAULT_GB_SERVICE_ADDR: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
    /// Default TCP port for graph-builder main service.
    const DEFAULT_GB_SERVICE_PORT: u16 = 8080;
    /// Default streams and their basearches to process.
    const DEFAULT_STREAMS: [(&'static str, &'static [&'static str]); 3] = [
        ("stable", &["x86_64", "aarch64", "s390x", "ppc64le"]),
        ("testing", &["x86_64", "aarch64", "s390x", "ppc64le"]),
        ("next", &["x86_64", "aarch64", "s390x", "ppc64le"]),
    ];

    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip_addr, self.port)
    }
}

impl Default for ServiceSettings {
    fn default() -> Self {
        Self {
            origin_allowlist: None,
            ip_addr: Self::DEFAULT_GB_SERVICE_ADDR.into(),
            port: Self::DEFAULT_GB_SERVICE_PORT,
            streams: Self::DEFAULT_STREAMS.iter().map(|&t| t).collect(),
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
