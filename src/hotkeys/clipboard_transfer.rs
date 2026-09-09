use gtk::gio;
use gtk::gio::prelude::*;
use gtk::glib;
use gtk::glib::variant::Handle;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum TransferError {
    InvalidReply,
    InvalidHandle,
    Cancelled,
    Timeout,
    Io(String),
}

pub(super) fn take_reply_fd(
    reply: &glib::Variant,
    fds: &gio::UnixFDList,
) -> Result<OwnedFd, TransferError> {
    let (Handle(index),) = reply
        .get::<(Handle,)>()
        .ok_or(TransferError::InvalidReply)?;
    if index < 0 || index >= fds.length() {
        return Err(TransferError::InvalidHandle);
    }
    let raw = fds
        .get(index)
        .map_err(|error| TransferError::Io(error.to_string()))?;
    // GUnixFDList::get duplicates the descriptor; this OwnedFd owns that duplicate.
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

pub(super) fn write_async(
    fd: OwnedFd,
    bytes: Arc<[u8]>,
    timeout: Duration,
    cancel: &gio::Cancellable,
    done: impl FnOnce(Result<usize, TransferError>) + 'static,
) {
    if cancel.is_cancelled() {
        drop(fd);
        done(Err(TransferError::Cancelled));
        return;
    }
    if timeout.is_zero() {
        drop(fd);
        done(Err(TransferError::Timeout));
        return;
    }
    // Pollable async writes require a nonblocking descriptor, including portal pipes.
    let mut error = std::ptr::null_mut();
    let configured = unsafe { glib::ffi::g_unix_set_fd_nonblocking(fd.as_raw_fd(), 1, &mut error) };
    if configured == 0 {
        let message = if error.is_null() {
            "cannot configure nonblocking clipboard transfer".to_string()
        } else {
            let error: glib::Error = unsafe { glib::translate::from_glib_full(error) };
            error.to_string()
        };
        drop(fd);
        done(Err(TransferError::Io(message)));
        return;
    }
    let context = glib::MainContext::ref_thread_default();
    let timed_out = Arc::new(AtomicBool::new(false));
    let deadline_flag = timed_out.clone();
    let deadline_cancel = cancel.clone();
    let deadline = glib::timeout_source_new(
        timeout,
        Some("jameprompt-clipboard-transfer-timeout"),
        glib::Priority::DEFAULT,
        move || {
            deadline_flag.store(true, Ordering::Relaxed);
            deadline_cancel.cancel();
            glib::ControlFlow::Break
        },
    );
    deadline.attach(Some(&context));
    // Ownership moves into GIO; no other owner may close this descriptor.
    let stream = unsafe { gio::UnixOutputStream::take_fd(fd) };
    let completion_stream = stream.clone();
    let completion_cancel = cancel.clone();
    let expected = bytes.len();
    stream.write_all_async(
        bytes,
        glib::Priority::DEFAULT,
        Some(cancel),
        move |result| {
            deadline.destroy();
            let outcome = if timed_out.load(Ordering::Relaxed) {
                Err(TransferError::Timeout)
            } else if completion_cancel.is_cancelled() {
                Err(TransferError::Cancelled)
            } else {
                match result {
                    Ok((_, count, None)) if count == expected => Ok(count),
                    Ok((_, _, Some(error))) | Err((_, error)) => {
                        Err(TransferError::Io(error.to_string()))
                    }
                    Ok((_, _, None)) => {
                        Err(TransferError::Io("incomplete clipboard transfer".into()))
                    }
                }
            };
            // Close even after cancellation so the receiver observes EOF before completion.
            let closed = completion_stream
                .close(gio::Cancellable::NONE)
                .map_err(|error| TransferError::Io(error.to_string()));
            done(outcome.and_then(|count| closed.map(|()| count)));
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk::glib::variant::{Handle, ToVariant};
    use std::cell::RefCell;
    use std::io::Read;
    use std::os::fd::AsFd;
    use std::os::unix::net::UnixStream;
    use std::rc::Rc;
    use std::time::Instant;

    fn run_write(
        writer: UnixStream,
        bytes: Arc<[u8]>,
        timeout: Duration,
        cancelled: bool,
    ) -> Result<usize, TransferError> {
        let context = glib::MainContext::new();
        context
            .with_thread_default(|| {
                let result = Rc::new(RefCell::new(None));
                let output = result.clone();
                let cancel = gio::Cancellable::new();
                if cancelled {
                    cancel.cancel();
                }
                write_async(writer.into(), bytes, timeout, &cancel, move |value| {
                    *output.borrow_mut() = Some(value);
                });
                let started = Instant::now();
                loop {
                    while context.pending() {
                        context.iteration(false);
                    }
                    if let Some(value) = result.borrow_mut().take() {
                        break value;
                    }
                    assert!(
                        started.elapsed() < Duration::from_secs(5),
                        "transfer callback stalled"
                    );
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
            .unwrap()
    }

    #[test]
    fn reply_handle_selects_fd_list_entry_not_process_descriptor() {
        let (decoy, _decoy_reader) = UnixStream::pair().unwrap();
        let (writer, mut reader) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let fds = gio::UnixFDList::new();
        assert_eq!(fds.append(decoy.as_fd()).unwrap(), 0);
        assert_eq!(fds.append(writer.as_fd()).unwrap(), 1);
        let fd = take_reply_fd(&(Handle(1),).to_variant(), &fds).unwrap();
        drop(fds);
        drop(writer);
        let mut output = std::fs::File::from(fd);
        std::io::Write::write_all(&mut output, b"correct descriptor").unwrap();
        drop(output);
        let mut received = Vec::new();
        reader.read_to_end(&mut received).unwrap();
        assert_eq!(received, b"correct descriptor");
    }

    #[test]
    fn reply_rejects_integer_instead_of_unix_handle() {
        let fds = gio::UnixFDList::new();
        assert_eq!(
            take_reply_fd(&(0i32,).to_variant(), &fds).unwrap_err(),
            TransferError::InvalidReply
        );
    }

    #[test]
    fn reply_rejects_out_of_range_handle() {
        let fds = gio::UnixFDList::new();
        assert_eq!(
            take_reply_fd(&(Handle(9),).to_variant(), &fds).unwrap_err(),
            TransferError::InvalidHandle
        );
    }

    #[test]
    fn large_unicode_snapshot_arrives_exactly_and_reader_observes_eof() {
        let expected = "árbol 🦀\n第二行\n".repeat(65536).into_bytes();
        let bytes: Arc<[u8]> = expected.clone().into();
        let (writer, mut reader) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        let receiver = std::thread::spawn(move || {
            let mut received = Vec::new();
            reader.read_to_end(&mut received).unwrap();
            received
        });
        assert_eq!(
            run_write(writer, bytes, Duration::from_secs(3), false).unwrap(),
            expected.len()
        );
        assert_eq!(receiver.join().unwrap(), expected);
    }

    #[test]
    fn unresponsive_receiver_times_out_and_descriptor_closes() {
        let (writer, mut reader) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let bytes: Arc<[u8]> = vec![b'x'; 4 * 1024 * 1024].into();
        assert_eq!(
            run_write(writer, bytes, Duration::from_millis(30), false),
            Err(TransferError::Timeout)
        );
        let mut partial = Vec::new();
        reader.read_to_end(&mut partial).unwrap();
        assert!(partial.len() < 4 * 1024 * 1024);
    }

    #[test]
    fn cancellation_does_not_send_prompt_bytes() {
        let (writer, mut reader) = UnixStream::pair().unwrap();
        reader
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert_eq!(
            run_write(
                writer,
                Arc::from(&b"private prompt"[..]),
                Duration::from_secs(2),
                true
            ),
            Err(TransferError::Cancelled)
        );
        let mut received = Vec::new();
        reader.read_to_end(&mut received).unwrap();
        assert!(received.is_empty());
    }

    #[test]
    fn closed_receiver_is_failure_not_success() {
        let (writer, reader) = UnixStream::pair().unwrap();
        drop(reader);
        assert!(matches!(
            run_write(
                writer,
                Arc::from(&b"prompt"[..]),
                Duration::from_secs(2),
                false
            ),
            Err(TransferError::Io(_))
        ));
    }
}
