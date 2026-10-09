"""AT-SPI observer kept behind a native-session boundary."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


class AtspiUnavailable(RuntimeError):
    """Raised when the system AT-SPI binding is not available."""


@dataclass(frozen=True)
class AccessibleMatch:
    name: str
    role: str
    process_id: int


class AtspiObserver:
    def __init__(self) -> None:
        try:
            import gi

            gi.require_version("Atspi", "2.0")
            from gi.repository import Atspi
        except (ImportError, ValueError) as error:
            raise AtspiUnavailable("libatspi GObject-Introspection bindings are required") from error
        self._atspi = Atspi

    def find(
        self, name: str, role: str | None = None, process_id: int | None = None
    ) -> AccessibleMatch | None:
        desktop = self._atspi.get_desktop(0)
        result = self._find_node(desktop, name, role, process_id)
        return result

    def _find_node(
        self, node: Any, name: str, role: str | None, process_id: int | None
    ) -> AccessibleMatch | None:
        if node is None:
            return None
        node_name = node.get_name() or ""
        node_role = node.get_role_name()
        node_process_id = node.get_process_id()
        if (
            node_name == name
            and (role is None or node_role == role)
            and (process_id is None or node_process_id == process_id)
        ):
            return AccessibleMatch(node_name, node_role, node_process_id)
        for index in range(node.get_child_count()):
            match = self._find_node(
                node.get_child_at_index(index), name, role, process_id
            )
            if match is not None:
                return match
        return None
