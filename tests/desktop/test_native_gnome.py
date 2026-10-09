"""Native GNOME tests, explicitly opt-in and fail-closed when prerequisites are absent."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import time

import pytest

from .atspi_observer import AtspiObserver
from .controller import running_controller
from .sni_observer import StatusNotifierWatcher
from .dbus_driver import GdbusClient
from .preflight import missing_native_requirements


pytestmark = pytest.mark.native_gnome


def _require_native_session() -> None:
    if os.environ.get("JAME_PROMPT_NATIVE_GNOME") != "1":
        pytest.skip("set JAME_PROMPT_NATIVE_GNOME=1 to run native GNOME tests")
    missing = missing_native_requirements()
    if missing:
        details = ", ".join(f"{item.name} ({item.detail})" for item in missing)
        pytest.fail(f"native GNOME preflight failed: {details}")


def _set_atspi_enabled(enabled: bool) -> None:
    subprocess.run(
        [
            "busctl",
            "--user",
            "set-property",
            "org.a11y.Bus",
            "/org/a11y/bus",
            "org.a11y.Status",
            "IsEnabled",
            "b",
            str(enabled).lower(),
        ],
        check=True,
        capture_output=True,
        text=True,
    )


def _wait_for_window(controller) -> None:
    deadline = time.monotonic() + 10
    request_id = 9_100_000_000
    while time.monotonic() < deadline:
        state = controller.request(request_id, "state", {})
        request_id += 1
        if state.ok and state.result and state.result["window_id_assigned"]:
            return
        time.sleep(0.05)
    pytest.fail("native window did not receive an assigned id")


def _wait_for_atspi(process_id: int, expected: bool) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        found = AtspiObserver().find("JamePrompt", process_id=process_id) is not None
        if found == expected:
            return
        time.sleep(0.1)
    pytest.fail(f"AT-SPI visibility did not become {expected} for PID {process_id}")


def _wait_for_sni(watcher: StatusNotifierWatcher, process_id: int, expected: bool) -> None:
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if watcher.is_registered(
            "io.github.roymejia2217.JamePrompt.Test.tray", process_id
        ) == expected:
            return
        time.sleep(0.1)
    pytest.fail(f"SNI registration did not become {expected} for PID {process_id}")


@pytest.fixture
def native_app():
    _require_native_session()
    binary = Path(os.environ.get("JAME_PROMPT_TEST_BINARY", "target/debug/jame-prompt"))
    assert binary.is_file(), f"missing feature-enabled binary: {binary}"
    activate_atspi = os.environ.get("JAME_PROMPT_NATIVE_ATSPI_SIGNAL") == "1"
    if activate_atspi:
        _set_atspi_enabled(False)
    try:
        with running_controller(binary) as controller:
            show = controller.request(1, "show_window", {})
            assert show.ok
            if activate_atspi:
                _wait_for_window(controller)
                _set_atspi_enabled(True)
            yield controller
    finally:
        if activate_atspi:
            _set_atspi_enabled(True)


def test_native_binary_controller_lifecycle_round_trip(native_app) -> None:
    hello = native_app.request(2, "hello", {})
    assert hello.ok
    assert hello.result is not None
    assert hello.result["protocol_version"] == 1

    state = native_app.request(3, "state", {})
    assert state.ok
    assert state.result is not None
    assert state.result["window_id_assigned"] is True

    close = native_app.request(4, "close_window", {})
    assert close.ok

    for request_id in range(5, 25):
        state = native_app.request(request_id, "state", {})
        assert state.ok
        assert state.result is not None
        if state.result["window_id_assigned"] is False:
            break
        time.sleep(0.05)
    else:
        pytest.fail("window remained assigned after close_window")

    reopen = native_app.request(26, "show_window", {})
    assert reopen.ok
    reopened_state = None
    for request_id in range(27, 47):
        reopened_state = native_app.request(request_id, "state", {})
        assert reopened_state.ok
        assert reopened_state.result is not None
        if reopened_state.result["window_id_assigned"]:
            break
        time.sleep(0.05)
    else:
        pytest.fail("window did not reopen after close_window")
    assert reopened_state is not None
    assert reopened_state.result["window_generation"] == state.result["window_generation"] + 1

    quit_response = native_app.request(47, "quit", {})
    assert quit_response.ok


def test_status_notifier_watcher_is_reachable(native_app) -> None:
    watcher = StatusNotifierWatcher(GdbusClient())
    hello = native_app.request(30, "hello", {})
    assert hello.ok and hello.result is not None
    _wait_for_sni(watcher, hello.result["pid"], True)
    quit_response = native_app.request(32, "quit", {})
    assert quit_response.ok
    _wait_for_sni(watcher, hello.result["pid"], False)


def test_application_is_visible_to_atspi(native_app) -> None:
    hello = native_app.request(31, "hello", {})
    assert hello.ok and hello.result is not None
    _wait_for_atspi(hello.result["pid"], True)
    close = native_app.request(32, "close_window", {})
    assert close.ok
    _wait_for_window_closed(native_app)
    _wait_for_atspi(hello.result["pid"], False)
    show = native_app.request(33, "show_window", {})
    assert show.ok
    _wait_for_window(native_app)
    _wait_for_atspi(hello.result["pid"], True)


def _wait_for_window_closed(controller) -> None:
    deadline = time.monotonic() + 10
    request_id = 9_200_000_000
    while time.monotonic() < deadline:
        state = controller.request(request_id, "state", {})
        request_id += 1
        if state.ok and state.result and not state.result["window_id_assigned"]:
            return
        time.sleep(0.05)
    pytest.fail("native window remained assigned after close")
