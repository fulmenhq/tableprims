// SPDX-License-Identifier: MIT OR Apache-2.0
//! Process-run telemetry: a card and an append-only event stream, following
//! `contract: process-run/v0` of 3leaps Crucible (telemetry only; no control
//! channel).
//!
//! Telemetry is off unless a runtime directory is given. It is fail-open:
//! any problem disables it for the rest of the run and changes nothing else.
//! Every file operation runs on one emitter thread. The conversion only
//! updates atomic counters and sends messages, and waits a bounded time for
//! the terminal event at the end, so a stalled file system cannot hold it.
//!
//! Layout under the runtime directory `DIR`:
//!
//! - `DIR/proc/<run_id>/card.json`: the card, published once it has been
//!   checked, and removed when the run ends normally;
//! - `DIR/events/<run_id>.ndjson`: the event stream, kept.
//!
//! Directories are created owner-only and files owner read-write on Unix.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::identity;

/// The capability token of the contract.
pub(crate) const CAPABILITY: &str = "contract: process-run/v0";
/// This program's profile of event payloads.
pub(crate) const PROFILE: &str = "tableprims.xlsx-write-run/v0";
/// Environment variable naming the runtime directory when the flag is absent.
pub(crate) const RUNTIME_DIR_ENV: &str = "TABLEBOLT_RUNTIME_DIR";

/// A progress event is emitted at most this often, while bytes move.
const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);
/// A heartbeat is emitted when nothing else has been for this long.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// How long the end of a run waits for the terminal event to be written.
const FINISH_WAIT: Duration = Duration::from_secs(2);

/// The read of the rows the run is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Scan,
    Write,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Phase::Scan => "scan",
            Phase::Write => "write",
        }
    }
}

/// How the run ended, for the terminal event.
pub(crate) enum Terminal {
    /// A result with status `ok` was published.
    Completed {
        rows: u64,
        sha256: String,
        bytes: u64,
    },
    /// A result with status `cancelled` was published.
    Canceled { signal: &'static str },
    /// Anything else: a published result with status `refused` or `failed`,
    /// or a result that could not be published.
    Failed {
        status: &'static str,
        category: &'static str,
        rule: &'static str,
    },
}

/// The phase bit of [`Progress::state`].
const WRITE_PHASE: u64 = 1 << 63;

/// What the conversion updates and the emitter thread reads: the phase in
/// the top bit and the bytes read in this phase below it, in one value so
/// the two are always read together.
#[derive(Default)]
pub(crate) struct Progress {
    state: AtomicU64,
}

impl Progress {
    /// Counts the bytes read through `inner`.
    pub(crate) fn reader<R>(self: &Arc<Self>, inner: R) -> Counting<R> {
        Counting {
            inner,
            progress: Arc::clone(self),
        }
    }

    /// Starts a read of the rows from its first byte.
    pub(crate) fn phase(&self, phase: Phase) {
        let bit = if phase == Phase::Write {
            WRITE_PHASE
        } else {
            0
        };
        self.state.store(bit, Ordering::Relaxed);
    }

    /// The phase and the bytes read in it.
    fn read(&self) -> (Phase, u64) {
        let state = self.state.load(Ordering::Relaxed);
        let phase = if state & WRITE_PHASE == 0 {
            Phase::Scan
        } else {
            Phase::Write
        };
        (phase, state & !WRITE_PHASE)
    }
}

/// A reader that adds the bytes it reads to the run's progress.
pub(crate) struct Counting<R> {
    inner: R,
    progress: Arc<Progress>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buffer)?;
        // Below 2^63 bytes the count never reaches the phase bit.
        self.progress
            .state
            .fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}

enum Message {
    Terminal(Terminal),
}

/// The telemetry of one run. Inert when telemetry is off.
pub(crate) struct Telemetry {
    progress: Arc<Progress>,
    sender: Option<Sender<Message>>,
    finished: Option<Receiver<()>>,
}

/// What the emitter thread needs to know about the run.
struct Setup {
    runtime_dir: PathBuf,
    run_id: String,
    total: Option<u64>,
    started_at: String,
    /// The paths the conversion reads or writes; telemetry never creates
    /// anything that could be one of them.
    avoid: Vec<PathBuf>,
}

impl Telemetry {
    /// A run without telemetry.
    pub(crate) fn off() -> Self {
        Self {
            progress: Arc::default(),
            sender: None,
            finished: None,
        }
    }

    /// Starts telemetry for `run_id` under `runtime_dir`. `total` is the
    /// length of the rows file when it is a regular file.
    /// `started_at` is when the program started, as close as it can tell.
    /// `avoid` lists the paths the conversion reads or writes.
    pub(crate) fn start(
        runtime_dir: Option<&Path>,
        run_id: &str,
        total: Option<u64>,
        started_at: SystemTime,
        avoid: Vec<PathBuf>,
    ) -> Self {
        let Some(runtime_dir) = runtime_dir else {
            return Self::off();
        };
        let Ok(runtime_dir) = std::path::absolute(runtime_dir) else {
            return Self::off();
        };
        let setup = Setup {
            runtime_dir,
            run_id: run_id.to_owned(),
            total,
            started_at: timestamp(started_at),
            avoid,
        };
        let progress = Arc::new(Progress::default());
        let (sender, receiver) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let shared = Arc::clone(&progress);
        let spawned = thread::Builder::new()
            .name("telemetry".into())
            .spawn(move || {
                emitter(&setup, &shared, &receiver);
                let _ = done.send(());
            });
        if spawned.is_err() {
            return Self::off();
        }
        Self {
            progress,
            sender: Some(sender),
            finished: Some(finished),
        }
    }

    /// The counters the conversion updates.
    pub(crate) fn progress(&self) -> &Arc<Progress> {
        &self.progress
    }

    /// Emits the terminal event and waits, for a bounded time, for the
    /// emitter to finish.
    pub(crate) fn finish(self, terminal: Terminal) {
        if let (Some(sender), Some(finished)) = (self.sender, self.finished) {
            if sender.send(Message::Terminal(terminal)).is_ok() {
                let _ = finished.recv_timeout(FINISH_WAIT);
            }
        }
    }
}

/// The emitter thread: sets up the files, emits `started`, then progress
/// and heartbeats until the terminal event, then removes the card. Any
/// failure ends telemetry for the run.
fn emitter(setup: &Setup, progress: &Progress, messages: &Receiver<Message>) {
    let Some((mut stream, card)) = open(setup) else {
        // Telemetry is off; the terminal message, when it comes, is dropped.
        let _ = messages.recv();
        return;
    };
    let mut last_done = u64::MAX;
    let mut last_event = Instant::now();
    loop {
        match messages.recv_timeout(PROGRESS_INTERVAL) {
            Ok(Message::Terminal(terminal)) => {
                let (event, severity, data) = terminal_event(terminal);
                stream.emit(event, severity, data, false);
                break;
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let (phase, done) = progress.read();
        let phase = phase.name();
        if done != last_done {
            let mut data = Map::new();
            data.insert("phase".into(), json!(phase));
            data.insert("done".into(), json!(done));
            if let Some(total) = setup.total {
                data.insert("total".into(), json!(total));
            }
            data.insert("unit".into(), json!("bytes"));
            stream.emit("progress", "INFO", Value::Object(data), false);
            last_done = done;
            last_event = Instant::now();
        } else if last_event.elapsed() >= HEARTBEAT_INTERVAL {
            stream.emit(
                "heartbeat",
                "INFO",
                json!({"phase": phase, "done": done}),
                false,
            );
            last_event = Instant::now();
        }
        if stream.file.is_none() {
            break;
        }
    }
    // The card is removed whether or not the stream is still writable.
    let _ = fs::remove_file(&card);
    if let Some(directory) = card.parent() {
        let _ = fs::remove_dir(directory);
    }
}

fn terminal_event(terminal: Terminal) -> (&'static str, &'static str, Value) {
    match terminal {
        Terminal::Completed {
            rows,
            sha256,
            bytes,
        } => (
            "completed",
            "INFO",
            json!({"status": "ok", "rows": rows, "output": {"sha256": sha256, "bytes": bytes}}),
        ),
        Terminal::Canceled { signal } => (
            "canceled",
            "WARN",
            json!({"status": "cancelled", "reason": signal}),
        ),
        Terminal::Failed {
            status,
            category,
            rule,
        } => (
            "failed",
            "ERROR",
            json!({"status": status, "reason": format!("{category}/{rule}")}),
        ),
    }
}

/// The open event stream.
struct Stream {
    file: Option<File>,
    seq: u64,
    run_id: String,
    pid: u32,
}

impl Stream {
    /// Appends one event. A failed write ends the stream.
    fn emit(&mut self, event: &str, severity: &str, data: Value, producer: bool) {
        let Some(file) = &mut self.file else {
            return;
        };
        let mut line = Map::new();
        line.insert("ts".into(), json!(timestamp(SystemTime::now())));
        line.insert("event".into(), json!(event));
        line.insert("run_id".into(), json!(self.run_id));
        line.insert("seq".into(), json!(self.seq));
        line.insert("severity".into(), json!(severity));
        line.insert("pid".into(), json!(self.pid));
        if producer {
            line.insert("producer".into(), producer_json());
        }
        line.insert("data".into(), data);
        let text = format!("{}\n", Value::Object(line));
        if file.write_all(text.as_bytes()).is_err() {
            self.file = None;
        }
        self.seq += 1;
    }
}

fn producer_json() -> Value {
    json!({
        "name": identity::BINARY_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "profile": PROFILE,
    })
}

/// The card for a run, as bytes.
fn card_bytes(setup: &Setup, events: &Path) -> Option<Vec<u8>> {
    let card = json!({
        "capabilities": [CAPABILITY],
        "run_id": setup.run_id,
        "pid": std::process::id(),
        "started_at": setup.started_at,
        "producer": producer_json(),
        "telemetry": {"path": events.to_str()?, "format": "ndjson"},
    });
    let mut bytes = serde_json::to_vec_pretty(&card).ok()?;
    bytes.push(b'\n');
    Some(bytes)
}

/// The card for a run, once it passes [`check_card`]; `None` otherwise, and
/// then no card is published.
fn card_for(setup: &Setup, events: &Path) -> Option<Vec<u8>> {
    let bytes = card_bytes(setup, events)?;
    check_card(&bytes).then_some(bytes)
}

/// Checks the card, creates the directories and the stream, emits
/// `started`, then publishes the card, so a published card always points at
/// a stream that begins with `started`. On any failure nothing this run
/// created is left behind. Returns the stream and the card's path.
fn open(setup: &Setup) -> Option<(Stream, PathBuf)> {
    let proc_dir = setup.runtime_dir.join("proc");
    let events_dir = setup.runtime_dir.join("events");
    let events = events_dir.join(format!("{}.ndjson", setup.run_id));
    let card_dir = proc_dir.join(&setup.run_id);
    let card = card_dir.join("card.json");
    let staging = card_dir.join(".card.json.tmp");
    // The conversion's paths come first: telemetry is off if anything it
    // would create could be one of them, or could contain one.
    if collides(
        &setup.avoid,
        &[&setup.runtime_dir, &proc_dir, &events_dir, &card_dir],
        &[&events, &card, &staging],
    ) {
        return None;
    }
    // The serialized card is checked before anything is created.
    let bytes = card_for(setup, &events)?;
    for directory in [&setup.runtime_dir, &proc_dir, &events_dir] {
        create_dir(directory, true)?;
    }
    create_dir(&card_dir, false)?;
    let Some(file) = create_file(&events) else {
        let _ = fs::remove_dir(&card_dir);
        return None;
    };
    let mut stream = Stream {
        file: Some(file),
        seq: 0,
        run_id: setup.run_id.clone(),
        pid: std::process::id(),
    };
    let mut data = Map::new();
    data.insert("profile_id".into(), json!(tableprims_xlsx::PROFILE_ID));
    if let Some(total) = setup.total {
        data.insert("source_bytes".into(), json!(total));
    }
    stream.emit("started", "INFO", Value::Object(data), true);
    let published = stream.file.is_some()
        && create_file(&staging).is_some_and(|mut staged| staged.write_all(&bytes).is_ok())
        && fs::rename(&staging, &card).is_ok();
    if !published {
        let _ = fs::remove_file(&staging);
        let _ = fs::remove_dir(&card_dir);
        // The stream holds only `started`, from this run; it is removed.
        drop(stream);
        let _ = fs::remove_file(&events);
        return None;
    }
    Some((stream, card))
}

/// A path as lowercase components, for comparison: the longest part that
/// exists resolved through symbolic links, the rest as written. Lowercase
/// so that names a case-insensitive file system would treat as one compare
/// equal; that can only find more collisions, never fewer.
fn comparison_key(path: &Path) -> Option<Vec<String>> {
    let path = std::path::absolute(path).ok()?;
    let mut existing = PathBuf::new();
    let mut rest = Vec::new();
    for component in path.components() {
        if rest.is_empty() && existing.join(component).exists() {
            existing.push(component);
        } else {
            rest.push(component);
        }
    }
    let base = fs::canonicalize(&existing).unwrap_or(existing);
    let mut key: Vec<String> = base
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    for component in rest {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                key.pop();
            }
            other => key.push(other.as_os_str().to_string_lossy().to_lowercase()),
        }
    }
    Some(key)
}

/// Whether telemetry's planned entries could collide with the conversion:
/// a planned directory or file that could be one of `avoid`, or a directory
/// telemetry would create that would contain one of them. A path that
/// cannot be compared counts as a collision.
fn collides(avoid: &[PathBuf], directories: &[&PathBuf], files: &[&PathBuf]) -> bool {
    let Some(avoid) = avoid
        .iter()
        .map(|path| comparison_key(path))
        .collect::<Option<Vec<_>>>()
    else {
        return true;
    };
    for planned in directories.iter().chain(files) {
        let Some(key) = comparison_key(planned) else {
            return true;
        };
        if avoid.contains(&key) {
            return true;
        }
    }
    directories.iter().any(|directory| {
        !directory.exists()
            && comparison_key(directory)
                .is_none_or(|key| avoid.iter().any(|path| path.starts_with(&key)))
    })
}

/// Creates a directory owner-only. An existing one is accepted only where
/// `existing` allows it.
fn create_dir(path: &Path, existing: bool) -> Option<()> {
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.mode(0o700);
        builder
    };
    #[cfg(not(unix))]
    let builder = fs::DirBuilder::new();
    match builder.create(path) {
        Ok(()) => Some(()),
        Err(error) if existing && error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {
            Some(())
        }
        Err(_) => None,
    }
}

/// Creates a new file, owner read-write; an existing file is an error.
fn create_file(path: &Path) -> Option<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).ok()
}

/// Checks a serialized telemetry-only card against the closed subset of the
/// process-run card schema this program writes: exactly the members below,
/// the capability token, a positive pid, a UTC timestamp, a producer with a
/// name and a version, and an absolute stream path in NDJSON.
pub(crate) fn check_card(bytes: &[u8]) -> bool {
    let Ok(Value::Object(card)) = serde_json::from_slice::<Value>(bytes) else {
        return false;
    };
    let nonempty =
        |value: Option<&Value>| value.and_then(Value::as_str).is_some_and(|s| !s.is_empty());
    let members = [
        "capabilities",
        "run_id",
        "pid",
        "started_at",
        "producer",
        "telemetry",
    ];
    if card.len() != members.len() || !members.iter().all(|m| card.contains_key(*m)) {
        return false;
    }
    let Some(capabilities) = card["capabilities"].as_array() else {
        return false;
    };
    let mut seen = std::collections::BTreeSet::new();
    if capabilities.is_empty()
        || !capabilities
            .iter()
            .all(|c| c.as_str().is_some_and(|s| seen.insert(s.to_owned())))
        || !seen.contains(CAPABILITY)
    {
        return false;
    }
    if !nonempty(card.get("run_id")) || card["pid"].as_u64().is_none_or(|pid| pid == 0) {
        return false;
    }
    if !card["started_at"].as_str().is_some_and(is_utc_timestamp) {
        return false;
    }
    let Some(producer) = card["producer"].as_object() else {
        return false;
    };
    if !producer
        .keys()
        .all(|k| ["name", "version", "profile"].contains(&k.as_str()))
        || !nonempty(producer.get("name"))
        || !nonempty(producer.get("version"))
        || producer.get("profile").is_some_and(|p| !nonempty(Some(p)))
    {
        return false;
    }
    let Some(telemetry) = card["telemetry"].as_object() else {
        return false;
    };
    telemetry.len() == 2
        && telemetry
            .get("path")
            .and_then(Value::as_str)
            .is_some_and(|path| !path.is_empty() && Path::new(path).is_absolute())
        && telemetry.get("format").and_then(Value::as_str) == Some("ndjson")
}

/// `YYYY-MM-DDThh:mm:ss.sssZ`, the form this program writes, naming a real
/// date and time: a month of 1 to 12, a day that month has (leap years by
/// the Gregorian rule), hours 0 to 23, minutes and seconds 0 to 59.
fn is_utc_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 24 || !text.is_ascii() {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let field = &text[range];
        field
            .bytes()
            .all(|b| b.is_ascii_digit())
            .then(|| field.parse().ok())
            .flatten()
    };
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'.'),
        (23, b'Z'),
    ];
    if !separators.iter().all(|&(at, byte)| bytes[at] == byte) {
        return false;
    }
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second), Some(_)) = (
        number(0..4),
        number(5..7),
        number(8..10),
        number(11..13),
        number(14..16),
        number(17..19),
        number(20..23),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day) && hour < 24 && minute < 60 && second < 60
}

/// A UTC time as `YYYY-MM-DDThh:mm:ss.sssZ`.
pub(crate) fn timestamp(time: SystemTime) -> String {
    let since = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let seconds = since.as_secs();
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01.
fn civil(days: u64) -> (u64, u64, u64) {
    // Howard Hinnant's days-to-civil algorithm, for non-negative days.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Whether `run_id` can name this run's telemetry files: not ending in `.`
/// (which covers `.` and `..`), and not a name Windows reserves for a device (`CON`, `PRN`, `AUX`,
/// `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, in any case, with or without
/// an extension), which is refused on every platform.
pub(crate) fn usable_run_id(run_id: &str) -> bool {
    // `.` and `..` name directories, and Windows drops a trailing dot, so
    // `a.` would name the same file as `a`.
    if run_id.ends_with('.') {
        return false;
    }
    let stem = run_id
        .split('.')
        .next()
        .unwrap_or(run_id)
        .to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    !device
}

/// The runtime directory: the flag, else the environment variable when it
/// is set and not blank, else none.
pub(crate) fn runtime_dir(flag: Option<PathBuf>) -> Option<PathBuf> {
    flag.or_else(|| {
        let value = std::env::var_os(RUNTIME_DIR_ENV)?;
        let blank = value.to_str().is_some_and(|text| text.trim().is_empty());
        (!value.is_empty() && !blank).then(|| PathBuf::from(value))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vendored(name: &str) -> Value {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../third_party/3leaps-crucible/v0.2.12/schemas/process-run/v0")
            .join(name);
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    fn schema_accepts(card: &Value) -> bool {
        let validator = jsonschema::draft202012::options()
            .should_validate_formats(true)
            .build(&vendored("process-card.schema.json"))
            .unwrap();
        validator.is_valid(card)
    }

    fn setup() -> Setup {
        Setup {
            runtime_dir: std::env::temp_dir(),
            run_id: "case-0001".into(),
            total: Some(10),
            started_at: timestamp(SystemTime::now()),
            avoid: Vec::new(),
        }
    }

    #[test]
    fn telemetry_entries_never_take_a_conversion_path() {
        let root = std::env::temp_dir().join(format!("tblp-collide-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("events")).unwrap();
        let events = root.join("events/demo.ndjson");
        let dirs = [
            root.clone(),
            root.join("proc"),
            root.join("events"),
            root.join("proc/demo"),
        ];
        let dirs: Vec<&PathBuf> = dirs.iter().collect();
        let files = [events.clone(), root.join("proc/demo/card.json")];
        let files: Vec<&PathBuf> = files.iter().collect();
        let collide = |path: PathBuf| collides(&[path], &dirs, &files);
        // The same file, spelled differently.
        assert!(collide(events.clone()));
        assert!(collide(root.join("events/./demo.ndjson")));
        assert!(collide(root.join("proc/../events/demo.ndjson")));
        assert!(collide(root.join("EVENTS/Demo.NDJSON")));
        // A directory telemetry would create, or something inside it.
        assert!(collide(root.join("proc")));
        assert!(collide(root.join("proc/demo/out.xlsx")));
        // Elsewhere, including beside an existing directory.
        assert!(!collide(root.join("events/other.ndjson")));
        assert!(!collide(root.join("out.xlsx")));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("events"), root.join("link")).unwrap();
            assert!(collide(root.join("link/demo.ndjson")));
        }
        fs::remove_dir_all(&root).unwrap();
    }

    fn written() -> Value {
        let events = std::env::temp_dir().join("events/case-0001.ndjson");
        serde_json::from_slice(&card_bytes(&setup(), &events).unwrap()).unwrap()
    }

    /// Expected texts were worked out with Python's `datetime` in UTC.
    #[test]
    fn timestamps_are_utc_with_milliseconds() {
        let at = |seconds: u64, millis: u64| {
            timestamp(UNIX_EPOCH + Duration::from_millis(seconds * 1000 + millis))
        };
        assert_eq!(at(0, 0), "1970-01-01T00:00:00.000Z");
        assert_eq!(at(951_782_400, 5), "2000-02-29T00:00:00.005Z");
        assert_eq!(at(1_795_200_000, 999), "2026-11-20T18:40:00.999Z");
        assert_eq!(at(253_402_300_799, 0), "9999-12-31T23:59:59.000Z");
        assert!(is_utc_timestamp(&timestamp(SystemTime::now())));
        for valid in [
            "2024-02-29T00:00:00.000Z",
            "2000-02-29T23:59:59.999Z",
            "2026-12-31T12:30:45.001Z",
        ] {
            assert!(is_utc_timestamp(valid), "{valid}");
        }
        for invalid in [
            "2026-02-29T00:00:00.000Z",
            "1900-02-29T00:00:00.000Z",
            "2026-02-31T00:00:00.000Z",
            "2026-04-31T00:00:00.000Z",
            "2026-13-01T00:00:00.000Z",
            "2026-00-10T00:00:00.000Z",
            "2026-01-00T00:00:00.000Z",
            "2026-01-01T24:00:00.000Z",
            "2026-01-01T23:60:00.000Z",
            "2026-01-01T23:59:60.000Z",
            "2026-01-01T23:59:59.000+00:00",
            "2026-01-01 23:59:59.000Z",
        ] {
            assert!(!is_utc_timestamp(invalid), "{invalid}");
        }
    }

    /// The card this program writes is accepted by its own check and by an
    /// independent validator of the vendored entry schema.
    #[test]
    fn the_written_card_passes_both_checks() {
        let card = written();
        assert!(check_card(card.to_string().as_bytes()));
        assert!(schema_accepts(&card));
    }

    /// Mutations the schema refuses are refused by the check too.
    #[test]
    fn invalid_cards_fail_both_checks() {
        type Mutation = Box<dyn Fn(&mut Value)>;
        let mutations: Vec<Mutation> = vec![
            Box::new(|c| c["capabilities"] = json!([])),
            Box::new(|c| c["capabilities"] = json!(["contract: process-run/v1"])),
            Box::new(|c| c["capabilities"] = json!([CAPABILITY, CAPABILITY])),
            Box::new(|c| c["capabilities"] = json!([CAPABILITY, 7])),
            Box::new(|c| c["run_id"] = json!("")),
            Box::new(|c| c["pid"] = json!(0)),
            Box::new(|c| c["pid"] = json!("12")),
            Box::new(|c| c["started_at"] = json!("yesterday")),
            Box::new(|c| c["started_at"] = json!("2026-02-31T10:00:00.000Z")),
            Box::new(|c| c["started_at"] = json!("2026-02-28T24:61:61.000Z")),
            Box::new(|c| c["producer"] = json!({"name": "tablebolt"})),
            Box::new(|c| c["producer"]["extra"] = json!("x")),
            Box::new(|c| c["producer"]["profile"] = json!("")),
            Box::new(|c| c["telemetry"]["format"] = json!("json")),
            Box::new(|c| c["telemetry"]["path"] = json!("")),
            Box::new(|c| c["telemetry"]["extra"] = json!(1)),
            Box::new(|c| c["extra"] = json!(true)),
            Box::new(|c| {
                c.as_object_mut().unwrap().remove("telemetry");
            }),
            Box::new(|c| {
                c.as_object_mut().unwrap().remove("producer");
            }),
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut card = written();
            mutate(&mut card);
            assert!(!schema_accepts(&card), "schema accepted mutation {index}");
            assert!(
                !check_card(card.to_string().as_bytes()),
                "check accepted mutation {index}"
            );
        }
    }

    /// What the check refuses beyond the schema: a relative stream path and
    /// a timestamp that is not this program's UTC form, which the standard
    /// requires in prose; and a control channel, which this program does not
    /// offer.
    #[test]
    fn the_check_is_stricter_than_the_schema_where_the_prose_is() {
        for mutate in [
            |c: &mut Value| c["telemetry"]["path"] = json!("events/case-0001.ndjson"),
            |c: &mut Value| c["started_at"] = json!("2026-11-20T15:20:00+01:00"),
        ] {
            let mut card = written();
            mutate(&mut card);
            assert!(!check_card(card.to_string().as_bytes()));
        }
        let golden = vendored("examples/process-card.example.json");
        assert!(schema_accepts(&golden));
        assert!(!check_card(golden.to_string().as_bytes()));
        let mut telemetry_only = golden;
        telemetry_only.as_object_mut().unwrap().remove("control");
        assert!(check_card(telemetry_only.to_string().as_bytes()));
        assert!(!check_card(b"not json"));
    }

    #[test]
    fn run_ids_that_cannot_name_files_are_refused() {
        for bad in [
            ".",
            "..",
            "...",
            "a.",
            "CON",
            "con",
            "Nul",
            "aux.txt",
            "prn.tar.gz",
            "COM1",
            "com9.x",
            "LPT1",
            "lpt9",
        ] {
            assert!(!usable_run_id(bad), "{bad}");
        }
        for good in [
            ".a",
            "case-0001",
            "CONSOLE",
            "COM0",
            "COM10",
            "LPT",
            "auto-0123",
            "a.con",
            "NULL",
        ] {
            assert!(usable_run_id(good), "{good}");
        }
    }

    /// A card that fails the check is not produced for publication: here
    /// a stream path that is not absolute.
    #[test]
    fn only_a_checked_card_is_published() {
        let absolute = std::env::temp_dir().join("events/case-0001.ndjson");
        assert!(card_for(&setup(), &absolute).is_some());
        assert!(card_for(&setup(), Path::new("events/case-0001.ndjson")).is_none());
    }

    #[test]
    fn progress_reads_the_phase_and_bytes_together() {
        let progress = Arc::new(Progress::default());
        let mut sink = Vec::new();
        progress
            .reader(&b"0123456789"[..])
            .read_to_end(&mut sink)
            .unwrap();
        assert_eq!(progress.read(), (Phase::Scan, 10));
        progress.phase(Phase::Write);
        assert_eq!(progress.read(), (Phase::Write, 0));
        progress.reader(&b"abc"[..]).read_to_end(&mut sink).unwrap();
        assert_eq!(progress.read(), (Phase::Write, 3));
    }
}
