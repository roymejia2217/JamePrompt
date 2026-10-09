"""StatusNotifierItem observation through the standard session-bus API."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from .dbus_driver import GdbusClient


WATCHER_DESTINATION = "org.kde.StatusNotifierWatcher"
WATCHER_PATH = "/StatusNotifierWatcher"
WATCHER_INTERFACE = "org.kde.StatusNotifierWatcher"
PROPERTIES_INTERFACE = "org.freedesktop.DBus.Properties"
KSNI_ITEM_PREFIX = "org.kde.StatusNotifierItem-"
KSNI_ITEM_PATH = "/StatusNotifierItem"


class RegisteredItemUnavailable(RuntimeError):
    """Raised when a watcher entry disappears during owner resolution."""


@dataclass(frozen=True)
class StatusNotifierWatcher:
    bus: GdbusClient

    @staticmethod
    def parse_registered_items(value: Any) -> tuple[tuple[str, str], ...]:
        """Decode the typed ``as`` watcher property into owner/path pairs."""

        try:
            items = value.unpack()
        except AttributeError as error:
            raise ValueError("registered watcher items must be a typed GLib.Variant") from error
        if not isinstance(items, (list, tuple)):
            raise ValueError("registered watcher items must be an array")
        parsed: list[tuple[str, str]] = []
        for item in items:
            if not isinstance(item, str):
                raise ValueError("registered watcher item has no owner/path")
            if "/" not in item:
                if not item.startswith(KSNI_ITEM_PREFIX):
                    raise ValueError("registered watcher item has no owner/path")
                parsed.append((item, KSNI_ITEM_PATH))
                continue
            owner, object_path = item.split("/", 1)
            owner = owner.removesuffix("@")
            object_path = "/" + object_path
            if not owner.startswith(":") or not object_path.startswith("/"):
                raise ValueError("registered watcher item has invalid owner/path")
            parsed.append((owner, object_path))
        return tuple(parsed)

    def _typed_property(self) -> Any:
        try:
            import gi

            gi.require_version("Gio", "2.0")
            from gi.repository import Gio, GLib
        except (ImportError, ValueError) as error:
            raise RuntimeError("Gio GObject-Introspection bindings are required") from error
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        result = bus.call_sync(
            WATCHER_DESTINATION,
            WATCHER_PATH,
            PROPERTIES_INTERFACE,
            "Get",
            GLib.Variant("(ss)", (WATCHER_INTERFACE, "RegisteredStatusNotifierItems")),
            GLib.VariantType("(v)"),
            Gio.DBusCallFlags.NONE,
            10_000,
            None,
        )
        return result.get_child_value(0).get_variant()

    def registered_items(self) -> tuple[tuple[str, str], ...]:
        return self.parse_registered_items(self._typed_property())

    def _item_id(self, owner: str, object_path: str) -> tuple[str, int]:
        try:
            import gi

            gi.require_version("Gio", "2.0")
            from gi.repository import Gio, GLib
        except (ImportError, ValueError) as error:
            raise RuntimeError("Gio GObject-Introspection bindings are required") from error
        bus = Gio.bus_get_sync(Gio.BusType.SESSION, None)
        try:
            pid_result = bus.call_sync(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetConnectionUnixProcessID",
                GLib.Variant("(s)", (owner,)),
                GLib.VariantType("(u)"),
                Gio.DBusCallFlags.NONE,
                10_000,
                None,
            )
            process_id = pid_result.get_child_value(0).get_uint32()
            result = bus.call_sync(
                owner,
                object_path,
                PROPERTIES_INTERFACE,
                "Get",
                GLib.Variant("(ss)", ("org.kde.StatusNotifierItem", "Id")),
                GLib.VariantType("(v)"),
                Gio.DBusCallFlags.NONE,
                10_000,
                None,
            )
        except GLib.Error as error:
            raise RegisteredItemUnavailable(f"registered item {owner} disappeared") from error
        return result.get_child_value(0).get_variant().get_string(), process_id

    def is_registered(self, application_id: str, process_id: int | None = None) -> bool:
        for owner, object_path in self.registered_items():
            try:
                item_id, item_process_id = self._item_id(owner, object_path)
            except RegisteredItemUnavailable:
                continue
            if item_id == application_id and (
                process_id is None or item_process_id == process_id
            ):
                return True
        return False

    def has_registered_items(self) -> bool:
        """Return whether the watcher currently owns at least one item."""

        return bool(self.registered_items())
