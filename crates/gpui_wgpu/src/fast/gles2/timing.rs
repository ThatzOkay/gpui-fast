use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

pub(crate) struct Timer(Option<Sample>);
struct Sample {
    id: u64,
    scope: &'static str,
    start_us: u128,
    start: Instant,
    last: Instant,
    stages: [(&'static str, f64); 12],
    count: usize,
}
impl Timer {
    pub(crate) fn new(scope: &'static str) -> Self {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        static NEXT: AtomicU64 = AtomicU64::new(1);
        if !*ENABLED.get_or_init(|| {
            matches!(
                std::env::var("GPUI_FRAME_TIMING").as_deref(),
                Ok("1" | "true")
            )
        }) {
            return Self(None);
        }
        let now = Instant::now();
        Self(Some(Sample {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            scope,
            start_us: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros(),
            start: now,
            last: now,
            stages: [("", 0.0); 12],
            count: 0,
        }))
    }
    pub(crate) fn mark(&mut self, stage: &'static str) {
        if let Some(s) = &mut self.0 {
            let now = Instant::now();
            if s.count < s.stages.len() {
                s.stages[s.count] = (stage, now.duration_since(s.last).as_secs_f64() * 1000.0);
                s.count += 1;
            }
            s.last = now;
        }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        self.mark("tail");
        if let Some(s) = &self.0 {
            log::info!(target: "gpui_frame_timing", "scope={} id={} start_us={} total_ms={:.3} stages_ms={:?}",
                s.scope, s.id, s.start_us, s.start.elapsed().as_secs_f64() * 1000.0, &s.stages[..s.count]);
        }
    }
}
