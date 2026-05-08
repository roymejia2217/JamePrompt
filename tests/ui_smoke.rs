#![cfg(target_os = "linux")]

use std::process::Command;
use std::thread::sleep;
use std::time::{Duration, Instant};
use tempfile::tempdir;

fn xdotool(args: &[&str]) {
    let output = Command::new("xdotool")
        .args(args)
        .output()
        .expect("xdotool should be available");

    assert!(
        output.status.success(),
        "xdotool {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wmctrl(args: &[&str]) {
    let output = Command::new("wmctrl")
        .args(args)
        .output()
        .expect("wmctrl should be available");

    assert!(
        output.status.success(),
        "wmctrl {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn wait_for_window(class_name: &str, timeout: Duration) -> String {
    let started = Instant::now();

    while started.elapsed() < timeout {
        let output = Command::new("xdotool")
            .args(["search", "--class", class_name])
            .output()
            .expect("xdotool search should run");

        if output.status.success() {
            let ids = String::from_utf8_lossy(&output.stdout);
            let mut best_window: Option<(String, u64)> = None;

            for window_id in ids.lines().map(str::trim).filter(|line| !line.is_empty()) {
                let geometry = Command::new("xwininfo")
                    .args(["-id", window_id])
                    .output()
                    .expect("xwininfo should run");

                if !geometry.status.success() {
                    continue;
                }

                let info = String::from_utf8_lossy(&geometry.stdout);
                let width = info
                    .lines()
                    .find(|line| line.contains("Width:"))
                    .and_then(|line| line.split_whitespace().last())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let height = info
                    .lines()
                    .find(|line| line.contains("Height:"))
                    .and_then(|line| line.split_whitespace().last())
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);
                let area = width.saturating_mul(height);

                if best_window
                    .as_ref()
                    .map_or(true, |(_, best_area)| area > *best_area)
                {
                    best_window = Some((window_id.to_string(), area));
                }
            }

            if let Some((window_id, _)) = best_window {
                return window_id;
            }
        }

        sleep(Duration::from_millis(250));
    }

    panic!("window with class {:?} did not appear", class_name);
}

#[test]
#[ignore]
fn ui_smoke_runs_real_window_activity_and_writes_a_report() {
    if std::env::var_os("DISPLAY").is_none() {
        panic!("DISPLAY must be set for the UI smoke test");
    }

    let dir = tempdir().expect("temporary directory should be created");
    let report_path = dir.path().join("ui-smoke-report.json");
    let binary = env!("CARGO_BIN_EXE_jame-prompt");

    let mut child = Command::new(binary)
        .arg("--ui-smoke")
        .env("JAME_PROMPT_PERF", "1")
        .env("JAME_PROMPT_PERF_REPORT_PATH", &report_path)
        .env("XDG_DATA_HOME", dir.path())
        .env("JAME_PROMPT_UI_SMOKE_DURATION_MS", "12000")
        .spawn()
        .expect("UI smoke binary should start");

    let window_id = wait_for_window("jame-prompt", Duration::from_secs(30));
    wmctrl(&["-ia", &window_id]);
    xdotool(&["windowmap", "--sync", &window_id]);
    xdotool(&["windowactivate", "--sync", &window_id]);
    let interaction_rounds = ["Alpha", "Beta", "Gamma", "Delta"];

    for text_value in interaction_rounds {
        xdotool(&["key", "--window", &window_id, "Tab"]);
        xdotool(&["type", "--window", &window_id, "--delay", "50", text_value]);
        sleep(Duration::from_secs(2));
    }

    assert!(
        child
            .try_wait()
            .expect("child wait should succeed")
            .is_none(),
        "UI smoke process should still be alive during the soak"
    );

    child.kill().expect("UI smoke process should be killable");
    let status = child.wait().expect("UI smoke process should wait");
    assert!(
        !status.success(),
        "UI smoke process should be terminated by the test harness"
    );
}
