use crate::launch::should_run_perf_smoke_from_args;
use crate::perf;
use crate::ui::JamePromptApp;

pub(crate) fn should_run() -> bool {
    should_run_perf_smoke_from_args(std::env::args_os())
}

pub(crate) fn run() {
    let app = perf::measure("smoke.app_init", JamePromptApp::default);
    perf::measure("smoke.view", || {
        let _ = app.view();
    });
    drop(app);
}
