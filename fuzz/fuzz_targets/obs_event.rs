#![no_main]
use libfuzzer_sys::fuzz_target;
use semwright_obs_driver::{
    bounds,
    events::EventEngine,
    lifecycle::Operations,
    refs::Registry,
    state::Stamp,
};
use std::time::Duration;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = bounds::parse(data, bounds::MAX_EVENT) {
        let mut engine = EventEngine::new(8);
        let mut refs = Registry::new(8, Duration::from_secs(1));
        let mut ops = Operations::default();
        let mut stamp = Stamp { generation: 1, graph_revision: 1 };
        let _ = engine.ingest(&value, 1, &mut stamp, &mut refs, &mut ops);
        assert!(engine.len() <= 8);
    }
});
