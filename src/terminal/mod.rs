//! Local Unix pseudo-terminal process management. No shell is implicitly invoked.
pub mod screen;

use crate::config::ButtonConfig;
use screen::Screen;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

pub struct Terminal {
    pub screen: Screen,
    pub status: String,
    master: File,
    child: Option<Child>,
    input: VecDeque<u8>,
    eof: bool,
}

fn winsize(cols: usize, rows: usize) -> libc::winsize {
    libc::winsize {
        ws_row: rows as u16,
        ws_col: cols as u16,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

impl Terminal {
    pub fn start(button: &ButtonConfig, cols: usize, rows: usize) -> Result<Self, String> {
        let args = button.args()?;
        let (cols, rows) = (cols.clamp(2, 500), rows.clamp(2, 200));
        Self::spawn(button, args, cols, rows)
            .map_err(|error| format!("Could not start {}: {error}", button.command))
    }

    fn spawn(
        button: &ButtonConfig,
        args: Vec<String>,
        cols: usize,
        rows: usize,
    ) -> io::Result<Self> {
        let mut master_fd = -1;
        let mut slave_fd = -1;
        let size = winsize(cols, rows);
        // SAFETY: openpty initializes both descriptors; all pointers are valid or null.
        if unsafe {
            libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        } == -1
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openpty returned fresh descriptors, each owned exactly once here.
        let master = unsafe { File::from_raw_fd(master_fd) };
        let slave = unsafe { File::from_raw_fd(slave_fd) };
        for fd in [master_fd, slave_fd] {
            // SAFETY: both descriptors are still open. Prevent leaking them through exec.
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: master_fd is an open descriptor. Only the master becomes nonblocking.
        let flags = unsafe { libc::fcntl(master_fd, libc::F_GETFL) };
        if flags == -1
            || unsafe { libc::fcntl(master_fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
        {
            return Err(io::Error::last_os_error());
        }
        let mut command = Command::new(if button.shell {
            "/bin/sh"
        } else {
            &button.command
        });
        command
            .args(args)
            .env("TERM", "vt100")
            .env_remove("COLORTERM")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        if !button.working_directory.is_empty() {
            command.current_dir(&button.working_directory);
        }
        // SAFETY: pre_exec calls only async-signal-safe libc operations and constructs IO errors.
        // Command has already installed the slave as stdin/stdout/stderr when this hook runs.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let status = format!("Running {} · PID {}", button.label, child.id());
        Ok(Self {
            screen: Screen::new(cols, rows),
            status,
            master,
            child: Some(child),
            input: VecDeque::new(),
            eof: false,
        })
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if self.is_running() && self.input.len() + bytes.len() <= 65_536 {
            self.input.extend(bytes);
        }
    }

    /// Work is bounded per frame so a noisy or non-reading child cannot freeze the UI.
    pub fn poll(&mut self) {
        let mut buffer = [0; 8192];
        for _ in 0..32 {
            if self.eof {
                break;
            }
            match self.master.read(&mut buffer) {
                Ok(0) => self.eof = true,
                Ok(count) => {
                    let replies = self.screen.feed(&buffer[..count]);
                    self.send(&replies);
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                // Linux reports EIO when the last slave descriptor closes.
                Err(error) if error.raw_os_error() == Some(libc::EIO) => self.eof = true,
                Err(error) => {
                    self.status = format!("Terminal read error: {error}");
                    self.eof = true;
                }
            }
        }
        if !self.input.is_empty() {
            match self.master.write(self.input.make_contiguous()) {
                Ok(count) => {
                    self.input.drain(..count);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    self.status = format!("Terminal input error: {error}");
                    self.input.clear();
                }
            }
        }
        if let Some(child) = &mut self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.status = format!("Process finished: {status}");
                    self.child = None;
                    self.input.clear();
                }
                Ok(None) => {}
                Err(error) => self.status = format!("Could not read process status: {error}"),
            }
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.clamp(2, 500), rows.clamp(2, 200));
        if (cols, rows) == (self.screen.cols, self.screen.rows) {
            return;
        }
        let size = winsize(cols, rows);
        // SAFETY: master remains open, and size points to an initialized winsize.
        if unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
            self.status = format!("Could not resize terminal: {}", io::Error::last_os_error());
            return;
        }
        self.screen.resize(cols, rows);
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // SAFETY: setsid made the unreaped child's PID its process group ID.
            // Kill the group as well as the direct child, and reap the child to avoid zombies.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            let _ = child.wait();
            self.status = "Stopped".into();
            self.input.clear();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for(terminal: &mut Terminal, predicate: impl Fn(&Terminal) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            terminal.poll();
            if predicate(terminal) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out: {}", terminal.status);
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn child_has_a_tty_receives_input_and_exits() {
        let button = ButtonConfig {
            command: "/bin/sh".into(),
            arguments: "-c 'test -t 0 && test -t 1 && test -t 2 && stty size; read value; printf \"received:%s\\n\" \"$value\"'".into(),
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        wait_for(&mut terminal, |t| t.screen.text().contains("24 80"));
        terminal.send(b"hello\r");
        wait_for(&mut terminal, |t| !t.is_running());
        terminal.poll();
        assert!(terminal.screen.text().contains("received:hello"));
    }

    #[test]
    fn interactive_top_renders_and_accepts_quit() {
        let button = ButtonConfig {
            command: "/usr/bin/top".into(),
            arguments: "-d 0.1".into(),
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 100, 30).unwrap();
        wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
        terminal.resize(90, 25);
        terminal.send(b"q");
        wait_for(&mut terminal, |t| !t.is_running());
        assert!(
            terminal.status.contains("exit status: 0"),
            "{}",
            terminal.status
        );
    }

    #[test]
    fn shell_pipeline_streams_json_through_jq() {
        let button = ButtonConfig {
            command: r#"printf '%s\n' '{"message":"hello"}' '{"message":"world"}' | /usr/bin/jq --unbuffered -r .message"#.into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        wait_for(&mut terminal, |t| !t.is_running());
        terminal.poll();
        assert!(
            terminal.screen.text().contains("hello\nworld"),
            "{}",
            terminal.screen.text()
        );
        assert!(terminal.status.contains("exit status: 0"));
    }

    #[test]
    fn launch_errors_are_reported_and_stop_reaps_child() {
        let mut button = ButtonConfig {
            command: "/no/such/program".into(),
            ..Default::default()
        };
        assert!(Terminal::start(&button, 80, 24).is_err());
        button.command = "/bin/sleep".into();
        button.arguments = "60".into();
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        terminal.stop();
        assert!(!terminal.is_running());
        assert_eq!(terminal.status, "Stopped");
    }

    #[test]
    fn every_bundled_example_command_runs_locally() {
        for name in [
            "system-monitor.yaml",
            "json-streams.yaml",
            "terminal-demo.yaml",
            "log-tail.yaml",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join(name);
            let config = crate::config::Config::load_existing(&path).unwrap();
            assert_eq!(config.buttons.len(), config.visible_buttons());
            for button in &config.buttons {
                let mut button = button.clone();
                let log = if button.command == "/usr/bin/tail" {
                    assert_eq!(
                        button.args().unwrap(),
                        ["-n", "50", "-F", "/var/log/syslog"]
                    );
                    let directory = std::env::temp_dir().join(format!(
                        "rs-button-builder-tail-test-{}-{}",
                        std::process::id(),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_nanos()
                    ));
                    std::fs::create_dir(&directory).unwrap();
                    let path = directory.join("syslog");
                    std::fs::write(&path, "existing log entry\n").unwrap();
                    button.arguments = format!(
                        "-n 50 -F --sleep-interval=0.1 {}",
                        shlex::try_quote(path.to_str().unwrap()).unwrap()
                    );
                    Some(path)
                } else {
                    None
                };
                let mut terminal = Terminal::start(&button, 120, 32)
                    .unwrap_or_else(|error| panic!("{name} / {}: {error}", button.label));
                if button.command == "/usr/bin/top" {
                    wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
                    terminal.send(b"q");
                } else if button.label == "Input Echo" {
                    wait_for(&mut terminal, |t| t.screen.text().contains("Your name:"));
                    terminal.send(b"Example\r");
                } else if let Some(path) = log {
                    wait_for(&mut terminal, |t| {
                        t.screen.text().contains("existing log entry")
                    });
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .unwrap();
                    writeln!(file, "appended log entry").unwrap();
                    wait_for(&mut terminal, |t| {
                        t.screen.text().contains("appended log entry")
                    });
                    std::fs::rename(&path, path.with_extension("1")).unwrap();
                    std::fs::write(&path, "rotated log entry\n").unwrap();
                    wait_for(&mut terminal, |t| {
                        t.screen.text().contains("rotated log entry")
                    });
                    assert!(
                        terminal.is_running(),
                        "Following must continue after rotation"
                    );
                    terminal.send(b"\x03");
                    wait_for(&mut terminal, |t| !t.is_running());
                    assert!(terminal.status.contains("signal: 2"), "{}", terminal.status);
                    continue;
                }
                wait_for(&mut terminal, |t| !t.is_running());
                terminal.poll();
                assert!(
                    terminal.status.contains("exit status: 0"),
                    "{name} / {}: {}",
                    button.label,
                    terminal.status
                );
                assert!(
                    !terminal.screen.text().trim().is_empty(),
                    "{name} / {} produced no output",
                    button.label
                );
            }
        }
    }
}
