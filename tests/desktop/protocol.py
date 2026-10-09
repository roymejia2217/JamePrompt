"""Typed NDJSON protocol primitives for the feature-gated Rust controller."""

from __future__ import annotations

from dataclasses import dataclass
import json
from typing import Any, TextIO


PROTOCOL_VERSION = 1
MAX_PACKET_BYTES = 64 * 1024


class ProtocolError(ValueError):
    """Raised when a controller frame violates the versioned contract."""


@dataclass(frozen=True)
class Response:
    request_id: int
    ok: bool
    result: dict[str, Any] | None
    error: dict[str, str] | None


@dataclass(frozen=True)
class Event:
    run_id: str
    operation_id: int
    sequence: int
    monotonic_ns: int
    revision: int | None
    window_generation: int | None
    event: str


def encode_request(request_id: int, operation: str, args: dict[str, Any]) -> bytes:
    if isinstance(request_id, bool) or not isinstance(request_id, int) or request_id < 1:
        raise ProtocolError("request_id must be a positive integer")
    if not operation or not isinstance(operation, str):
        raise ProtocolError("operation must be a non-empty string")
    if not isinstance(args, dict):
        raise ProtocolError("args must be an object")
    frame = {
        "version": PROTOCOL_VERSION,
        "request_id": request_id,
        "operation": operation,
        "args": args,
    }
    encoded = (json.dumps(frame, separators=(",", ":"), sort_keys=True) + "\n").encode()
    if len(encoded) > MAX_PACKET_BYTES:
        raise ProtocolError("request exceeds controller frame limit")
    return encoded


def decode_frame(line: bytes | str) -> Response | Event:
    raw_bytes = line if isinstance(line, bytes) else line.encode()
    if len(raw_bytes) > MAX_PACKET_BYTES:
        raise ProtocolError("response exceeds controller frame limit")
    if not raw_bytes.endswith(b"\n"):
        raise ProtocolError("controller frame is missing its newline delimiter")
    try:
        raw = raw_bytes[:-1].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ProtocolError("controller frame is not valid UTF-8") from error
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ProtocolError("response is not valid JSON") from error
    if not isinstance(value, dict) or value.get("version") != PROTOCOL_VERSION:
        raise ProtocolError("unsupported or malformed controller frame")
    if "ok" in value:
        required = {"version", "request_id", "ok", "result", "error"}
        if set(value) != required:
            raise ProtocolError("response contains an unexpected field")
        if (
            isinstance(value["request_id"], bool)
            or not isinstance(value["request_id"], int)
            or value["request_id"] < 1
            or not isinstance(value["ok"], bool)
        ):
            raise ProtocolError("response identity fields are invalid")
        result = value["result"]
        error = value["error"]
        if result is not None and not isinstance(result, dict):
            raise ProtocolError("response result must be an object or null")
        if error is not None:
            if not isinstance(error, dict) or set(error) != {"code", "message"}:
                raise ProtocolError("response error must contain code and message")
            if not all(isinstance(error[key], str) for key in ("code", "message")):
                raise ProtocolError("response error fields must be strings")
        if value["ok"] and error is not None:
            raise ProtocolError("successful response must have a null error")
        if not value["ok"] and result is not None:
            raise ProtocolError("failed response must have a null result")
        return Response(value["request_id"], value["ok"], result, error)
    required = {
        "version",
        "run_id",
        "operation_id",
        "sequence",
        "monotonic_ns",
        "revision",
        "window_generation",
        "event",
    }
    if set(value) != required or not isinstance(value["event"], str):
        raise ProtocolError("event contains an unexpected field")
    integer_fields = ("operation_id", "sequence", "monotonic_ns")
    if any(
        isinstance(value[field], bool) or not isinstance(value[field], int)
        for field in integer_fields
    ):
        raise ProtocolError("event numeric fields are invalid")
    for field in ("revision", "window_generation"):
        if value[field] is not None and (
            isinstance(value[field], bool) or not isinstance(value[field], int)
        ):
            raise ProtocolError("event correlation fields are invalid")
    if not isinstance(value["run_id"], str):
        raise ProtocolError("event run_id is invalid")
    return Event(
        run_id=value["run_id"],
        operation_id=value["operation_id"],
        sequence=value["sequence"],
        monotonic_ns=value["monotonic_ns"],
        revision=value["revision"],
        window_generation=value["window_generation"],
        event=value["event"],
    )


def read_frames(stream: TextIO) -> list[Response | Event]:
    return [decode_frame(line) for line in stream if line.strip()]
