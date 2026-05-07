use std::process::Command;
use tempfile::tempdir;

#[test]
fn perf_smoke_mode_writes_a_report_and_exits_cleanly() {
    let dir = tempdir().expect("temporary directory should be created");
    let report_path = dir.path().join("perf-report.json");
    let binary = env!("CARGO_BIN_EXE_jame-prompt");

    let output = Command::new(binary)
        .arg("--perf-smoke")
        .env("JAME_PROMPT_PERF", "1")
        .env("JAME_PROMPT_PERF_REPORT_PATH", &report_path)
        .env("JAME_PROMPT_PERF_SLOW_MS", "1")
        .output()
        .expect("perf smoke binary should run");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report_path.exists(),
        "perf report should be written at {}",
        report_path.display()
    );

    let report = std::fs::read_to_string(&report_path).expect("perf report should be readable");
    assert!(report.contains("\"smoke.app_init\""));
    assert!(report.contains("\"smoke.view\""));
}
