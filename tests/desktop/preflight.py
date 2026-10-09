"""Explicit native-session prerequisites; no dependency installation occurs here."""

from __future__ import annotations

from dataclasses import dataclass
import importlib.util
import os
import shutil


def _atspi_gi_available() -> bool:
    if importlib.util.find_spec("gi") is None:
        return False
    try:
        import gi

        gi.require_version("Atspi", "2.0")
        from gi.repository import Atspi  # noqa: F401
    except (ImportError, ValueError):
        return False
    return True


@dataclass(frozen=True)
class Requirement:
    name: str
    available: bool
    detail: str


def native_requirements() -> tuple[Requirement, ...]:
    desktop = os.environ.get("XDG_CURRENT_DESKTOP", "")
    session = os.environ.get("DESKTOP_SESSION", "")
    gnome_session = "gnome" in f"{desktop}:{session}".lower()
    return (
        Requirement("wayland", bool(os.environ.get("WAYLAND_DISPLAY")), "WAYLAND_DISPLAY"),
        Requirement("gnome", gnome_session, "XDG_CURRENT_DESKTOP/DESKTOP_SESSION"),
        Requirement("busctl", shutil.which("busctl") is not None, "busctl executable"),
        Requirement("gdbus", shutil.which("gdbus") is not None, "gdbus executable"),
        Requirement("atspi_gi", _atspi_gi_available(), "libatspi GObject-Introspection binding"),
    )


def missing_native_requirements() -> tuple[Requirement, ...]:
    return tuple(requirement for requirement in native_requirements() if not requirement.available)
