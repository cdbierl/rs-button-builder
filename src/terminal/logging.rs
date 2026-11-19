//! Per-run output logs, written off the UI thread with bounded buffering.
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static CLOSING: Mutex<Vec<JoinHandle<()>>> = Mutex::new(Vec::new());

// Retain startup styling without allowing an indefinitely silent process to grow memory.
const CONTROL_PREFIX_LIMIT: usize = 65_536;
// Raw bytes and the screen parser's cumulative meaningful-output flag.
type LogChunk = (Vec<u8>, bool);

enum Update {
    Opened(PathBuf),
    Error(String),
    Finished,
}

pub(super) struct OutputLog {
    pub path: Option<PathBuf>,
    pub error: Option<String>,
    pub finished: bool,
    sender: Option<mpsc::SyncSender<LogChunk>>,
    tail: Option<mpsc::Sender<LogChunk>>,
    updates: mpsc::Receiver<Update>,
    pending: Option<LogChunk>,
    worker: Option<JoinHandle<()>>,
    wake: Arc<Mutex<Option<super::Wake>>>,
}

impl OutputLog {
    pub fn start(parent: &Path, label: &str) -> io::Result<Self> {
        let directory = parent.join(sanitize_label(label));
        let stamp = local_timestamp()?;
        // At most 32 chunks (256 KiB), plus one pending 8 KiB chunk, per terminal.
        let (sender, receiver) = mpsc::sync_channel::<LogChunk>(32);
        let (tail, remaining) = mpsc::channel::<LogChunk>();
        let (update, updates) = mpsc::channel();
        let wake = Arc::new(Mutex::new(None::<super::Wake>));
        let notify = wake.clone();
        let worker = std::thread::Builder::new()
            .name("terminal-log".into())
            .spawn(move || {
                let result = (|| -> io::Result<()> {
                    let mut file = None;
                    let mut prefix = VecDeque::new();
                    // Closing disconnects both channels after sending any pending chunk.
                    for (bytes, printed_text) in receiver.into_iter().chain(remaining) {
                        if file.is_none() {
                            if !printed_text {
                                prefix.extend(&bytes);
                                let excess = prefix.len().saturating_sub(CONTROL_PREFIX_LIMIT);
                                prefix.drain(..excess);
                                continue;
                            }
                            std::fs::create_dir_all(&directory)?;
                            let (path, mut opened) = create_log(&directory, &stamp)?;
                            let _ = update.send(Update::Opened(path));
                            notify_ui(&notify);
                            opened.write_all(prefix.make_contiguous())?;
                            prefix.clear();
                            file = Some(opened);
                        }
                        file.as_mut().unwrap().write_all(&bytes)?;
                    }
                    if let Some(mut file) = file {
                        file.flush()?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    log::error!("Terminal log error: {error}");
                    let _ = update.send(Update::Error(error.to_string()));
                }
                let _ = update.send(Update::Finished);
                notify_ui(&notify);
            })?;
        Ok(Self {
            path: None,
            error: None,
            finished: false,
            sender: Some(sender),
            tail: Some(tail),
            updates,
            pending: None,
            worker: Some(worker),
            wake,
        })
    }

    pub fn set_waker(&mut self, wake: super::Wake) {
        *self.wake.lock().unwrap() = Some(wake.clone());
        // The worker may already have completed before the callback was attached.
        wake();
    }

    pub fn has_pending_write(&self) -> bool {
        self.pending.is_some()
    }

    pub fn poll(&mut self) {
        loop {
            match self.updates.try_recv() {
                Ok(Update::Opened(path)) => self.path = Some(path),
                Ok(Update::Error(error)) => self.error = Some(error),
                Ok(Update::Finished) => self.finished = true,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.error = Some("Log worker disconnected".into());
                        self.finished = true;
                    }
                    break;
                }
            }
        }
    }

    /// Pause PTY reads when disk writes fall behind, without waiting on the UI thread.
    pub fn ready(&mut self) -> bool {
        self.poll();
        if let Some(bytes) = self.pending.take() {
            self.enqueue(bytes);
        }
        self.pending.is_none()
    }

    pub fn record(&mut self, bytes: &[u8], printed_text: bool) -> bool {
        debug_assert!(self.pending.is_none());
        self.enqueue((bytes.to_vec(), printed_text));
        self.pending.is_none()
    }

    fn enqueue(&mut self, bytes: LogChunk) {
        let Some(sender) = &self.sender else { return };
        if let Err(mpsc::TrySendError::Full(bytes)) = sender.try_send(bytes) {
            self.pending = Some(bytes);
        }
    }

    /// Drain queued output asynchronously, including the chunk waiting for queue capacity.
    pub fn close(&mut self) {
        if let Some(tail) = self.tail.take()
            && let Some(bytes) = self.pending.take()
        {
            let _ = tail.send(bytes);
        }
        self.sender = None;
        if let Some(worker) = self.worker.take()
            && !worker.is_finished()
        {
            let mut closing = CLOSING.lock().unwrap();
            closing.retain(|worker| !worker.is_finished());
            closing.push(worker);
        }
    }
}

fn notify_ui(notifier: &Mutex<Option<super::Wake>>) {
    let wake = notifier.lock().unwrap().clone();
    if let Some(wake) = wake {
        wake();
    }
}

impl Drop for OutputLog {
    fn drop(&mut self) {
        self.close();
    }
}

fn sanitize_label(label: &str) -> String {
    let mut name = String::new();
    for character in label.chars() {
        if character.is_ascii_alphanumeric() {
            name.push(character.to_ascii_lowercase());
        } else if !name.is_empty() && !name.ends_with('_') {
            name.push('_');
        }
        // Leave room below filesystem component limits, even for very long labels.
        if name.len() >= 128 {
            break;
        }
    }
    let name = name.trim_end_matches('_');
    if name.is_empty() {
        "button".into()
    } else {
        name.into()
    }
}

fn local_timestamp() -> io::Result<String> {
    let seconds: libc::time_t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs()
        .try_into()
        .map_err(io::Error::other)?;
    // SAFETY: tm is a C record accepting zero initialization; both pointers are valid.
    let mut local: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&seconds, &mut local) }.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec,
    ))
}

fn create_log(directory: &Path, stamp: &str) -> io::Result<(PathBuf, File)> {
    for suffix in 0_u64.. {
        let filename = if suffix == 0 {
            format!("{stamp}.log")
        } else {
            format!("{stamp}_{suffix}.log")
        };
        let path = directory.join(filename);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

/// Called after the GUI closes. A stalled filesystem cannot hold application exit indefinitely.
pub(super) fn finish_writes() {
    let workers = std::mem::take(&mut *CLOSING.lock().unwrap());
    let deadline = Instant::now() + Duration::from_secs(2);
    for worker in workers {
        while !worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if worker.is_finished() {
            if worker.join().is_err() {
                log::error!("Terminal log worker panicked");
            }
        } else {
            log::error!("Timed out finishing terminal logs; queued output may not be saved");
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn opening_and_finishing_a_log_wake_the_ui() {
        let directory = std::env::temp_dir().join(format!(
            "button-builder-log-wake-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let mut log = OutputLog::start(&directory, "Wake").unwrap();
        let (send, receive) = mpsc::channel();
        log.set_waker(Arc::new(move || {
            let _ = send.send(());
        }));
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        log.record(b"logged output", true);
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        log.poll();
        assert!(log.path.is_some());
        log.close();
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        log.poll();
        assert!(log.finished);
        assert_eq!(
            std::fs::read(log.path.as_ref().unwrap()).unwrap(),
            b"logged output"
        );
    }

    #[test]
    fn a_long_control_prefix_is_bounded_and_keeps_the_first_printable_chunk() {
        let directory = std::env::temp_dir().join(format!(
            "button-builder-control-prefix-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let mut log = OutputLog::start(&directory, "Controls then text").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        for _ in 0..12 {
            while !log.ready() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            log.record(&[0; 8192], false);
        }
        assert!(!directory.exists(), "Controls alone cannot open a log");
        while !log.ready() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        log.record(b"first printable text", true);
        log.close();
        while !log.finished {
            assert!(Instant::now() < deadline);
            log.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(log.error.is_none());
        let bytes = std::fs::read(log.path.as_ref().unwrap()).unwrap();
        assert_eq!(
            bytes.len(),
            CONTROL_PREFIX_LIMIT + b"first printable text".len()
        );
        assert!(bytes[..CONTROL_PREFIX_LIMIT].iter().all(|&byte| byte == 0));
        assert_eq!(&bytes[CONTROL_PREFIX_LIMIT..], b"first printable text");
    }

    #[test]
    fn labels_and_same_second_runs_have_safe_distinct_paths() {
        for (label, expected) in [
            ("Run Top", "run_top"),
            ("  JSON / Events: [Live]  ", "json_events_live"),
            ("../../etc/passwd", "etc_passwd"),
            ("__", "button"),
            ("hello\0world", "hello_world"),
        ] {
            assert_eq!(sanitize_label(label), expected);
        }
        assert_eq!(sanitize_label(&"A".repeat(1000)).len(), 128);
        let directory = std::env::temp_dir().join(format!(
            "button-builder-log-names-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let stamp = local_timestamp().unwrap();
        assert_eq!(stamp.len(), 15);
        assert_eq!(&stamp[8..9], "_");
        assert!(stamp.chars().all(|c| c.is_ascii_digit() || c == '_'));
        let (first, mut file) = create_log(&directory, &stamp).unwrap();
        file.write_all(b"first run").unwrap();
        let (second, mut file) = create_log(&directory, &stamp).unwrap();
        file.write_all(b"second run").unwrap();
        assert_eq!(first.file_name().unwrap(), format!("{stamp}.log").as_str());
        assert_eq!(
            second.file_name().unwrap(),
            format!("{stamp}_1.log").as_str()
        );
        assert_eq!(std::fs::read(first).unwrap(), b"first run");
        assert_eq!(std::fs::read(&second).unwrap(), b"second run");
        assert_eq!(
            std::fs::metadata(second).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn a_full_write_queue_stays_bounded_and_closes_without_losing_pending_output() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (tail, remaining) = mpsc::channel();
        let (_update, updates) = mpsc::channel();
        let mut log = OutputLog {
            wake: Arc::new(Mutex::new(None)),
            path: None,
            error: None,
            finished: false,
            sender: Some(sender),
            tail: Some(tail),
            updates,
            pending: None,
            worker: None,
        };
        assert!(log.record(b"first chunk", true));
        assert!(!log.record(b"last chunk", true));
        let started = Instant::now();
        for _ in 0..1000 {
            assert!(!log.ready(), "A stalled writer must apply backpressure");
        }
        log.close();
        assert!(started.elapsed() < Duration::from_millis(500));
        let saved: Vec<_> = receiver
            .into_iter()
            .chain(remaining)
            .flat_map(|(bytes, _)| bytes)
            .collect();
        assert_eq!(saved, b"first chunklast chunk");
    }
}
