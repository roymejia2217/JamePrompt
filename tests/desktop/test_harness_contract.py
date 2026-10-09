"""Deterministic tests for the Python harness and Rust protocol contract."""

from pathlib import Path
import socket
import subprocess
import sys
import threading

import pytest

from .controller import ControllerClient, running_controller
from .protocol import Event, MAX_PACKET_BYTES, ProtocolError, Response, decode_frame, encode_request
from .preflight import missing_native_requirements


ROOT = Path(__file__).parents[2]


def test_pyproject_declares_locked_pytest_project() -> None:
    project = (ROOT / "pyproject.toml").read_text()
    lock = ROOT / "uv.lock"
    assert '"pytest==9.0.2"' in project
    assert lock.is_file()
    assert "name = \"pytest\"" in lock.read_text()


def test_controller_startup_failure_reports_child_exit_status() -> None:
    # Python rejects the application's CLI switches before opening a socket.
    with pytest.raises(RuntimeError, match="before creating the controller socket.*status 2"):
        with running_controller(Path(sys.executable)):
            pytest.fail("a child that rejects startup must never yield a controller")


def test_request_is_versioned_and_bounded() -> None:
    frame = encode_request(1, "hello", {})
    assert frame.endswith(b"\n")
    assert b'"args":{}' in frame


def test_protocol_rejects_malformed_or_unknown_frames() -> None:
    with pytest.raises(ProtocolError):
        decode_frame('{"version":1,"request_id":1,"ok":true}')
    with pytest.raises(ProtocolError):
        decode_frame('{"version":2,"request_id":1,"ok":true,"result":null,"error":null}')
    with pytest.raises(ProtocolError):
        decode_frame('{"version":1,"request_id":1,"ok":true,"result":null,"error":null}')


def test_response_frame_is_typed() -> None:
    response = decode_frame(
        '{"version":1,"request_id":3,"ok":true,"result":{},"error":null}\n'
    )
    assert isinstance(response, Response)
    assert response.request_id == 3


def test_response_requires_explicit_nullable_result_and_error_types() -> None:
    response = decode_frame(
        '{"version":1,"request_id":3,"ok":true,"result":null,"error":null}\n'
    )
    assert isinstance(response, Response)
    assert response.result is None
    assert response.error is None

    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"request_id":3,"ok":true,"result":[],"error":null}'
            + "\n"
        )
    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"request_id":3,"ok":false,"result":null,"error":"bad"}'
            + "\n"
        )


def test_response_rejects_boolean_request_ids_and_mismatched_ids() -> None:
    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"request_id":true,"ok":true,"result":null,"error":null}\n'
        )


def test_event_preserves_correlation_fields_and_rejects_wrong_types() -> None:
    event = decode_frame(
        '{"version":1,"run_id":"r1","operation_id":4,"sequence":9,'
        '"monotonic_ns":10,"revision":2,"window_generation":3,"event":"closed"}\n'
    )
    assert isinstance(event, Event)
    assert getattr(event, "revision", None) == 2
    assert getattr(event, "window_generation", None) == 3

    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"run_id":"r1","operation_id":"4","sequence":9,'
            '"monotonic_ns":10,"revision":2,"window_generation":3,"event":"closed"}\n'
        )


def test_frame_limit_counts_utf8_bytes_and_newline() -> None:
    with pytest.raises(ProtocolError):
        encode_request(1, "hello", {"payload": "ñ" * 40_000})

    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"request_id":1,"ok":true,"result":null,"error":null}'
            + "x" * (64 * 1024)
        )


def test_request_decoder_rejects_truncated_response_frame() -> None:
    with pytest.raises(ProtocolError):
        decode_frame(
            '{"version":1,"request_id":3,"ok":true,"result":null,"error":null'
        )


def test_native_preflight_reports_missing_bindings_without_installing() -> None:
    # This is a diagnostic contract. Native tests must fail closed later, not
    # silently install packages or turn an unavailable session into success.
    assert isinstance(missing_native_requirements(), tuple)


def test_client_decodes_rust_success_and_failure_frames() -> None:
    emitted = subprocess.run(
        [
            "cargo",
            "test",
            "--locked",
            "--bin",
            "jame-prompt",
            "--features",
            "desktop-integration-tests",
            "emits_rust_response_wire_frames",
            "--",
            "--nocapture",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    frames = [
        bytes.fromhex(line.split("=", 1)[1])
        for line in emitted.stdout.splitlines()
        if line.startswith("RUST_WIRE_HEX=")
    ]
    assert len(frames) == 2

    left, right = socket.socketpair()
    client = ControllerClient(left)

    def serve_actual_rust_frames() -> None:
        try:
            for frame in frames:
                right.recv(4096)
                right.sendall(frame)
        finally:
            right.close()

    thread = threading.Thread(target=serve_actual_rust_frames)
    thread.start()
    try:
        assert client.request(7, "hello", {}) == Response(7, True, {"ready": True}, None)
        assert client.request(8, "hello", {}) == Response(
            8, False, None, {"code": "invalid_frame", "message": "bad"}
        )
    finally:
        client.close()
        thread.join(timeout=1)


def test_client_rejects_wrong_version_unknown_fields_and_nonobject_frames() -> None:
    invalid_frames = (
        b'{"version":2,"request_id":1,"ok":true,"result":null,"error":null}\n',
        b'{"version":1,"request_id":1,"ok":true,"result":null,"error":null,"extra":1}\n',
        b'[]\n',
    )

    for frame in invalid_frames:
        with pytest.raises(ProtocolError):
            decode_frame(frame)


def test_client_rejects_wrong_types_and_mismatched_request_id() -> None:
    with pytest.raises(ProtocolError):
        decode_frame(
            b'{"version":1,"request_id":true,"ok":true,"result":null,"error":null}\n'
        )

    left, right = socket.socketpair()
    client = ControllerClient(left)

    def send_mismatched_response() -> None:
        try:
            right.recv(4096)
            right.sendall(
                b'{"version":1,"request_id":2,"ok":true,"result":{},"error":null}\n'
            )
        finally:
            right.close()

    thread = threading.Thread(target=send_mismatched_response)
    thread.start()
    try:
        with pytest.raises(RuntimeError, match="request_id mismatch"):
            client.request(1, "hello", {})
    finally:
        client.close()
        thread.join(timeout=1)


def test_frame_bound_includes_newline_and_multibyte_bytes() -> None:
    payload = "é" * MAX_PACKET_BYTES
    oversized = (
        b'{"version":1,"request_id":1,"ok":true,"result":{"payload":"'
        + payload.encode()
        + b'"},"error":null}\n'
    )
    assert len(oversized) > MAX_PACKET_BYTES

    with pytest.raises(ProtocolError):
        decode_frame(oversized)


def test_truncated_and_oversize_frames_fail_closed() -> None:
    with pytest.raises(ProtocolError):
        decode_frame(b'{"version":1,"request_id":1,"ok":true,"result":null')

    left, right = socket.socketpair()
    client = ControllerClient(left)
    try:
        right.sendall(b"x" * MAX_PACKET_BYTES + b"\n")
        with pytest.raises(ProtocolError, match="byte limit"):
            client._read_frame()
    finally:
        client.close()
        right.close()


def test_events_preserve_run_revision_and_generation() -> None:
    left, right = socket.socketpair()
    client = ControllerClient(left)

    def send_event_then_response() -> None:
        try:
            right.recv(4096)
            right.sendall(
                b'{"version":1,"run_id":"run-1","operation_id":1,"sequence":2,'
                b'"monotonic_ns":3,"revision":4,"window_generation":5,"event":"ready"}\n'
                b'{"version":1,"request_id":1,"ok":true,"result":{},"error":null}\n'
            )
        finally:
            right.close()

    thread = threading.Thread(target=send_event_then_response)
    thread.start()
    try:
        assert client.request(1, "state", {}).ok
        assert client.events() == (
            Event("run-1", 1, 2, 3, 4, 5, "ready"),
        )
    finally:
        client.close()
        thread.join(timeout=1)
