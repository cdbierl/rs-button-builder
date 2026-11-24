//! Local Unix pseudo-terminal process management. No shell is implicitly invoked.
pub mod colors;
mod logging;
pub mod screen;
mod watcher;

use crate::layout_config::ButtonConfig;
use screen::Screen;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
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

fn log_directory(override_path: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    override_path
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .map(|home| home.join("Documents/ButtonBuilder/logs"))
        })
}

impl Terminal {
    pub fn start(button: &ButtonConfig, cols: usize, rows: usize) -> Result<Self, String> {
        let directory = log_directory(
            std::env::var_os("BUTTON_BUILDER_LOG_DIR"),
            std::env::var_os("HOME"),
        );
        Self::start_with_log_dir(button, cols, rows, directory.as_deref())
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
mod tests;
