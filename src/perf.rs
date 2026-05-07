use serde::Serialize;
use std::collections::BTreeMap;
use std::env;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

const DEFAULT_SLOW_OPERATION_THRESHOLD_MS: u64 = 25;

#[derive(Debug, Clone)]
pub struct PerfConfig {
    pub enabled: bool,
    pub report_path: Option<PathBuf>,
    pub slow_operation_threshold: Duration,
}

impl PerfConfig {
    pub fn from_env() -> Self {
        Self {
            enabled: read_env_bool("JAME_PROMPT_PERF"),
            report_path: env::var("JAME_PROMPT_PERF_REPORT_PATH")
                .ok()
                .map(PathBuf::from),
            slow_operation_threshold: Duration::from_millis(read_env_u64(
                "JAME_PROMPT_PERF_SLOW_MS",
                DEFAULT_SLOW_OPERATION_THRESHOLD_MS,
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PerfSampleReport {
    pub label: String,
    pub count: u64,
    pub total_ms: f64,
    pub average_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PerfReport {
    pub app_name: String,
    pub app_version: String,
    pub enabled: bool,
    pub generated_at_unix_ms: u128,
    pub uptime_ms: f64,
    pub slow_operation_threshold_ms: f64,
    pub samples: Vec<PerfSampleReport>,
    pub slowest_samples: Vec<PerfSampleReport>,
}

#[derive(Debug, Default, Clone)]
struct PerfStats {
    count: u64,
    total: Duration,
    min: Option<Duration>,
    max: Option<Duration>,
}

impl PerfStats {
    fn record(&mut self, duration: Duration) {
        self.count += 1;
        self.total += duration;

        self.min = Some(match self.min {
            Some(current) if current <= duration => current,
            _ => duration,
        });

        self.max = Some(match self.max {
            Some(current) if current >= duration => current,
            _ => duration,
        });
    }

    fn to_report(&self, label: String) -> PerfSampleReport {
        let total_ms = duration_to_ms(self.total);
        let average_ms = if self.count == 0 {
            0.0
        } else {
            total_ms / self.count as f64
        };

        PerfSampleReport {
            label,
            count: self.count,
            total_ms,
            average_ms,
            min_ms: self.min.map(duration_to_ms).unwrap_or_default(),
            max_ms: self.max.map(duration_to_ms).unwrap_or_default(),
        }
    }
}

pub struct PerfRecorder {
    config: PerfConfig,
    started_at: Instant,
    samples: Mutex<BTreeMap<String, PerfStats>>,
    flushed: AtomicBool,
}

impl PerfRecorder {
    pub fn new(config: PerfConfig) -> Self {
        Self {
            config,
            started_at: Instant::now(),
            samples: Mutex::new(BTreeMap::new()),
            flushed: AtomicBool::new(false),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn measure<T>(&self, label: &'static str, f: impl FnOnce() -> T) -> T {
        if !self.is_enabled() {
            return f();
        }

        let started = Instant::now();
        let output = f();
        self.record(label, started.elapsed());
        output
    }

    pub fn record(&self, label: &'static str, duration: Duration) {
        if !self.is_enabled() {
            return;
        }

        let mut samples = match self.samples.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        samples
            .entry(label.to_string())
            .or_default()
            .record(duration);
    }

    pub fn snapshot(&self) -> PerfReport {
        let samples = match self.samples.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

        let mut sample_reports: Vec<PerfSampleReport> = samples
            .iter()
            .map(|(label, stats)| stats.to_report(label.clone()))
            .collect();
        sample_reports.sort_by(|left, right| left.label.cmp(&right.label));

        let mut slowest_samples = sample_reports.clone();
        slowest_samples.sort_by(|left, right| {
            right
                .total_ms
                .partial_cmp(&left.total_ms)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.label.cmp(&right.label))
        });
        slowest_samples.truncate(5);

        PerfReport {
            app_name: crate::config::APP_NAME.to_string(),
            app_version: crate::config::APP_VERSION.to_string(),
            enabled: self.is_enabled(),
            generated_at_unix_ms: unix_timestamp_ms(),
            uptime_ms: duration_to_ms(self.started_at.elapsed()),
            slow_operation_threshold_ms: duration_to_ms(self.config.slow_operation_threshold),
            samples: sample_reports,
            slowest_samples,
        }
    }

    pub fn flush(&self) -> Option<PerfReport> {
        if !self.is_enabled() {
            return None;
        }

        if self.flushed.swap(true, Ordering::SeqCst) {
            return Some(self.snapshot());
        }

        let report = self.snapshot();

        if let Some(path) = &self.config.report_path {
            if let Some(parent) = path.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    warn!(
                        path = %path.display(),
                        error = %error,
                        "Performance report directory could not be created"
                    );
                }
            }

            match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    if let Err(error) = std::fs::write(path, json) {
                        warn!(
                            path = %path.display(),
                            error = %error,
                            "Performance report could not be written"
                        );
                    }
                }
                Err(error) => {
                    warn!(error = %error, "Performance report could not be serialized");
                }
            }
        }

        self.log_report(&report);
        Some(report)
    }

    fn log_report(&self, report: &PerfReport) {
        if report.samples.is_empty() {
            info!(
                app = %report.app_name,
                version = %report.app_version,
                uptime_ms = report.uptime_ms,
                "Performance tracking enabled, but no samples were collected"
            );
            return;
        }

        let mut message = String::new();
        for sample in &report.slowest_samples {
            let _ = write!(
                &mut message,
                "{}={}ms({}x) ",
                sample.label, sample.total_ms, sample.count
            );
        }

        info!(
            app = %report.app_name,
            version = %report.app_version,
            uptime_ms = report.uptime_ms,
            slow_threshold_ms = report.slow_operation_threshold_ms,
            samples = report.samples.len(),
            slowest = %message.trim_end(),
            "Performance summary"
        );

        for sample in report
            .samples
            .iter()
            .filter(|sample| sample.max_ms >= report.slow_operation_threshold_ms)
        {
            warn!(
                label = %sample.label,
                count = sample.count,
                max_ms = sample.max_ms,
                average_ms = sample.average_ms,
                "Slow performance sample detected"
            );
        }
    }
}

static GLOBAL_RECORDER: OnceLock<PerfRecorder> = OnceLock::new();

pub fn global() -> &'static PerfRecorder {
    GLOBAL_RECORDER.get_or_init(|| PerfRecorder::new(PerfConfig::from_env()))
}

pub fn measure<T>(label: &'static str, f: impl FnOnce() -> T) -> T {
    global().measure(label, f)
}

#[allow(dead_code)]
pub fn record(label: &'static str, duration: Duration) {
    global().record(label, duration);
}

pub fn flush_global() -> Option<PerfReport> {
    GLOBAL_RECORDER.get().and_then(PerfRecorder::flush)
}

#[allow(dead_code)]
pub fn enabled() -> bool {
    global().is_enabled()
}

fn duration_to_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn unix_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn read_env_bool(name: &str) -> bool {
    matches!(
        env::var(name)
            .ok()
            .as_deref()
            .map(str::trim)
            .map(|value| value.to_ascii_lowercase()),
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on")
    )
}

fn read_env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn recorder_measures_and_orders_samples() {
        let recorder = PerfRecorder::new(PerfConfig {
            enabled: true,
            report_path: None,
            slow_operation_threshold: Duration::from_millis(1),
        });

        recorder.record("db.query", Duration::from_millis(4));
        recorder.record("db.query", Duration::from_millis(6));
        recorder.record("ui.render", Duration::from_millis(2));

        let report = recorder.snapshot();

        assert_eq!(report.samples.len(), 2);
        assert_eq!(report.slowest_samples[0].label, "db.query");
        assert_eq!(report.slowest_samples[0].count, 2);
        assert_eq!(report.slowest_samples[0].total_ms, 10.0);
    }

    #[test]
    fn recorder_measure_returns_value_without_losing_result() {
        let recorder = PerfRecorder::new(PerfConfig {
            enabled: true,
            report_path: None,
            slow_operation_threshold: Duration::from_millis(1),
        });

        let result = recorder.measure("startup.phase", || 42usize);

        assert_eq!(result, 42);
        assert_eq!(recorder.snapshot().samples.len(), 1);
    }

    #[test]
    fn recorder_can_write_report_to_disk() {
        let dir = tempdir().expect("temp dir should be created");
        let path = dir.path().join("perf-report.json");
        let recorder = PerfRecorder::new(PerfConfig {
            enabled: true,
            report_path: Some(path.clone()),
            slow_operation_threshold: Duration::from_millis(1),
        });

        recorder.record("tray.poll", Duration::from_millis(3));

        let report = recorder.flush().expect("report should be available");

        assert!(path.exists());
        assert_eq!(report.samples[0].label, "tray.poll");
    }

    #[test]
    fn disabled_recorder_is_noop() {
        let recorder = PerfRecorder::new(PerfConfig {
            enabled: false,
            report_path: None,
            slow_operation_threshold: Duration::from_millis(1),
        });

        recorder.record("ui.render", Duration::from_millis(7));

        assert!(recorder.snapshot().samples.is_empty());
        assert!(recorder.flush().is_none());
    }
}
