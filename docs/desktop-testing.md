# Desktop Testing Architecture

JamePrompt desktop integration tests have two explicit dependency layers.

## Dependency Layers

The native layer is installed by the operating-system package manager. It owns
the libraries and introspection data that are part of the running GNOME session:

- GTK/GObject introspection (`python3-gi` and GTK typelibs)
- AT-SPI (`gir1.2-atspi-2.0` and `at-spi2-core`)
- session D-Bus tools (`gdbus` and `busctl`)

The Python layer is a regular `uv` project. Its source contract is
`pyproject.toml`, its exact resolution is `uv.lock`, and its local environment
is `.venv`. The project currently owns `pytest`; it deliberately does not
pretend that native GTK/AT-SPI bindings are portable PyPI-only dependencies.
The observer uses the official `libatspi` GObject-Introspection API directly,
not the historical `pyatspi2` compatibility wrapper.

Create and use the environment with:

```sh
export UV_PYTHON=/usr/bin/python3
export UV_PYTHON_DOWNLOADS=never
uv venv --python /usr/bin/python3 --system-site-packages --allow-existing
uv sync --locked
uv run --locked pytest tests/desktop -q
```

`--system-site-packages` is intentional: the native bindings come from the
distribution and must match the active desktop stack. CI installs those system
packages before creating the environment.
The explicit interpreter also applies to `uv sync` and `uv run`, overriding
`.python-version`; a uv-managed interpreter does not provide the distribution's
compiled GObject bindings. Native CI runs preflight and pytest in this same
environment and disables automatic interpreter downloads.

## Runtime Wiring

```text
pytest
  -> protocol.py              versioned NDJSON encoder/decoder
  -> controller.py            Unix socket and binary lifecycle
  -> feature-enabled binary   production iced Message/update path
  -> dbus_driver.py            shell-free gdbus session-bus calls
  -> sni_observer.py           StatusNotifierWatcher observations
  -> atspi_observer.py         native accessibility-tree observations
```

The Rust binary owns production state transitions. The controller only sends
bounded, reviewed operations through the normal application message path. It
does not write SQLite directly, invoke a shell, mutate production data, or
accept TCP connections.
Synthetic startup resolves the production-data identity read-only, even when its
ancestors are absent on a fresh runner. It does not create production directories;
dangling symlinks and paths escaping the private run root fail validation.
Early controller startup failures report the child exit status without exposing
raw application output.

## Test Boundaries

`tests/desktop/test_harness_contract.py` and `test_adapters.py` are deterministic
and run in normal CI. They validate the protocol, limits, lockfile contract,
and adapter boundaries without requiring a desktop session.

`test_native_gnome.py` is marked `native_gnome` and is opt-in:

```sh
cargo build --locked --features desktop-integration-tests --bin jame-prompt \
  --target-dir target/desktop-feature
JAME_PROMPT_NATIVE_GNOME=1 \
JAME_PROMPT_NATIVE_ATSPI_SIGNAL=1 \
JAME_PROMPT_TEST_BINARY=target/desktop-feature/debug/jame-prompt \
  uv run --locked pytest -m native_gnome tests/desktop
```

Native tests fail closed when Wayland, GNOME, AT-SPI, GObject introspection, or
the required D-Bus tools are unavailable. Normal test collection skips the
native marker unless the explicit environment variable is set. The optional
`JAME_PROMPT_NATIVE_ATSPI_SIGNAL=1` gate emits a temporary false/true
`org.a11y.Bus` activation transition for AccessKit Unix, then restores the
session property to `true`; use it only in an authorized live session.

There is no aggregate Python runner. Pytest remains the test runner; each
transport, observer, fixture, and contract has its own module and boundary.
