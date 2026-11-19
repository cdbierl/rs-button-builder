//! Local Unix pseudo-terminal process management. No shell is implicitly invoked.
pub mod colors;
mod logging;
pub mod screen;
mod watcher;

use crate::layout_config::ButtonConfig;
use screen::Screen;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

enum Cleanup {
    Child(Child),
    Flush(mpsc::Sender<()>),
}

static REAPER: OnceLock<mpsc::Sender<Cleanup>> = OnceLock::new();

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub struct Terminal {
    pub screen: Screen,
    pub status: String,
    master: Option<File>,
    child: Option<Child>,
    input: VecDeque<u8>,
    output: VecDeque<u8>,
    eof: bool,
    exit_status: Option<ExitStatus>,
    keep_open: bool,
    close_on_success: bool,
    log: Option<logging::OutputLog>,
    watcher: Option<watcher::Watcher>,
    wake: Option<Wake>,
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
        let directory =
            std::env::var_os("BUTTON_BUILDER_LOG_DIR").filter(|value| !value.is_empty());
        Self::start_with_log_dir(button, cols, rows, directory.as_deref().map(Path::new))
    }

    fn start_with_log_dir(
        button: &ButtonConfig,
        cols: usize,
        rows: usize,
        directory: Option<&Path>,
    ) -> Result<Self, String> {
        let args = button.args()?;
        let (cols, rows) = (cols.clamp(2, 500), rows.clamp(2, 200));
        let mut terminal = Self::spawn(button, args, cols, rows)
            .map_err(|error| format!("Could not start {}: {error}", button.command))?;
        if button.log_output
            && let Some(directory) = directory
        {
            terminal.log = Some(
                logging::OutputLog::start(directory, &button.label)
                    .map_err(|error| format!("Could not start terminal logging: {error}"))?,
            );
        }
        Ok(terminal)
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
            // Advertise colors so terminfo-based apps such as htop enable their themes.
            .env("TERM", "xterm-256color")
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
            exit_status: None,
            keep_open: false,
            close_on_success: button.close_on_success,
            log: None,
            watcher: None,
            wake: None,
        })
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    pub(crate) fn watch(&mut self, wake: Wake) -> io::Result<()> {
        if let Some(master) = &self.master {
            self.watcher = Some(watcher::Watcher::new(master, wake.clone())?);
        }
        if let Some(log) = &mut self.log {
            log.set_waker(wake.clone());
        }
        self.wake = Some(wake);
        self.arm_watcher();
        Ok(())
    }

    /// Buffered work needs another frame even when the PTY is no longer readable.
    pub(crate) fn has_pending_work(&self) -> bool {
        (!self.input.is_empty() && self.watcher.is_none())
            || !self.output.is_empty()
            || self.log.as_ref().is_some_and(|log| log.has_pending_write())
    }

    fn arm_watcher(&mut self) {
        if self.eof {
            self.watcher = None;
            return;
        }
        if let Some(watcher) = &mut self.watcher {
            watcher.arm(
                self.output.is_empty()
                    && self.log.as_ref().is_none_or(|log| !log.has_pending_write()),
                !self.input.is_empty(),
            );
        }
    }

    /// Keep polling until final process output and queued log writes have finished.
    pub fn needs_poll(&self) -> bool {
        self.is_running()
            || !self.eof
            || !self.output.is_empty()
            || self.log.as_ref().is_some_and(|log| !log.finished)
    }

    /// Close only after successful completion and draining output, unless an error or user
    /// interruption requires attention. By default, retain runs that ever printed text.
    pub fn should_auto_close(&self) -> bool {
        self.exit_status.is_some_and(|status| status.success())
            && (self.close_on_success || !self.screen.has_printed_text())
            && !self.keep_open
            && self.log_error().is_none()
            && !self.needs_poll()
    }

    pub fn log_path(&self) -> Option<&Path> {
        self.log.as_ref().and_then(|log| log.path.as_deref())
    }

    pub fn log_error(&self) -> Option<&str> {
        self.log.as_ref().and_then(|log| log.error.as_deref())
    }

    pub fn send(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.is_running() && self.input.len() + bytes.len() <= 65_536 {
            self.input.extend(bytes);
            if let Some(wake) = &self.wake {
                wake();
            }
        }
    }

    /// Bound parsing by elapsed time as well as bytes; unread output applies PTY backpressure.
    pub fn poll(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(2);
        self.check_exit();
        if let Some(log) = &mut self.log
            && !log.ready()
        {
            self.arm_watcher();
            return;
        }
        let mut processed = 0;
        while Instant::now() < deadline && processed < 16_384 {
            if self.output.is_empty() && !self.eof {
                let mut buffer = [0; 8192];
                let Some(master) = &mut self.master else {
                    break;
                };
                match master.read(&mut buffer) {
                    Ok(0) => self.eof = true,
                    Ok(count) => {
                        self.output.extend(&buffer[..count]);
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    // Linux reports EIO when the last slave descriptor closes.
                    Err(error) if error.raw_os_error() == Some(libc::EIO) => self.eof = true,
                    Err(error) => {
                        self.keep_open = true;
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
            let logged = count == 0
                || self.log.as_mut().is_none_or(|log| {
                    log.record(
                        &self.output.make_contiguous()[..count],
                        self.screen.has_printed_text(),
                    )
                });
            self.output.drain(..count);
            processed += count;
            self.send(&replies);
            if !logged {
                break;
            }
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
                    self.keep_open = true;
                    self.status = format!("Terminal input error: {error}");
                    self.input.clear();
                }
            }
        }
        if !self.is_running() && self.eof && self.output.is_empty() {
            let incomplete_utf8 = self.screen.finish();
            if let Some(log) = &mut self.log {
                if incomplete_utf8 {
                    log.record(&[], self.screen.has_printed_text());
                }
                log.close();
                log.poll();
            }
        }
        self.arm_watcher();
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
                self.keep_open = true;
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
        let exit_status = ExitStatus::from_raw(raw_status);
        self.exit_status = Some(exit_status);
        self.status = format!("Process finished: {exit_status}");
        let child = self.child.take().unwrap();
        terminate_groups(&child, self.master.as_ref());
        reap_in_background(child);
        self.input.clear();
    }

    /// Send SIGINT to the foreground job even when input is full or the PTY is in raw mode.
    pub fn interrupt(&mut self) {
        let Some(child) = &self.child else { return };
        self.keep_open = true;
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
            self.keep_open = true;
            self.status = format!("Could not resize terminal: {}", io::Error::last_os_error());
            return;
        }
        self.screen.resize(cols, rows);
    }

    pub fn stop(&mut self) {
        self.keep_open = true;
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
        self.watcher = None;
        self.eof = true;
        self.input.clear();
        self.output.clear();
        if let Some(log) = &mut self.log {
            log.close();
        }
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
    logging::finish_writes();
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

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "Process cleanup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn log_directory(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "button-builder-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ))
    }

    #[test]
    fn close_on_success_waits_for_output_and_preserves_logs() {
        for close_on_success in [false, true] {
            for exit_code in [0, 1] {
                for log_output in [false, true] {
                    let directory = log_directory("close-on-success");
                    let button = ButtonConfig {
                        command: format!(
                            "printf 'useful output'; printf 'stderr output' >&2; exit {exit_code}"
                        ),
                        shell: true,
                        log_output,
                        close_on_success,
                        ..Default::default()
                    };
                    let mut terminal =
                        Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
                    // Observe exit before polling: unread output must prevent closing even with opt-in.
                    wait_until(|| {
                        terminal.check_exit();
                        !terminal.is_running()
                    });
                    assert!(!terminal.should_auto_close());
                    wait_for(&mut terminal, |t| !t.needs_poll());
                    assert_eq!(terminal.screen.text().trim(), "useful outputstderr output");
                    assert_eq!(
                        terminal.should_auto_close(),
                        close_on_success && exit_code == 0
                    );
                    if log_output {
                        let path = terminal.log_path().unwrap().to_owned();
                        drop(terminal);
                        assert_eq!(std::fs::read(path).unwrap(), b"useful outputstderr output");
                    } else {
                        assert!(!directory.exists());
                    }
                }
            }
        }
    }

    #[test]
    fn close_on_success_keeps_logging_errors_visible() {
        let directory = log_directory("close-log-error");
        std::fs::write(&directory, "not a directory").unwrap();
        let button = ButtonConfig {
            command: "/usr/bin/printf".into(),
            arguments: "result".into(),
            log_output: true,
            close_on_success: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.exit_status.unwrap().success());
        assert!(terminal.log_error().is_some());
        assert!(!terminal.should_auto_close());
        assert!(terminal.screen.text().contains("result"));
    }

    #[test]
    fn silent_successes_and_failures_never_create_logs() {
        for (command, success) in [("/usr/bin/true", true), ("/usr/bin/false", false)] {
            let directory = log_directory("silent-run");
            let button = ButtonConfig {
                command: command.into(),
                log_output: true,
                ..Default::default()
            };
            let mut terminal =
                Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
            assert!(terminal.log_path().is_none());
            wait_for(&mut terminal, |t| !t.needs_poll());
            assert_eq!(terminal.should_auto_close(), success);
            assert!(terminal.log_error().is_none());
            assert!(terminal.log_path().is_none());
            assert!(
                !directory.exists(),
                "Silent runs must never create log directories"
            );
        }
    }

    #[test]
    fn control_only_output_never_creates_logs_and_only_success_is_silent() {
        for output in [
            "\\r\\033[2K\\033[3D", // Pause EO's exact log bytes.
            " \\t\\r\\n\\000\\007",
            "\\033]0;title\\007\\033Pmetadata\\033\\\\\\033[32m\\033[0m",
            "\u{200b}\u{200d}\u{feff}",
        ] {
            for status in [0, 1] {
                let directory = log_directory("control-only");
                let button = ButtonConfig {
                    command: format!("printf '{output}'; exit {status}"),
                    shell: true,
                    log_output: true,
                    ..Default::default()
                };
                let mut terminal =
                    Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
                wait_for(&mut terminal, |t| !t.needs_poll());
                assert_eq!(terminal.should_auto_close(), status == 0, "{output:?}");
                assert!(!terminal.screen.has_printed_text());
                assert!(terminal.log_error().is_none());
                assert!(terminal.log_path().is_none());
                assert!(!directory.exists());
            }
        }
    }

    #[test]
    fn incomplete_utf8_at_eof_is_visible_and_logged() {
        let directory = log_directory("incomplete-utf8");
        let button = ButtonConfig {
            command: "printf '\\342\\202'".into(),
            shell: true,
            log_output: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(!terminal.should_auto_close());
        assert!(terminal.screen.text().contains('\u{fffd}'));
        assert_eq!(
            std::fs::read(terminal.log_path().unwrap()).unwrap(),
            b"\xe2\x82"
        );
    }

    #[test]
    fn logging_waits_for_first_output_and_preserves_first_and_last_chunks() {
        let directory = log_directory("lazy-log");
        let button = ButtonConfig {
            command: "stty -echo; printf '\\r\\033[2K\\033[3D'; sleep 0.1; printf 'first chunk'; read line; printf 'last chunk'"
                .into(),
            shell: true,
            log_output: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        assert!(terminal.log_path().is_none());
        assert!(!directory.exists());
        wait_for(&mut terminal, |t| {
            t.log_path()
                .and_then(|path| std::fs::read(path).ok())
                .is_some_and(|bytes| bytes == b"\r\x1b[2K\x1b[3Dfirst chunk")
        });
        assert!(terminal.is_running());
        terminal.send(b"\n");
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.exit_status.unwrap().success());
        assert!(!terminal.should_auto_close());
        assert_eq!(
            std::fs::read(terminal.log_path().unwrap()).unwrap(),
            b"\r\x1b[2K\x1b[3Dfirst chunklast chunk"
        );
    }

    #[test]
    fn final_output_after_exit_and_a_cleared_screen_are_never_silent() {
        let directory = log_directory("erased-output");
        let button = ButtonConfig {
            command: "printf 'final output\\033[2J\\033[H'".into(),
            shell: true,
            log_output: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        let pid = terminal.child.as_ref().unwrap().id();
        wait_until(|| state(pid) == Some('Z'));
        terminal.check_exit();
        assert!(terminal.exit_status.unwrap().success());
        assert!(
            !terminal.screen.has_printed_text(),
            "The terminal has not read the queued output yet"
        );
        assert!(
            !terminal.should_auto_close(),
            "Exit status alone cannot close a tab"
        );
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.screen.text().trim().is_empty());
        assert!(!terminal.should_auto_close());
        assert_eq!(
            std::fs::read(terminal.log_path().unwrap()).unwrap(),
            b"final output\x1b[2J\x1b[H"
        );
    }

    #[test]
    fn stopped_and_interrupted_silent_runs_keep_tabs_without_creating_logs() {
        for interrupt in [false, true] {
            let directory = log_directory("silent-interruption");
            let button = ButtonConfig {
                command: "/bin/sleep".into(),
                arguments: "60".into(),
                log_output: true,
                ..Default::default()
            };
            let mut terminal =
                Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
            if interrupt {
                terminal.interrupt();
            } else {
                terminal.stop();
            }
            wait_for(&mut terminal, |t| !t.needs_poll());
            assert!(!terminal.should_auto_close());
            assert!(terminal.log_path().is_none());
            assert!(!directory.exists());
        }
        // Some programs handle SIGINT and still return zero; retain the requested interruption.
        let button = ButtonConfig {
            command: "trap 'exit 0' INT; kill -STOP $$; while :; do :; done".into(),
            shell: true,
            close_on_success: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        let pid = terminal.child.as_ref().unwrap().id();
        wait_until(|| state(pid) == Some('T'));
        terminal.interrupt();
        signal(pid as i32, libc::SIGCONT).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.exit_status.unwrap().success());
        assert!(!terminal.screen.has_printed_text());
        assert!(!terminal.should_auto_close());
    }

    #[test]
    fn logging_requires_both_button_opt_in_and_a_directory() {
        let directory = log_directory("opt-in");
        let mut button = ButtonConfig {
            command: "/usr/bin/printf".into(),
            arguments: "hello".into(),
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.screen.text().contains("hello"));
        assert!(terminal.log_path().is_none());
        assert!(
            !directory.exists(),
            "Unchecked buttons must not create log directories"
        );
        button.log_output = true;
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, None).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.screen.text().contains("hello"));
        assert!(terminal.log_path().is_none());
        assert!(!directory.exists());
    }

    #[test]
    fn logs_preserve_all_output_after_scrollback_eviction_and_screen_redraws() {
        let directory = log_directory("full-output");
        let button = ButtonConfig {
            label: "JSON / Events Stream".into(),
            log_output: true,
            command: "stty -onlcr; i=1; while [ \"$i\" -le 2300 ]; do printf 'line:%04d\\n' \"$i\"; i=$((i + 1)); done; printf '\\033[?1049h\\033[31mtransient λ\\033[0m\\rrewritten\\n\\033[?1049l'; printf 'stderr output\\n' >&2".into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.log_error().is_none());
        assert_eq!(terminal.screen.history.len(), screen::HISTORY_LIMIT);
        assert!(!terminal.screen.text().contains("line:0001"));
        let path = terminal.log_path().unwrap();
        assert_eq!(path.parent().unwrap(), directory.join("json_events_stream"));
        let saved = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            saved
                .lines()
                .filter(|line| line.starts_with("line:"))
                .count(),
            2300
        );
        assert!(saved.starts_with("line:0001\n"));
        assert!(saved.contains("line:2300\n"));
        assert!(saved.contains("\x1b[?1049h\x1b[31mtransient λ\x1b[0m\rrewritten\n\x1b[?1049l"));
        assert!(saved.ends_with("stderr output\n"));
    }

    #[test]
    fn concurrent_runs_log_live_output_separately_and_finish_after_stop_or_close() {
        let directory = log_directory("concurrent-logs");
        let mut terminals = Vec::new();
        for name in ["alpha", "beta"] {
            let button = ButtonConfig {
                label: "Run Top".into(),
                log_output: true,
                command: format!("stty -onlcr; printf '{name}-ready\\n'; read line"),
                shell: true,
                ..Default::default()
            };
            terminals
                .push(Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap());
        }
        for (terminal, name) in terminals.iter_mut().zip(["alpha", "beta"]) {
            wait_for(terminal, |t| {
                t.screen.text().contains("ready")
                    && t.log_path()
                        .and_then(|path| std::fs::read_to_string(path).ok())
                        .is_some_and(|text| text == format!("{name}-ready\n"))
            });
            assert!(
                terminal.is_running(),
                "Logs must be visible while the process runs"
            );
        }
        let paths: Vec<_> = terminals
            .iter()
            .map(|t| t.log_path().unwrap().to_owned())
            .collect();
        assert_ne!(paths[0], paths[1]);
        terminals[0].stop();
        drop(terminals);
        finish_cleanup();
        assert_eq!(std::fs::read_to_string(&paths[0]).unwrap(), "alpha-ready\n");
        assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), "beta-ready\n");
    }

    #[test]
    fn log_creation_errors_are_reported_without_blocking_process_controls() {
        let directory = log_directory("log-failure");
        std::fs::write(&directory, "not a directory").unwrap();
        let button = ButtonConfig {
            label: "Invalid log directory".into(),
            log_output: true,
            command: "printf ready; read line".into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start_with_log_dir(&button, 80, 24, Some(&directory)).unwrap();
        wait_for(&mut terminal, |t| {
            t.log_error().is_some() && t.screen.text().contains("ready")
        });
        let started = Instant::now();
        terminal.stop();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(!terminal.needs_poll());
        assert!(terminal.log_error().is_some());
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
    fn terminfo_reports_colors_and_styles_output() {
        let button = ButtonConfig {
            command: "printf 'colors:'; /usr/bin/tput colors; /usr/bin/tput setaf 2; printf 'green'; /usr/bin/tput sgr0".into(),
            shell: true,
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 80, 24).unwrap();
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.status.contains("exit status: 0"));
        assert_eq!(terminal.screen.text().trim(), "colors:256\ngreen");
        assert_eq!(
            terminal.screen.line(1)[0].style.fg_color,
            Some(colors::Color::Indexed(2))
        );
    }

    #[test]
    fn interactive_htop_renders_colors_and_accepts_quit() {
        if !Path::new("/usr/bin/htop").is_file() {
            eprintln!("Skipping htop integration check: /usr/bin/htop is unavailable");
            return;
        }
        // Use a fresh config so a user's monochrome theme cannot affect the check.
        let config = format!("HTOPRC={}", log_directory("htop-config").display());
        let button = ButtonConfig {
            command: "/usr/bin/env".into(),
            arguments: format!(
                "{} /usr/bin/htop --no-mouse",
                shlex::try_quote(&config).unwrap()
            ),
            ..Default::default()
        };
        let mut terminal = Terminal::start(&button, 100, 30).unwrap();
        wait_for(&mut terminal, |t| t.screen.text().contains("PID"));
        let colored = (0..30).any(|row| {
            terminal.screen.line(row).iter().any(|cell| {
                cell.style.fg_color.is_some_and(|color| {
                    let [r, g, b] = colors::Palette::new(true, None, None).resolve(color);
                    r != g || g != b
                })
            })
        });
        terminal.resize(90, 25);
        terminal.send(b"q");
        wait_for(&mut terminal, |t| !t.needs_poll());
        assert!(terminal.status.contains("exit status: 0"));
        assert!(
            colored,
            "htop should detect and emit colored terminal output"
        );
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
            let config = crate::layout_config::Config::load_existing(&path).unwrap();
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
