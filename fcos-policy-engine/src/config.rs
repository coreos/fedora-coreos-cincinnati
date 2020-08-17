use failure::Fallible;
use std::path::Path;

/// Configuration file.
#[derive(Debug, Default)]
pub struct FileConfig {}

impl FileConfig {
    pub fn parse_file(_path: impl AsRef<Path>) -> Fallible<Self> {
        // TODO(lucab): translate config entries.
        let cfg = FileConfig::default();
        Ok(cfg)
    }
}
