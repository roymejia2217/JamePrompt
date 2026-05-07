use crate::launch::should_run_perf_smoke_from_args;
use crate::perf;
use crate::ui::JamePromptApp;

pub(crate) fn should_run() -> bool {
    should_run_perf_smoke_from_args(std::env::args_os())
}

pub(crate) fn run() {
    #[cfg(target_os = "linux")]
    if let Err(error) = gtk::init() {
        tracing::warn!("GTK initialization failed in perf smoke mode: {error}");
    }

    let app = perf::measure("smoke.app_init", JamePromptApp::default);
    perf::measure("smoke.view", || {
        let _ = app.view();
    });
    drop(app);
}
