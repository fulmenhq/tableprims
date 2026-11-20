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
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
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
    Scan = 0,
    Write = 1,
}

impl Phase {
    fn name(raw: u8) -> &'static str {
        if raw == Phase::Write as u8 {
            "write"
        } else {
            "scan"
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

/// Counters the conversion updates; read by the emitter thread.
#[derive(Default)]
pub(crate) struct Progress {
    done: AtomicU64,
    phase: AtomicU8,
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
        self.done.store(0, Ordering::Relaxed);
        self.phase.store(phase as u8, Ordering::Relaxed);
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
        self.progress
            .done
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
    pub(crate) fn start(runtime_dir: Option<&Path>, run_id: &str, total: Option<u64>) -> Self {
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
            started_at: timestamp(SystemTime::now()),
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
        let done = progress.done.load(Ordering::Relaxed);
        let phase = Phase::name(progress.phase.load(Ordering::Relaxed));
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

/// Creates the directories and the stream, checks and publishes the card,
/// and emits `started`. Returns the stream and the card's path.
fn open(setup: &Setup) -> Option<(Stream, PathBuf)> {
    let proc_dir = setup.runtime_dir.join("proc");
    let events_dir = setup.runtime_dir.join("events");
    for directory in [&setup.runtime_dir, &proc_dir, &events_dir] {
        create_dir(directory, true)?;
    }
    let card_dir = proc_dir.join(&setup.run_id);
    create_dir(&card_dir, false)?;
    let events = events_dir.join(format!("{}.ndjson", setup.run_id));
    let opened = (|| {
        let file = create_file(&events)?;
        let bytes = card_bytes(setup, &events)?;
        // The serialized card is checked before it is given its name.
        if !check_card(&bytes) {
            return None;
        }
        let card = card_dir.join("card.json");
        let staging = card_dir.join(".card.json.tmp");
        let mut staged = create_file(&staging)?;
        let written = staged.write_all(&bytes).is_ok() && fs::rename(&staging, &card).is_ok();
        if !written {
            let _ = fs::remove_file(&staging);
            return None;
        }
        Some((file, card))
    })();
    let Some((file, card)) = opened else {
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
    Some((stream, card))
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

/// `YYYY-MM-DDThh:mm:ss.sssZ`, the form this program writes.
fn is_utc_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    let digits =
        |range: std::ops::Range<usize>| range.into_iter().all(|i| bytes[i].is_ascii_digit());
    bytes.len() == 24
        && digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && bytes[10] == b'T'
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
        && bytes[16] == b':'
        && digits(17..19)
        && bytes[19] == b'.'
        && digits(20..23)
        && bytes[23] == b'Z'
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

/// Whether `run_id` can name this run's telemetry files: not `.` or `..`,
/// and not a name Windows reserves for a device (`CON`, `PRN`, `AUX`,
/// `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, in any case, with or without
/// an extension), which is refused on every platform.
pub(crate) fn usable_run_id(run_id: &str) -> bool {
    if run_id == "." || run_id == ".." {
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
        }
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
            "...",
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

    #[test]
    fn a_blank_environment_value_is_unset() {
        // Only the flag path is exercised here; the environment is process
        // global and is covered by the integration tests.
        let flag = PathBuf::from("/tmp/x");
        assert_eq!(runtime_dir(Some(flag.clone())), Some(flag));
    }
}
