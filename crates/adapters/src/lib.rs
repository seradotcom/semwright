//! First-party application-native adapters. They never evaluate agent-supplied code.
#[cfg(unix)]
pub mod blender;
#[cfg(unix)]
pub mod chromium;

#[cfg(not(unix))]
pub mod chromium {
    use semwright_types::{Error, Result};
    use serde::Deserialize;
    use std::path::PathBuf;

    #[derive(Debug, Clone, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct BrowserConfig {
        pub executable: PathBuf,
        #[serde(default)]
        pub allowed_origins: Vec<String>,
        #[serde(default)]
        pub allow_downloads: bool,
        #[serde(default = "default_download_bytes")]
        pub max_download_bytes: u64,
        #[serde(default = "default_total_download_bytes")]
        pub max_total_download_bytes: u64,
        #[serde(default = "default_download_count")]
        pub max_downloads: u32,
    }

    const fn default_download_bytes() -> u64 {
        32 * 1024 * 1024
    }
    const fn default_total_download_bytes() -> u64 {
        64 * 1024 * 1024
    }
    const fn default_download_count() -> u32 {
        8
    }

    impl Default for BrowserConfig {
        fn default() -> Self {
            Self {
                executable: PathBuf::new(),
                allowed_origins: vec![],
                allow_downloads: false,
                max_download_bytes: default_download_bytes(),
                max_total_download_bytes: default_total_download_bytes(),
                max_downloads: default_download_count(),
            }
        }
    }

    impl BrowserConfig {
        pub fn validate(&self) -> Result<()> {
            if self.allowed_origins.len() > 128
                || self.max_download_bytes == 0
                || self.max_total_download_bytes == 0
                || self.max_downloads == 0
                || self.max_download_bytes > self.max_total_download_bytes
            {
                return Err(Error::invalid("Invalid browser configuration"));
            }
            Ok(())
        }
    }
}
