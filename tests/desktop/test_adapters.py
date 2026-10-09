"""Deterministic adapter tests that do not require a live desktop session."""

from .dbus_driver import GdbusClient
from .atspi_observer import AtspiObserver
from .protocol import ProtocolError, encode_request
from .sni_observer import StatusNotifierWatcher


def test_gdbus_adapter_keeps_arguments_out_of_a_shell() -> None:
    # The public adapter builds argv, never a shell command string.
    assert GdbusClient().timeout_seconds == 10.0
    assert encode_request(1, "state", {}) == (
        b'{"args":{},"operation":"state","request_id":1,"version":1}\n'
    )


def test_protocol_rejects_oversized_payloads() -> None:
    try:
        encode_request(1, "hello", {"payload": "x" * 70_000})
    except ProtocolError:
        return
    raise AssertionError("oversized controller payload was accepted")


def test_sni_parser_rejects_unowned_or_malformed_items() -> None:
    class TypedArray:
        def __init__(self, value):
            self.value = value

        def unpack(self):
            return self.value

    assert StatusNotifierWatcher.parse_registered_items(
        TypedArray([":1.4/org/kde/StatusNotifierItem"])
    ) == ((":1.4", "/org/kde/StatusNotifierItem"),)
    assert StatusNotifierWatcher.parse_registered_items(
        TypedArray([":1.4@/org/kde/StatusNotifierItem"])
    ) == ((":1.4", "/org/kde/StatusNotifierItem"),)
    assert StatusNotifierWatcher.parse_registered_items(
        TypedArray(["org.kde.StatusNotifierItem-42-1"])
    ) == (("org.kde.StatusNotifierItem-42-1", "/StatusNotifierItem"),)

    for value in (TypedArray(["not-an-owner"]), TypedArray([":1.4"]), TypedArray("bad")):
        try:
            StatusNotifierWatcher.parse_registered_items(value)
        except ValueError:
            pass
        else:
            raise AssertionError("malformed SNI item was accepted")


def test_atspi_match_requires_spawned_process_id() -> None:
    class Node:
        def __init__(self, name, role, process_id, children=()):
            self._name = name
            self._role = role
            self._process_id = process_id
            self._children = children

        def get_name(self):
            return self._name

        def get_role_name(self):
            return self._role

        def get_process_id(self):
            return self._process_id

        def get_child_count(self):
            return len(self._children)

        def get_child_at_index(self, index):
            return self._children[index]

    observer = object.__new__(AtspiObserver)
    root = Node("JamePrompt", "frame", 42, (None, Node("JamePrompt", "frame", 99)))
    assert observer._find_node(root, "JamePrompt", "frame", 7) is None
    match = observer._find_node(root, "JamePrompt", "frame", 42)
    assert match is not None
    assert match.process_id == 42
