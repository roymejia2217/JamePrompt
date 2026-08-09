use std::ffi::OsStr;

pub(crate) const START_MINIMIZED_ARG: &str = "--start-minimized";
pub(crate) const PERF_SMOKE_ARG: &str = "--perf-smoke";
pub(crate) const UI_SMOKE_ARG: &str = "--ui-smoke";

pub(crate) fn should_start_minimized() -> bool {
    should_start_minimized_from_args(std::env::args_os())
}

pub(crate) fn should_start_minimized_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(START_MINIMIZED_ARG))
}

pub(crate) fn should_run_perf_smoke_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(PERF_SMOKE_ARG))
}

pub(crate) fn should_run_ui_smoke_from_args<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .any(|arg| arg.as_ref() == OsStr::new(UI_SMOKE_ARG))
}

#[cfg(test)]
pub(crate) fn initial_window_visible(start_minimized: bool) -> bool {
    !start_minimized
}

pub(crate) fn should_show_window_after_hidden_start(
    start_minimized: bool,
    tray_available: bool,
) -> bool {
    start_minimized && !tray_available
}
