use gtk::gio;

pub(super) fn session_connection() -> Result<gio::DBusConnection, String> {
    let address = gio::dbus_address_get_for_bus_sync(gio::BusType::Session, gio::Cancellable::NONE)
        .map_err(|error| format!("cannot resolve the session bus: {error}"))?;
    let connection = gio::DBusConnection::for_address_sync(
        &address,
        gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
            | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
        None,
        gio::Cancellable::NONE,
    )
    .map_err(|error| format!("cannot connect to the session bus: {error}"))?;
    connection.set_exit_on_close(false);
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_connections_are_independently_owned() {
        const CHILD: &str = "JAME_PROMPT_TEST_PRIVATE_BUS";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new("dbus-run-session")
                .arg("--")
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "hotkeys::connection::tests::worker_connections_are_independently_owned",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env_remove("DBUS_SESSION_BUS_ADDRESS")
                .env_remove("DISPLAY")
                .env_remove("WAYLAND_DISPLAY")
                .output()
                .expect("dbus-run-session is required for the portal contract test");
            assert!(
                output.status.success(),
                "isolated connection contract failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let toolkit = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
            .expect("toolkit connection");
        let shortcuts = session_connection().expect("shortcut worker connection");
        let paste = session_connection().expect("paste worker connection");
        assert_ne!(shortcuts.unique_name(), paste.unique_name());
        assert_ne!(shortcuts.unique_name(), toolkit.unique_name());
        assert_ne!(paste.unique_name(), toolkit.unique_name());
        shortcuts
            .close_sync(gio::Cancellable::NONE)
            .expect("close owned connection");
        assert!(
            !paste.is_closed(),
            "shortcut shutdown must not close paste connection"
        );
        assert!(
            !toolkit.is_closed(),
            "worker shutdown must not close toolkit connection"
        );
        paste
            .close_sync(gio::Cancellable::NONE)
            .expect("close paste connection");
    }
}
