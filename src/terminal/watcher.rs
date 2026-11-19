//! Wait for PTY readiness without reading output or sharing terminal state.

use super::Wake;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

const READ: u8 = 1;
const WRITE: u8 = 2;

pub(super) struct Watcher {
    control: File,
    interest: Arc<AtomicU8>,
}

impl Watcher {
    pub fn new(master: &File, wake: Wake) -> io::Result<Self> {
        // Owning a duplicate prevents descriptor reuse while the worker is polling.
        let master = master.try_clone()?;
        let (mut receiver, control) = pipe()?;
        let interest = Arc::new(AtomicU8::new(0));
        let requested = interest.clone();
        std::thread::Builder::new()
            .name("terminal-watch".into())
            .spawn(move || {
                let mut armed = 0;
                loop {
                    let mut descriptors = [
                        libc::pollfd {
                            fd: receiver.as_raw_fd(),
                            events: libc::POLLIN,
                            revents: 0,
                        },
                        libc::pollfd {
                            // A negative descriptor is ignored, including HUP/ERR.
                            fd: if armed == 0 { -1 } else { master.as_raw_fd() },
                            events: (if armed & READ != 0 { libc::POLLIN } else { 0 })
                                | (if armed & WRITE != 0 { libc::POLLOUT } else { 0 }),
                            revents: 0,
                        },
                    ];
                    // SAFETY: both records refer to worker-owned descriptors and valid storage.
                    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, -1) };
                    if result < 0 {
                        if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                            continue;
                        }
                        wake();
                        return;
                    }
                    if descriptors[0].revents != 0 {
                        let mut buffer = [0; 64];
                        loop {
                            match receiver.read(&mut buffer) {
                                Ok(0) => return, // Dropping the UI handle stops the worker.
                                Ok(_) => {}
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                                    continue;
                                }
                                Err(_) => return,
                            }
                        }
                        armed = requested.load(Ordering::Acquire);
                    }
                    if descriptors[1].revents != 0 {
                        // Remain disarmed until the UI services the terminal and rearms us.
                        armed = 0;
                        wake();
                    }
                }
            })?;
        Ok(Self { control, interest })
    }

    pub fn arm(&mut self, read: bool, write: bool) {
        self.interest.store(
            (if read { READ } else { 0 }) | (if write { WRITE } else { 0 }),
            Ordering::Release,
        );
        // If the pipe is full, its existing bytes already wake the worker to read interest.
        loop {
            match self.control.write(&[1]) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => break,
            }
        }
    }
}

fn pipe() -> io::Result<(File, File)> {
    let mut descriptors = [-1; 2];
    // SAFETY: pipe2 initializes two descriptors in valid storage, owned below exactly once.
    if unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful pipe2 returned fresh read and write descriptors.
    Ok(unsafe {
        (
            File::from_raw_fd(descriptors[0]),
            File::from_raw_fd(descriptors[1]),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn readiness_waits_coalesces_and_rearms_without_consuming_output() {
        let (mut reader, mut writer) = pipe().unwrap();
        let (send, receive) = mpsc::channel();
        let mut watcher = Watcher::new(
            &reader,
            Arc::new(move || {
                let _ = send.send(());
            }),
        )
        .unwrap();
        watcher.arm(true, false);
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
        writer.write_all(b"hello").unwrap();
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
        let mut bytes = [0; 5];
        reader.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"hello");
        watcher.arm(true, false);
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
        writer.write_all(b"again").unwrap();
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        reader.read_exact(&mut bytes).unwrap();
        watcher.arm(true, false);
        drop(writer);
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
        drop(watcher);
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(1)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    #[test]
    fn blocked_input_wakes_only_when_the_pty_can_accept_writes() {
        let (mut reader, mut writer) = pipe().unwrap();
        while writer.write(&[0; 8192]).is_ok() {}
        let (send, receive) = mpsc::channel();
        let mut watcher = Watcher::new(
            &writer,
            Arc::new(move || {
                let _ = send.send(());
            }),
        )
        .unwrap();
        watcher.arm(false, true);
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
        while reader.read(&mut [0; 8192]).is_ok() {}
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(receive.recv_timeout(Duration::from_millis(50)).is_err());
    }
}
