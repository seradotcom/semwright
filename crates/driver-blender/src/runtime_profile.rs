//! Exact candidate runtime profiles; native acceptance is recorded separately.
use std::io;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Windows,
}
impl HostPlatform {
    pub fn current() -> io::Result<Self> {
        if cfg!(target_os = "linux") {
            Ok(Self::Linux)
        } else if cfg!(target_os = "windows") {
            Ok(Self::Windows)
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "No Blender runtime profile for this host",
            ))
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeProfile {
    host: HostPlatform,
    version: [u64; 3],
    resource_directory: &'static str,
    version_string: &'static str,
}
impl RuntimeProfile {
    pub fn exact(host: HostPlatform, version: [u64; 3]) -> io::Result<Self> {
        let (resource_directory, version_string) = match (host, version) {
            (HostPlatform::Linux, [4, 5, 14]) => ("4.5", "4.5.14 LTS"),
            (HostPlatform::Linux, [5, 2, 1]) | (HostPlatform::Windows, [5, 2, 1]) => {
                ("5.2", "5.2.1 LTS")
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Blender exact host/version pair is unsupported",
                ));
            }
        };
        Ok(Self {
            host,
            version,
            resource_directory,
            version_string,
        })
    }
    pub fn version(self) -> [u64; 3] {
        self.version
    }
    pub fn resource_directory(self) -> &'static str {
        self.resource_directory
    }
    pub fn version_string(self) -> &'static str {
        self.version_string
    }
    pub fn verify_report(host: HostPlatform, version: [u64; 3], banner: &str) -> io::Result<Self> {
        let p = Self::exact(host, version)?;
        if banner != p.version_string {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Blender version tuple and banner do not match the pinned profile",
            ));
        }
        Ok(p)
    }
    pub fn required_directories(self) -> &'static [&'static str] {
        match (self.host, self.version) {
            (HostPlatform::Linux, [4, 5, 14]) => &[
                "lib",
                "4.5/scripts",
                "4.5/extensions",
                "4.5/datafiles",
                "4.5/python",
            ],
            (HostPlatform::Linux, [5, 2, 1]) => &[
                "lib",
                "5.2/scripts",
                "5.2/extensions",
                "5.2/datafiles",
                "5.2/python",
            ],
            (HostPlatform::Windows, [5, 2, 1]) => &[
                "blender.shared",
                "blender.crt",
                "5.2/scripts",
                "5.2/extensions",
                "5.2/datafiles",
                "5.2/python",
            ],
            _ => unreachable!("RuntimeProfile is constructed only by exact"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_linux_profile_is_retained() {
        let p =
            RuntimeProfile::verify_report(HostPlatform::Linux, [4, 5, 14], "4.5.14 LTS").unwrap();
        assert_eq!(p.resource_directory(), "4.5");
        assert!(p.required_directories().contains(&"lib"));
    }
    #[test]
    fn windows_layout_is_explicit_and_distinct() {
        let p =
            RuntimeProfile::verify_report(HostPlatform::Windows, [5, 2, 1], "5.2.1 LTS").unwrap();
        assert_eq!(p.resource_directory(), "5.2");
        assert!(p.required_directories().contains(&"blender.shared"));
        assert!(!p.required_directories().contains(&"lib"));
    }
    #[test]
    fn version_ranges_and_wrong_banners_fail_closed() {
        for v in [[4, 5, 13], [4, 5, 15], [5, 2, 0], [5, 2, 2], [5, 3, 0]] {
            assert!(RuntimeProfile::exact(HostPlatform::Linux, v).is_err());
            assert!(RuntimeProfile::exact(HostPlatform::Windows, v).is_err());
        }
        assert!(RuntimeProfile::exact(HostPlatform::Windows, [4, 5, 14]).is_err());
        for b in ["5.2.1", "5.2.1 Alpha", "5.2.1 LTS\n", "4.5.14 LTS"] {
            assert!(RuntimeProfile::verify_report(HostPlatform::Windows, [5, 2, 1], b).is_err());
        }
    }
}
