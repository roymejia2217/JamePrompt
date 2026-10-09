"""Controller client. Process lifecycle and protocol transport stay separate."""

from __future__ import annotations

from contextlib import contextmanager
from collections import deque
import json
from pathlib import Path
import socket
import subprocess
import tempfile
import time
from typing import Any, Iterator

from .protocol import Event, MAX_PACKET_BYTES, ProtocolError, Response, decode_frame, encode_request


class ControllerClient:
    def __init__(self, connection: socket.socket) -> None:
        self._connection = connection
        self._reader = connection.makefile("rb")
        self._events: deque[Event] = deque(maxlen=64)

    def close(self) -> None:
        self._reader.close()
        self._connection.close()

    def request(self, request_id: int, operation: str, args: dict[str, Any]) -> Response:
        self._connection.sendall(encode_request(request_id, operation, args))
        while True:
            line = self._read_frame()
            if not line:
                raise RuntimeError("controller closed before replying")
            frame = decode_frame(line)
            if isinstance(frame, Event):
                self._events.append(frame)
                continue
            if frame.request_id != request_id:
                raise RuntimeError("controller response request_id mismatch")
            return frame

    def events(self) -> tuple[Event, ...]:
        return tuple(self._events)

    def _read_frame(self) -> bytes:
        frame = bytearray()
        while len(frame) < MAX_PACKET_BYTES:
            byte = self._reader.read(1)
            if not byte:
                return bytes(frame)
            frame.extend(byte)
            if byte == b"\n":
                return bytes(frame)
        byte = self._reader.read(1)
        if byte == b"\n":
            raise ProtocolError("controller frame exceeds the byte limit including newline")
        raise ProtocolError("controller frame exceeds the byte limit")


@contextmanager
def connected_controller(socket_path: Path) -> Iterator[ControllerClient]:
    connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        connection.settimeout(30)
        connection.connect(str(socket_path))
        client = ControllerClient(connection)
        try:
            yield client
        finally:
            client.close()
    except BaseException:
        connection.close()
        raise


def secure_run_directory() -> tempfile.TemporaryDirectory[str]:
    """Allocate the private parent required by the Rust controller contract."""

    directory = tempfile.TemporaryDirectory(prefix="jameprompt-desktop-")
    Path(directory.name).chmod(0o700)
    return directory


@contextmanager
def running_controller(binary: Path) -> Iterator[ControllerClient]:
    with secure_run_directory() as directory:
        root = Path(directory)
        socket_path = root / "controller.sock"
        profile_path = root / "profile.json"
        data_dir = root / "data"
        data_dir.mkdir()
        profile_path.write_text(
            json.dumps(
                {
                    "test_app_id": "io.github.roymejia2217.JamePrompt.Test",
                    "data_dir": str(data_dir),
                    "fixture_seed": "pytest",
                }
            )
        )
        process = subprocess.Popen(
            [
                str(binary),
                f"--desktop-test-controller={socket_path}",
                f"--desktop-test-profile={profile_path}",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            deadline = time.monotonic() + 15
            client: ControllerClient | None = None
            while client is None and time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError(
                        "test binary exited before creating the controller socket "
                        f"with status {process.returncode}"
                    )
                if socket_path.exists():
                    connection: socket.socket | None = None
                    try:
                        connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                        connection.settimeout(30)
                        connection.connect(str(socket_path))
                        client = ControllerClient(connection)
                    except ConnectionRefusedError:
                        if connection is not None:
                            connection.close()
                time.sleep(0.05)
            if client is None:
                raise TimeoutError("controller socket was not accepting connections")
            hello = client.request(9_000_000_001, "hello", {})
            if (
                not hello.ok
                or hello.result is None
                or hello.result.get("protocol_version") != 1
                or hello.result.get("pid") != process.pid
            ):
                raise RuntimeError("controller hello did not identify the spawned child")
            try:
                yield client
            finally:
                if process.poll() is None:
                    try:
                        client.request(9_000_000_002, "quit", {})
                    except (OSError, RuntimeError, ProtocolError):
                        pass
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.terminate()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait(timeout=5)
                client.close()
            if process.returncode not in (None, 0):
                raise RuntimeError(f"controller exited abnormally with status {process.returncode}")
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
