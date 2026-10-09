"""Small, shell-free wrapper around the system gdbus CLI."""

from __future__ import annotations

from dataclasses import dataclass
import subprocess
from typing import Sequence


class DbusError(RuntimeError):
    """Raised when a session-bus operation fails."""


@dataclass(frozen=True)
class GdbusClient:
    timeout_seconds: float = 10.0

    def call(
        self,
        destination: str,
        object_path: str,
        interface: str,
        method: str,
        arguments: Sequence[str] = (),
    ) -> str:
        command = [
            "gdbus",
            "call",
            "--session",
            "--dest",
            destination,
            "--object-path",
            object_path,
            "--method",
            f"{interface}.{method}",
            *arguments,
        ]
        try:
            result = subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=self.timeout_seconds,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise DbusError(f"gdbus call could not complete: {error}") from error
        if result.returncode != 0:
            detail = result.stderr.strip() or "gdbus returned a failure"
            raise DbusError(detail)
        return result.stdout.strip()
