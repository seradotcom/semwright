use semwright_audio_domain::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SNAPSHOT_VERSION: u32 = 1;
const MAX_ROUTES: usize = 1024;
const MAX_REGIONS: usize = 100_000;
const MAX_SENDS: usize = 4096;
const MAX_PLUGINS: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteKind {
    Track,
    Bus,
    Master,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRegion {
    pub id: String,
    pub name: String,
    pub position: u64,
    pub source_start: u64,
    pub length: u64,
    pub source_id: String,
    pub source_name: String,
    pub source_path: Option<String>,
    pub source_frames: u64,
    pub source_channels: u16,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSend {
    pub target_route: String,
    pub gain_millidb: i32,
    pub enabled: bool,
    pub pre_fader: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePlugin {
    pub id: String,
    pub name: String,
    pub unique_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRoute {
    pub id: String,
    pub name: String,
    pub kind: RouteKind,
    pub channels: u16,
    pub muted: bool,
    pub soloed: bool,
    pub gain_millidb: i32,
    pub pan_milli: i16,
    pub regions: Vec<NativeRegion>,
    pub sends: Vec<NativeSend>,
    pub plugins: Vec<NativePlugin>,
    pub routing_complete: bool,
    pub sends_complete: bool,
    pub plugins_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArdourSnapshot {
    pub snapshot_version: u32,
    pub ardour_version: String,
    pub session_name: String,
    pub sample_rate: u32,
    pub session_start: u64,
    pub session_end: u64,
    pub routes: Vec<NativeRoute>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl ArdourSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.snapshot_version != SNAPSHOT_VERSION {
            return Err(Error::unsupported("Unsupported Ardour snapshot version"));
        }
        bounded_text("Ardour version", &self.ardour_version, 256)?;
        bounded_text("session name", &self.session_name, 4096)?;
        if !(8_000..=384_000).contains(&self.sample_rate)
            || self.session_end < self.session_start
            || self.session_end > i64::MAX as u64
            || self.routes.len() > MAX_ROUTES
        {
            return Err(Error::invalid("Invalid Ardour session shape"));
        }
        if self.warnings.len() > 256 {
            return Err(Error::limit("Ardour warning budget exceeded"));
        }
        for warning in &self.warnings {
            bounded_text("Ardour warning", warning, 2048)?;
        }

        let mut route_ids = BTreeSet::new();
        let mut region_count = 0usize;
        let mut send_count = 0usize;
        let mut plugin_count = 0usize;
        for route in &self.routes {
            token("route ID", &route.id)?;
            bounded_text("route name", &route.name, 4096)?;
            if !route_ids.insert(route.id.clone())
                || !(1..=64).contains(&route.channels)
                || !(-120_000..=24_000).contains(&route.gain_millidb)
                || !(-1000..=1000).contains(&route.pan_milli)
            {
                return Err(Error::invalid("Invalid or duplicate Ardour route"));
            }
            region_count = region_count.saturating_add(route.regions.len());
            send_count = send_count.saturating_add(route.sends.len());
            plugin_count = plugin_count.saturating_add(route.plugins.len());
            if region_count > MAX_REGIONS || send_count > MAX_SENDS || plugin_count > MAX_PLUGINS {
                return Err(Error::limit("Ardour snapshot collection budget exceeded"));
            }
            let mut region_ids = BTreeSet::new();
            for region in &route.regions {
                token("region ID", &region.id)?;
                token("source ID", &region.source_id)?;
                bounded_text("region name", &region.name, 4096)?;
                bounded_text("source name", &region.source_name, 4096)?;
                if !region_ids.insert(region.id.clone())
                    || region.length == 0
                    || region.source_frames == 0
                    || region.source_channels == 0
                    || region.source_channels > 64
                    || region.source_start.saturating_add(region.length) > region.source_frames
                {
                    return Err(Error::invalid("Invalid Ardour region"));
                }
                if let Some(path) = &region.source_path {
                    bounded_text("source path", path, 4096)?;
                }
            }
            for send in &route.sends {
                token("send target", &send.target_route)?;
                if !(-120_000..=24_000).contains(&send.gain_millidb) {
                    return Err(Error::invalid("Invalid Ardour send gain"));
                }
            }
            for plugin in &route.plugins {
                token("plugin ID", &plugin.id)?;
                bounded_text("plugin name", &plugin.name, 4096)?;
                if let Some(id) = &plugin.unique_id {
                    bounded_text("plugin unique ID", id, 1024)?;
                }
            }
        }
        Ok(())
    }
}

fn token(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}

fn bounded_text(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}
