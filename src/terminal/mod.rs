//! Local Unix pseudo-terminal process management. No shell is implicitly invoked.
pub mod screen;

use crate::config::ButtonConfig;
use screen::Screen;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

enum Cleanup {
    Child(Child),
    Flush(mpsc::Sender<()>),
}

static REAPER: OnceLock<mpsc::Sender<Cleanup>> = OnceLock::new();

pub struct Terminal {
    pub screen: Screen,
    pub status: String,
    master: Option<File>,
    child: Option<Child>,
    input: VecDeque<u8>,
    output: VecDeque<u8>,
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
            master: Some(master),
            child: Some(child),
            input: VecDeque::new(),
            output: VecDeque::new(),
            eof: false,
        })
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Keep polling until the child exits and its final output has been drained.
    pub fn needs_poll(&self) -> bool {
        self.is_running() || !self.eof || !self.output.is_empty()
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if self.is_running() && self.input.len() + bytes.len() <= 65_536 {
            self.input.extend(bytes);
        }
    }

    /// Bound parsing by elapsed time as well as bytes; unread output applies PTY backpressure.
    pub fn poll(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(2);
        self.check_exit();
        let mut processed = 0;
        while Instant::now() < deadline && processed < 16_384 {
            if self.output.is_empty() && !self.eof {
                let mut buffer = [0; 8192];
                let Some(master) = &mut self.master else {
                    break;
                };
                match master.read(&mut buffer) {
                    Ok(0) => self.eof = true,
                    Ok(count) => self.output.extend(&buffer[..count]),
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
            if self.output.is_empty() {
                break;
            }
            let (count, replies) = self
                .screen
                .feed_until(self.output.make_contiguous(), deadline);
            self.output.drain(..count);
            processed += count;
            self.send(&replies);
        }
        if !self.input.is_empty()
            && let Some(master) = &mut self.master
        {
            let bytes = self.input.make_contiguous();
            match master.write(&bytes[..bytes.len().min(4096)]) {
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
    }

    fn check_exit(&mut self) {
        let Some(child) = &self.child else { return };
        // Observe without reaping: retaining the leader prevents its session ID being reused
        // while the cleanup worker terminates any remaining jobs in that session.
        // SAFETY: siginfo_t is a plain C record that accepts zero initialization.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: info is valid storage; WNOHANG never waits for the child to exit.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result == -1 {
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                self.status = format!("Could not read process status: {error}");
            }
            return;
        }
        // SAFETY: waitid initialized the child-status fields of siginfo.
        if unsafe { info.si_pid() } == 0 {
            return;
        }
        let code = unsafe { info.si_status() };
        let raw_status = if info.si_code == libc::CLD_EXITED {
            code << 8
        } else {
            code | if info.si_code == libc::CLD_DUMPED {
                0x80
            } else {
                0
            }
        };
        self.status = format!("Process finished: {}", ExitStatus::from_raw(raw_status));
        let child = self.child.take().unwrap();
        terminate_groups(&child, self.master.as_ref());
        reap_in_background(child);
        self.input.clear();
    }

    /// Send SIGINT to the foreground job even when input is full or the PTY is in raw mode.
    pub fn interrupt(&mut self) {
        let Some(child) = &self.child else { return };
        self.input.clear();
        let group = foreground_group(self.master.as_ref()).unwrap_or(child.id() as i32);
        if let Err(error) = signal(-group, libc::SIGINT) {
            self.status = format!("Could not interrupt process: {error}");
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = (cols.clamp(2, 500), rows.clamp(2, 200));
        if (cols, rows) == (self.screen.cols, self.screen.rows) {
            return;
        }
        let size = winsize(cols, rows);
        // SAFETY: master remains open, and size points to an initialized winsize.
        if let Some(master) = &self.master
            && unsafe { libc::ioctl(master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1
        {
            self.status = format!("Could not resize terminal: {}", io::Error::last_os_error());
            return;
        }
        self.screen.resize(cols, rows);
    }

    pub fn stop(&mut self) {
        if let Some(child) = &self.child {
            // Check the direct child as well, so permission errors are visible to the user.
            if let Err(error) = signal(child.id() as i32, libc::SIGKILL) {
                self.status = format!("Could not stop process: {error}");
                return;
            }
        }
        if let Some(child) = self.child.take() {
            terminate_groups(&child, self.master.as_ref());
            reap_in_background(child);
            self.status = "Stopped".into();
        }
        // Stop reading immediately, rather than draining an endlessly noisy process.
        self.master = None;
        self.eof = true;
        self.input.clear();
        self.output.clear();
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.stop();
    }
}

fn signal(pid: i32, signal: i32) -> io::Result<()> {
    // SAFETY: the caller supplies a PID/group belonging to this terminal session.
    if unsafe { libc::kill(pid, signal) } == -1 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) {
            return Err(error);
        }
    }
    Ok(())
}

fn foreground_group(master: Option<&File>) -> Option<i32> {
    // SAFETY: the descriptor refers to our open PTY master.
    let group = unsafe { libc::tcgetpgrp(master?.as_raw_fd()) };
    (group > 0).then_some(group)
}

fn terminate_groups(child: &Child, master: Option<&File>) {
    let leader = child.id() as i32;
    for pid in [
        Some(-leader),
        foreground_group(master).map(|group| -group),
        Some(leader),
    ]
    .into_iter()
    .flatten()
    {
        if let Err(error) = signal(pid, libc::SIGKILL) {
            log::warn!("Could not kill terminal process/group {pid}: {error}");
        }
    }
}

fn reap_in_background(child: Child) {
    let sender = REAPER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Cleanup>();
        std::thread::Builder::new()
            .name("terminal-cleanup".into())
            .spawn(move || {
                let mut children = Vec::<Child>::new();
                loop {
                    let request = match receiver.recv_timeout(Duration::from_millis(10)) {
                        Ok(request) => Some(request),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut flushes = Vec::new();
                    for request in request.into_iter().chain(receiver.try_iter()) {
                        match request {
                            Cleanup::Child(child) => {
                                kill_session_jobs(child.id());
                                children.push(child);
                            }
                            Cleanup::Flush(reply) => flushes.push(reply),
                        }
                    }
                    // try_wait keeps one uninterruptible child from blocking cleanup of other tabs.
                    children.retain_mut(|child| match child.try_wait() {
                        Ok(Some(_)) => false,
                        Ok(None) => true,
                        Err(error) => {
                            log::warn!("Could not reap terminal child {}: {error}", child.id());
                            false
                        }
                    });
                    for reply in flushes {
                        let _ = reply.send(());
                    }
                }
            })
            .expect("could not start terminal cleanup worker");
        sender
    });
    if let Err(error) = sender.send(Cleanup::Child(child)) {
        log::error!("Terminal cleanup worker disconnected: {error}");
    }
}

/// Called after the GUI event loop ends, so queued session cleanup can finish before exit.
/// A kernel-stalled child cannot make application shutdown wait indefinitely.
pub fn finish_cleanup() {
    let Some(sender) = REAPER.get() else { return };
    let (reply, received) = mpsc::channel();
    if sender.send(Cleanup::Flush(reply)).is_ok()
        && received.recv_timeout(Duration::from_secs(1)).is_err()
    {
        log::warn!("Timed out completing terminal session cleanup during shutdown");
    }
}

/// Include background jobs with separate process groups, while the leader is still unreaped.
fn kill_session_jobs(session: u32) {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
        else {
            continue;
        };
        // Open a stable process handle first, so PID reuse cannot target an unrelated process.
        // SAFETY: pidfd_open returns a new owned descriptor or -1.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd == -1 {
            continue;
        }
        // SAFETY: successful pidfd_open returned a fresh descriptor, owned here exactly once.
        let handle = unsafe { File::from_raw_fd(fd as i32) };
        let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some((_, fields)) = stat.rsplit_once(") ") else {
            continue;
        };
        if fields
            .split_whitespace()
            .nth(3)
            .and_then(|field| field.parse::<u32>().ok())
            != Some(session)
        {
            continue;
        }
        // SAFETY: handle refers to the process whose session was inspected; no siginfo is needed.
        if unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                handle.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        } == -1
        {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                log::warn!("Could not kill terminal session job {pid}: {error}");
            }
        }
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

    fn state(pid: u32) -> Option<char> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(") ")?.1.chars().next()
    }

    fn wait_until(predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "Process cleanup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn stopped_non_reading_child_cannot_block_poll_interrupt_or_stop() {
        let button = ButtonConfig {
            command:
                "trap '' INT TERM; stty -echo -icanon; printf 'ready\\n'; kill -STOP $$; sleep 60"
                    .into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        let pid = terminal.child.as_ref().unwrap().id();
        wait_for(&mut terminal, |t| {
            t.screen.text().contains("ready") && state(pid) == Some('T')
        });
        terminal.send(&vec![b'x'; 65_536]);
        let started = Instant::now();
        for _ in 0..20 {
            terminal.poll();
        }
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(!terminal.input.is_empty());
        terminal.interrupt();
        assert!(
            terminal.input.is_empty(),
            "Interrupt must bypass queued input"
        );
        terminal.poll();
        assert!(
            terminal.is_running(),
            "This child intentionally ignores SIGINT"
        );
        let started = Instant::now();
        terminal.stop();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(!terminal.needs_poll());
        assert!(terminal.master.is_none());
        assert_eq!(terminal.status, "Stopped");
        wait_until(|| state(pid).is_none());
    }

    #[test]
    fn interrupt_reaches_the_foreground_process_in_raw_mode() {
        let button = ButtonConfig {
            command: "stty raw -echo; printf ready; exec sleep 60".into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        wait_for(&mut terminal, |t| t.screen.text().contains("ready"));
        terminal.interrupt();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.status.contains("signal: 2"), "{}", terminal.status);
    }

    #[test]
    fn closing_and_shell_exit_kill_background_jobs_in_separate_process_groups() {
        for wait in [true, false] {
            let button = ButtonConfig {
                command: format!(
                    "set -m; trap '' HUP; sleep 60 & printf 'job:%s\\n' \"$!\"; {}",
                    if wait { "wait" } else { "exit 0" }
                ),
                shell: true,
                ..Default::default()
            };
            let mut terminal = Terminal::start(&button, 80, 24).unwrap();
            let leader = terminal.child.as_ref().unwrap().id();
            wait_for(&mut terminal, |t| t.screen.text().contains("job:"));
            let job: u32 = terminal
                .screen
                .text()
                .lines()
                .find_map(|line| line.strip_prefix("job:"))
                .unwrap()
                .parse()
                .unwrap();
            if wait {
                // SAFETY: querying the group of our test's background job does not send a signal.
                assert_ne!(unsafe { libc::getpgid(job as i32) }, leader as i32);
                assert!(terminal.is_running());
                let started = Instant::now();
                drop(terminal);
                assert!(started.elapsed() < Duration::from_millis(500));
                finish_cleanup();
            } else {
                wait_for(&mut terminal, |t| !t.needs_poll());
                assert!(terminal.status.contains("exit status: 0"));
            }
            wait_until(|| matches!(state(job), None | Some('Z')));
            wait_until(|| state(leader).is_none());
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
        assert!(terminal.needs_poll());
        wait_for(&mut terminal, |t| t.screen.text().contains("24 80"));
        terminal.send(b"hello\r");
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(!terminal.is_running());
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
    fn throttled_output_is_preserved_until_the_terminal_is_fully_drained() {
        let button = ButtonConfig {
            command: "i=1; while [ \"$i\" -le 1000 ]; do printf 'line:%04d:abcdefghijklmnopqrstuvwxyz0123456789\\n' \"$i\"; i=$((i + 1)); done".into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        let output = terminal.screen.text();
        assert_eq!(
            output
                .lines()
                .filter(|line| line.starts_with("line:"))
                .count(),
            1000
        );
        assert!(output.contains("line:0001:abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(output.contains("line:1000:abcdefghijklmnopqrstuvwxyz0123456789"));
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
        let pid = terminal.child.as_ref().unwrap().id();
        terminal.stop();
        assert!(!terminal.is_running());
        assert_eq!(terminal.status, "Stopped");
        wait_until(|| state(pid).is_none());
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
