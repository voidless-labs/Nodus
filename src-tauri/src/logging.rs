//! File logging with rotation (t35).
//!
//! The log used to be a single file that grew forever across runs — 95 MB and half
//! a million lines by September, too big to open in Notepad, which is exactly the
//! moment a log is needed. Now:
//!
//! * files are named by the day they were started and numbered within it:
//!   `nodus.2026-09-10-n1.log`, `nodus.2026-09-10-n2.log`, …;
//! * a file is cut at [`MAX_FILE_BYTES`] and at midnight, so any file opens in a plain
//!   text editor and one date's file holds only that date;
//! * the first line of every file states the span it covers and how it ended —
//!   closed properly, or cut short by a crash or power-off. It lives in space
//!   reserved when the file is created, so closing a file only overwrites those
//!   bytes: nothing in a large file ever has to move;
//! * a file left "still being written" by a run that never closed it is stamped on
//!   the next launch with the time of its last entry — the moment things stopped;
//! * old files are pruned by count or by folder size, from the app settings, and the
//!   file being written is never deleted;
//! * times are the computer's local clock, exactly as the system shows them.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use tracing::info;

/// A log file is cut before it grows past this, so it always opens in Notepad.
pub const MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
/// Default for "keep the newest N files".
pub const DEFAULT_MAX_FILES: u32 = 5;
/// Default for "keep the folder under N MB".
pub const DEFAULT_MAX_MB: u32 = 50;
/// A size limit below one full file would delete everything but the current file.
pub const MIN_MAX_MB: u32 = 10;

/// Bytes reserved for the first line, newline included. Every header variant is
/// padded to exactly this, which is what lets a header be rewritten in place.
const HEADER_BYTES: usize = 256;
/// How far back from the end to look for the last entry of a file cut short.
const TAIL_BYTES: u64 = 64 * 1024;

const MARK_WRITING: &str = "ещё пишется";
const MARK_CLOSED: &str = "закрыт";
const MARK_CUT_SHORT: &str =
    "оборван (Nodus не успел закрыть лог: сбой или выключение компьютера)";

/// How old log files are pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retention {
    /// Keep the newest N files, the one being written included.
    Files(u32),
    /// Keep the whole folder under N megabytes.
    Megabytes(u32),
}

// ── Local time ──────────────────────────────────────────────────────────────

/// A wall-clock time on the computer's local clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millis: u16,
}

impl Stamp {
    /// Now, as the operating system reports local time.
    pub fn now() -> Self {
        local_now()
    }

    fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    fn hms(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    fn same_day(&self, other: &Stamp) -> bool {
        (self.year, self.month, self.day) == (other.year, other.month, other.day)
    }

    /// Rebuild a stamp from a file's date (`YYYY-MM-DD`) and a header time (`HH:MM:SS`).
    fn from_parts(date: &str, hms: &str) -> Option<Self> {
        let d = date.as_bytes();
        let t = hms.as_bytes();
        if d.len() != 10 || t.len() != 8 {
            return None;
        }
        Some(Self {
            year: date.get(0..4)?.parse().ok()?,
            month: date.get(5..7)?.parse().ok()?,
            day: date.get(8..10)?.parse().ok()?,
            hour: hms.get(0..2)?.parse().ok()?,
            minute: hms.get(3..5)?.parse().ok()?,
            second: hms.get(6..8)?.parse().ok()?,
            millis: 0,
        })
    }
}

#[cfg(target_os = "windows")]
fn local_now() -> Stamp {
    // SAFETY: GetLocalTime only fills a plain value struct; it cannot fail.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Stamp {
        year: t.wYear,
        month: t.wMonth as u8,
        day: t.wDay as u8,
        hour: t.wHour as u8,
        minute: t.wMinute as u8,
        second: t.wSecond as u8,
        millis: t.wMilliseconds,
    }
}

/// Off Windows there is no local-time API in std; UTC is the honest fallback.
#[cfg(not(target_os = "windows"))]
fn local_now() -> Stamp {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let sod = secs % 86_400;
    Stamp {
        year,
        month,
        day,
        hour: (sod / 3600) as u8,
        minute: (sod / 60 % 60) as u8,
        second: (sod % 60) as u8,
        millis: since.subsec_millis() as u16,
    }
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
#[cfg(not(target_os = "windows"))]
fn civil_from_days(z: i64) -> (u16, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as u16;
    (year, month, day)
}

/// Timestamps log lines with the local clock: `2026-09-10 19:03:12.345`. The same
/// shape as the date in file names, and what the crash repair reads back.
pub struct LocalTimer;

impl tracing_subscriber::fmt::time::FormatTime for LocalTimer {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let t = Stamp::now();
        write!(
            w,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            t.year, t.month, t.day, t.hour, t.minute, t.second, t.millis
        )
    }
}

// ── Files, names, headers ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Writing,
    Closed,
    CutShort,
}

fn file_name(date: &str, n: u32) -> String {
    format!("nodus.{date}-n{n}.log")
}

/// `nodus.2026-09-10-n3.log` → `("2026-09-10", 3)`. Anything else — the old single
/// `nodus.log`, stray files — is not ours to rotate or delete.
fn parse_file_name(name: &str) -> Option<(String, u32)> {
    let core = name.strip_prefix("nodus.")?.strip_suffix(".log")?;
    let (date, n) = core.rsplit_once("-n")?;
    let date_ok = date.len() == 10
        && date.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    if !date_ok || n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = n.parse().ok()?;
    (n > 0).then(|| (date.to_string(), n))
}

#[derive(Debug, Clone)]
struct LogFile {
    path: PathBuf,
    date: String,
    n: u32,
    bytes: u64,
}

/// Our log files in `dir`, oldest first.
fn list_logs(dir: &Path) -> Vec<LogFile> {
    let mut files: Vec<LogFile> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let (date, n) = parse_file_name(name.to_str()?)?;
            let bytes = entry.metadata().ok()?.len();
            Some(LogFile { path: entry.path(), date, n, bytes })
        })
        .collect();
    files.sort_by(|a, b| (a.date.as_str(), a.n).cmp(&(b.date.as_str(), b.n)));
    files
}

fn next_index(dir: &Path, date: &str) -> u32 {
    list_logs(dir)
        .iter()
        .filter(|f| f.date == date)
        .map(|f| f.n)
        .max()
        .unwrap_or(0)
        + 1
}

fn render_header(version: &str, date: &str, n: u32, start: &str, end: &str, status: Status) -> Vec<u8> {
    let head = format!("# Nodus {version} · лог n{n} за {date} · период: ");
    let mut text = match status {
        Status::Writing => format!("{head}с {start} · {MARK_WRITING}"),
        Status::Closed => format!("{head}{start} — {end} · {MARK_CLOSED}"),
        Status::CutShort => format!("{head}{start} — {end} · {MARK_CUT_SHORT}"),
    };
    // Never happens with a sane version string, but a header must never grow past
    // its reserved space — that would overwrite the first log entry.
    if text.len() > HEADER_BYTES - 1 {
        let mut cut = HEADER_BYTES - 1;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    let mut bytes = text.into_bytes();
    bytes.resize(HEADER_BYTES - 1, b' ');
    bytes.push(b'\n');
    bytes
}

fn header_status(header: &str) -> Option<Status> {
    // Order matters: the cut-short wording contains "закрыт" as part of "закрыть".
    if header.contains(MARK_WRITING) {
        Some(Status::Writing)
    } else if header.contains("оборван") {
        Some(Status::CutShort)
    } else if header.contains(MARK_CLOSED) {
        Some(Status::Closed)
    } else {
        None
    }
}

fn header_version(header: &str) -> Option<&str> {
    header.strip_prefix("# Nodus ")?.split(" · ").next()
}

/// The start time a header records, in either the open or the closed wording.
fn header_start(header: &str) -> Option<String> {
    let after = header.split("период: ").nth(1)?;
    let after = after.strip_prefix("с ").unwrap_or(after);
    let hms = after.get(0..8)?;
    Stamp::from_parts("2000-01-01", hms).map(|_| hms.to_string())
}

fn read_header(file: &mut File) -> io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut buf = Vec::with_capacity(HEADER_BYTES);
    file.take(HEADER_BYTES as u64).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn write_header(file: &mut File, header: &[u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(header)?;
    file.flush()
}

/// `HH:MM:SS` of the last line that starts with a log timestamp.
fn last_entry_time(file: &mut File) -> io::Result<Option<String>> {
    let len = file.metadata()?.len();
    let from = len.saturating_sub(TAIL_BYTES).max(HEADER_BYTES as u64);
    if from >= len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(from))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    Ok(tail.split(|b| *b == b'\n').rev().find_map(entry_time))
}

/// `2026-09-10 19:03:12…` at the start of a line → `19:03:12`. Continuation lines
/// (a multi-line message, a backtrace) carry no timestamp and are skipped.
fn entry_time(line: &[u8]) -> Option<String> {
    let l = line.get(0..19)?;
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let shape = l[4] == b'-' && l[7] == b'-' && l[10] == b' ' && l[13] == b':' && l[16] == b':';
    (shape && digits.iter().all(|&i| l[i].is_ascii_digit()))
        .then(|| String::from_utf8_lossy(&l[11..19]).into_owned())
}

/// A file still marked "being written" at launch belongs to a run that never closed
/// it: Nodus crashed or the computer went down. Stamp it with its last entry's time.
fn repair_unfinished(dir: &Path) {
    for f in list_logs(dir) {
        let _ = repair_file(&f);
    }
}

fn repair_file(f: &LogFile) -> io::Result<()> {
    let mut file = OpenOptions::new().read(true).write(true).open(&f.path)?;
    let header = read_header(&mut file)?;
    if header_status(&header) != Some(Status::Writing) {
        return Ok(());
    }
    let version = header_version(&header).unwrap_or("?").to_string();
    let start = header_start(&header).unwrap_or_else(|| "??:??:??".to_string());
    let end = last_entry_time(&mut file)?.unwrap_or_else(|| start.clone());
    write_header(
        &mut file,
        &render_header(&version, &f.date, f.n, &start, &end, Status::CutShort),
    )
}

/// Delete the oldest of our files until the limit holds. The current file is never
/// a candidate, and files that are not ours (the old `nodus.log`) are never touched.
fn prune_to(dir: &Path, current: &Path, max_files: Option<usize>, max_bytes: Option<u64>) {
    let mut others: Vec<LogFile> = list_logs(dir).into_iter().filter(|f| f.path != current).collect();
    if let Some(k) = max_files {
        let keep = k.max(1) - 1; // the current file takes one slot
        while others.len() > keep {
            let oldest = others.remove(0);
            let _ = fs::remove_file(&oldest.path);
        }
    }
    if let Some(limit) = max_bytes {
        let current_bytes = fs::metadata(current).map(|m| m.len()).unwrap_or(0);
        let mut total = current_bytes + others.iter().map(|f| f.bytes).sum::<u64>();
        while total > limit && !others.is_empty() {
            let oldest = others.remove(0);
            if fs::remove_file(&oldest.path).is_ok() {
                total -= oldest.bytes;
            }
        }
    }
}

fn prune(dir: &Path, current: &Path, retention: Retention) {
    match retention {
        Retention::Files(k) => prune_to(dir, current, Some(k as usize), None),
        Retention::Megabytes(mb) => {
            prune_to(dir, current, None, Some(u64::from(mb.max(MIN_MAX_MB)) * 1024 * 1024))
        }
    }
}

// ── The writer ──────────────────────────────────────────────────────────────

/// Where the writer reads its pruning policy: the app settings in production, a
/// fixed value in tests (the global would make parallel tests interfere).
enum RetentionSource {
    Settings,
    #[cfg_attr(not(test), allow(dead_code))]
    Fixed(Retention),
}

impl RetentionSource {
    fn get(&self) -> Retention {
        match self {
            RetentionSource::Settings => current_retention(),
            RetentionSource::Fixed(r) => *r,
        }
    }
}

type Clock = Box<dyn Fn() -> Stamp + Send>;

struct Inner {
    dir: PathBuf,
    version: String,
    clock: Clock,
    max_bytes: u64,
    retention: RetentionSource,
    file: Option<File>,
    path: PathBuf,
    n: u32,
    start: Stamp,
    last: Stamp,
    bytes: u64,
    finished: bool,
}

impl Inner {
    fn open_new(&mut self, now: Stamp, n: u32) -> io::Result<()> {
        let date = now.date();
        let path = self.dir.join(file_name(&date, n));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.write_all(&render_header(&self.version, &date, n, &now.hms(), "", Status::Writing))?;
        self.file = Some(file);
        self.path = path;
        self.n = n;
        self.start = now;
        self.last = now;
        self.bytes = HEADER_BYTES as u64;
        Ok(())
    }

    /// Carry on in today's last file when the previous run closed it properly and it
    /// has room — a restart should not burn one of the "keep N files" slots. A file
    /// cut short by a crash is left as evidence and never continued.
    fn try_continue(&mut self, f: &LogFile, now: Stamp) -> io::Result<bool> {
        let mut file = OpenOptions::new().read(true).write(true).open(&f.path)?;
        let header = read_header(&mut file)?;
        if header_status(&header) != Some(Status::Closed) {
            return Ok(false);
        }
        let Some(start_hms) = header_start(&header) else {
            return Ok(false);
        };
        let Some(start) = Stamp::from_parts(&f.date, &start_hms) else {
            return Ok(false);
        };
        write_header(
            &mut file,
            &render_header(&self.version, &f.date, f.n, &start_hms, "", Status::Writing),
        )?;
        self.bytes = file.seek(SeekFrom::End(0))?;
        self.file = Some(file);
        self.path = f.path.clone();
        self.n = f.n;
        self.start = start;
        self.last = now;
        Ok(true)
    }

    /// Stamp the header with the real span and let the file go.
    fn close(&mut self, status: Status) {
        if let Some(mut file) = self.file.take() {
            let header = render_header(
                &self.version,
                &self.start.date(),
                self.n,
                &self.start.hms(),
                &self.last.hms(),
                status,
            );
            let _ = write_header(&mut file, &header);
        }
    }

    fn rotate_if_needed(&mut self, now: Stamp, incoming: usize) -> io::Result<()> {
        let new_day = !now.same_day(&self.start);
        // A file holding only its header is never "full": one line larger than the
        // limit must not spin through empty files forever.
        let full = self.bytes > HEADER_BYTES as u64 && self.bytes + incoming as u64 > self.max_bytes;
        if self.file.is_some() && !new_day && !full {
            return Ok(());
        }
        self.close(Status::Closed);
        let n = next_index(&self.dir, &now.date());
        self.open_new(now, n)?;
        prune(&self.dir, &self.path, self.retention.get());
        Ok(())
    }
}

/// The rotating file writer. Cheap to clone: the non-blocking worker holds one
/// handle and the module keeps another, to close the file properly on exit.
#[derive(Clone)]
pub struct RollingWriter(Arc<Mutex<Inner>>);

impl RollingWriter {
    fn open(
        dir: PathBuf,
        version: String,
        clock: Clock,
        max_bytes: u64,
        retention: RetentionSource,
    ) -> io::Result<Self> {
        fs::create_dir_all(&dir)?;
        repair_unfinished(&dir);
        let now = clock();
        let mut inner = Inner {
            dir,
            version,
            clock,
            max_bytes,
            retention,
            file: None,
            path: PathBuf::new(),
            n: 0,
            start: now,
            last: now,
            bytes: 0,
            finished: false,
        };
        let today = now.date();
        let last_today = list_logs(&inner.dir).into_iter().filter(|f| f.date == today).last();
        let continued = match &last_today {
            Some(f) if f.bytes < max_bytes => inner.try_continue(f, now)?,
            _ => false,
        };
        if !continued {
            let n = next_index(&inner.dir, &today);
            inner.open_new(now, n)?;
        }
        prune(&inner.dir, &inner.path, inner.retention.get());
        Ok(Self(Arc::new(Mutex::new(inner))))
    }

    /// Close the current file as properly finished; later writes are ignored.
    fn close(&self) {
        let mut g = lock(&self.0);
        if !g.finished {
            g.close(Status::Closed);
            g.finished = true;
        }
    }
}

impl Write for RollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut g = lock(&self.0);
        if g.finished {
            return Ok(buf.len());
        }
        let now = (g.clock)();
        g.rotate_if_needed(now, buf.len())?;
        g.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "log file unavailable"))?
            .write_all(buf)?;
        g.bytes += buf.len() as u64;
        g.last = now;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        match lock(&self.0).file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ── Process-wide wiring ─────────────────────────────────────────────────────

static WRITER: OnceLock<RollingWriter> = OnceLock::new();
static GUARD: Mutex<Option<tracing_appender::non_blocking::WorkerGuard>> = Mutex::new(None);
/// Pruning policy packed as `mode << 32 | value` (0 = files, 1 = megabytes), so a
/// reader never sees a mode from one update with the value of another.
static RETENTION: AtomicU64 = AtomicU64::new(DEFAULT_MAX_FILES as u64);

fn current_retention() -> Retention {
    let packed = RETENTION.load(Ordering::Relaxed);
    let value = packed as u32;
    if packed >> 32 == 1 {
        Retention::Megabytes(value)
    } else {
        Retention::Files(value)
    }
}

/// `%APPDATA%\com.nodus.app\logs`, where the log files live.
pub fn log_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|appdata| Path::new(&appdata).join("com.nodus.app").join("logs"))
}

/// Set the pruning policy (from the settings) and apply it right away. The file
/// being written is never deleted.
pub fn set_retention(retention: Retention) {
    let packed = match retention {
        Retention::Files(n) => u64::from(n),
        Retention::Megabytes(mb) => (1u64 << 32) | u64::from(mb),
    };
    RETENTION.store(packed, Ordering::Relaxed);
    if let Some(writer) = WRITER.get() {
        let g = lock(&writer.0);
        if !g.finished {
            prune(&g.dir, &g.path, retention);
        }
    }
}

/// Install logging: stdout always, plus the rotating file log when `%APPDATA%` exists.
/// Default level `info,nodus=debug` so our own debug lines are kept without
/// third-party noise; override with `RUST_LOG`-style env filters.
pub fn init(version: &str) {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let make_filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,nodus=debug"));

    if let Some(dir) = log_dir() {
        let opened = RollingWriter::open(
            dir.clone(),
            version.to_string(),
            Box::new(Stamp::now),
            MAX_FILE_BYTES,
            RetentionSource::Settings,
        );
        match opened {
            Ok(writer) => {
                let (nb, guard) = tracing_appender::non_blocking(writer.clone());
                tracing_subscriber::registry()
                    .with(make_filter())
                    .with(fmt::layer().with_timer(LocalTimer))
                    .with(fmt::layer().with_ansi(false).with_timer(LocalTimer).with_writer(nb))
                    .init();
                let _ = WRITER.set(writer);
                *lock(&GUARD) = Some(guard);
                info!("file log in {}", dir.display());
                return;
            }
            Err(e) => eprintln!("nodus: file log unavailable ({e}); logging to stdout only"),
        }
    }
    tracing_subscriber::fmt()
        .with_timer(LocalTimer)
        .with_env_filter(make_filter())
        .init();
}

/// Close the log file properly. Call on a normal exit: Tauri ends the process with
/// `exit`, which skips destructors, so without this every normal shutdown would be
/// reported as a crash on the next launch.
pub fn finish() {
    // Let the background worker write out what it still holds before the header is
    // stamped — dropping the guard flushes and joins it.
    let guard = lock(&GUARD).take();
    drop(guard);
    if let Some(writer) = WRITER.get() {
        writer.close();
    }
}

/// Open the log folder in Explorer, so nobody has to type `%APPDATA%` paths by hand.
pub fn open_folder() -> Result<(), String> {
    let dir = log_dir().ok_or_else(|| "the log folder is unavailable on this system".to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    reveal(&dir)
}

#[cfg(target_os = "windows")]
fn reveal(dir: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(not(target_os = "windows"))]
fn reveal(_dir: &Path) -> Result<(), String> {
    Err("opening the log folder is only supported on Windows".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("nodus-log-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn at(date: &str, hms: &str) -> Stamp {
        Stamp::from_parts(date, hms).expect("valid stamp")
    }

    /// A clock the test moves by hand.
    fn clock(start: Stamp) -> (Arc<Mutex<Stamp>>, Clock) {
        let shared = Arc::new(Mutex::new(start));
        let reader = Arc::clone(&shared);
        (shared, Box::new(move || *lock(&reader)))
    }

    fn open(dir: &Path, c: Clock, max_bytes: u64, keep: Retention) -> RollingWriter {
        RollingWriter::open(dir.to_path_buf(), "0.6.0".into(), c, max_bytes, RetentionSource::Fixed(keep))
            .expect("open writer")
    }

    /// Write one log line stamped with the writer's current clock.
    fn line(w: &mut RollingWriter, now: &Arc<Mutex<Stamp>>, text: &str) {
        let t = *lock(now);
        let entry = format!("{} {}.000  INFO nodus: {text}\n", t.date(), t.hms());
        w.write_all(entry.as_bytes()).expect("write");
    }

    fn first_line(path: &Path) -> String {
        let text = fs::read_to_string(path).expect("read log");
        text.lines().next().unwrap_or_default().trim_end().to_string()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = list_logs(dir)
            .into_iter()
            .map(|f| f.path.file_name().unwrap_or_default().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn file_names_parse_only_our_scheme() {
        assert_eq!(parse_file_name("nodus.2026-09-10-n1.log"), Some(("2026-09-10".into(), 1)));
        assert_eq!(parse_file_name("nodus.2026-09-10-n12.log"), Some(("2026-09-10".into(), 12)));
        assert_eq!(parse_file_name("nodus.log"), None); // the old single file
        assert_eq!(parse_file_name("nodus.2026-09-10-n0.log"), None);
        assert_eq!(parse_file_name("nodus.2026-9-10-n1.log"), None);
        assert_eq!(parse_file_name("nodus.2026-09-10-n+1.log"), None);
        assert_eq!(parse_file_name("notes.txt"), None);
    }

    /// Every header variant is exactly the reserved size — the property that lets a
    /// header be rewritten without touching the first entry after it.
    #[test]
    fn every_header_fits_its_reserved_space_exactly() {
        for status in [Status::Writing, Status::Closed, Status::CutShort] {
            let h = render_header("0.6.0-rc.12", "2026-09-10", 999, "19:03:12", "23:59:58", status);
            assert_eq!(h.len(), HEADER_BYTES, "{status:?}");
            assert_eq!(h.last(), Some(&b'\n'));
            let text = String::from_utf8(h).expect("utf-8 header");
            assert_eq!(header_status(&text), Some(status), "{text}");
            assert_eq!(header_start(&text).as_deref(), Some("19:03:12"));
        }
    }

    #[test]
    fn a_new_file_is_dated_and_says_it_is_being_written() {
        let dir = TempDir::new("new");
        let (_now, c) = clock(at("2026-09-10", "19:03:12"));
        let _w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
        let path = dir.0.join("nodus.2026-09-10-n1.log");
        assert_eq!(
            first_line(&path),
            "# Nodus 0.6.0 · лог n1 за 2026-09-10 · период: с 19:03:12 · ещё пишется"
        );
    }

    /// Cut at the size limit: the finished file records its real span, the next
    /// number continues.
    #[test]
    fn size_cut_closes_the_file_with_its_real_span() {
        let dir = TempDir::new("size");
        let (now, c) = clock(at("2026-09-10", "19:03:12"));
        let mut w = open(&dir.0, c, HEADER_BYTES as u64 + 150, Retention::Files(5));
        line(&mut w, &now, "first");
        *lock(&now) = at("2026-09-10", "19:40:55");
        line(&mut w, &now, "second");
        *lock(&now) = at("2026-09-10", "20:15:00");
        line(&mut w, &now, "a line long enough to push the file over its tiny test limit ..........");
        assert_eq!(names(&dir.0), ["nodus.2026-09-10-n1.log", "nodus.2026-09-10-n2.log"]);
        assert_eq!(
            first_line(&dir.0.join("nodus.2026-09-10-n1.log")),
            "# Nodus 0.6.0 · лог n1 за 2026-09-10 · период: 19:03:12 — 19:40:55 · закрыт"
        );
        assert!(first_line(&dir.0.join("nodus.2026-09-10-n2.log")).contains("с 20:15:00 · ещё пишется"));
    }

    /// Midnight starts a new dated file, so one date's file holds only that date.
    #[test]
    fn midnight_starts_a_file_for_the_new_date() {
        let dir = TempDir::new("midnight");
        let (now, c) = clock(at("2026-09-10", "23:50:00"));
        let mut w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
        *lock(&now) = at("2026-09-10", "23:59:58");
        line(&mut w, &now, "late");
        *lock(&now) = at("2026-09-11", "00:00:03");
        line(&mut w, &now, "after midnight");
        assert_eq!(names(&dir.0), ["nodus.2026-09-10-n1.log", "nodus.2026-09-11-n1.log"]);
        assert!(first_line(&dir.0.join("nodus.2026-09-10-n1.log")).ends_with("23:50:00 — 23:59:58 · закрыт"));
    }

    /// A run that never closed its file (crash, power-off) is stamped on the next
    /// launch with the time of its last entry — and that file is kept as evidence,
    /// not continued.
    #[test]
    fn a_file_cut_short_is_stamped_with_its_last_entry_on_next_launch() {
        let dir = TempDir::new("crash");
        {
            let (now, c) = clock(at("2026-09-10", "19:03:12"));
            let mut w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
            *lock(&now) = at("2026-09-10", "21:14:07");
            line(&mut w, &now, "the last thing before the crash");
            // Dropped without close(): what a crash looks like to the file.
        }
        let (_now, c) = clock(at("2026-09-10", "21:20:00"));
        let _w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
        let crashed = first_line(&dir.0.join("nodus.2026-09-10-n1.log"));
        assert!(crashed.contains("период: 19:03:12 — 21:14:07 · оборван"), "{crashed}");
        assert_eq!(header_status(&crashed), Some(Status::CutShort));
        assert!(first_line(&dir.0.join("nodus.2026-09-10-n2.log")).contains("ещё пишется"));
    }

    /// A normal close followed by a restart the same day carries on in the same file,
    /// so restarts do not eat into "keep the newest N files".
    #[test]
    fn a_restart_after_a_normal_close_continues_the_same_file() {
        let dir = TempDir::new("continue");
        {
            let (now, c) = clock(at("2026-09-10", "19:03:12"));
            let mut w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
            line(&mut w, &now, "run one");
            w.close();
        }
        let (now, c) = clock(at("2026-09-10", "20:00:00"));
        let mut w = open(&dir.0, c, MAX_FILE_BYTES, Retention::Files(5));
        line(&mut w, &now, "run two");
        assert_eq!(names(&dir.0), ["nodus.2026-09-10-n1.log"]);
        let path = dir.0.join("nodus.2026-09-10-n1.log");
        assert!(first_line(&path).contains("с 19:03:12 · ещё пишется"));
        let body = fs::read_to_string(&path).expect("read");
        assert!(body.contains("run one") && body.contains("run two"));
    }

    #[test]
    fn keep_by_count_never_deletes_the_current_file() {
        let dir = TempDir::new("count");
        let (now, c) = clock(at("2026-09-10", "10:00:00"));
        let mut w = open(&dir.0, c, HEADER_BYTES as u64 + 60, Retention::Files(2));
        for minute in 1..=6 {
            *lock(&now) = at("2026-09-10", &format!("10:{minute:02}:00"));
            line(&mut w, &now, "enough text to fill a tiny file");
        }
        let left = names(&dir.0);
        assert_eq!(left.len(), 2, "{left:?}");
        let current = lock(&w.0).path.clone();
        assert!(current.exists(), "the file being written must survive pruning");
    }

    #[test]
    fn keep_by_size_deletes_oldest_until_under_the_limit() {
        let dir = TempDir::new("bytes");
        for n in 1..=4 {
            fs::write(dir.0.join(file_name("2026-09-09", n)), [b'x'; 100]).expect("seed");
        }
        let current = dir.0.join(file_name("2026-09-10", 1));
        fs::write(&current, [b'x'; 100]).expect("seed current");
        prune_to(&dir.0, &current, None, Some(250));
        assert_eq!(names(&dir.0), ["nodus.2026-09-09-n4.log", "nodus.2026-09-10-n1.log"]);
    }

    /// The old single `nodus.log` and anything else in the folder are not ours to delete.
    #[test]
    fn files_outside_the_scheme_are_left_alone() {
        let dir = TempDir::new("foreign");
        fs::write(dir.0.join("nodus.log"), b"legacy").expect("seed");
        fs::write(dir.0.join("notes.txt"), b"mine").expect("seed");
        let current = dir.0.join(file_name("2026-09-10", 1));
        fs::write(&current, b"now").expect("seed");
        prune_to(&dir.0, &current, Some(1), Some(1));
        assert!(dir.0.join("nodus.log").exists());
        assert!(dir.0.join("notes.txt").exists());
        assert!(current.exists());
    }

    #[test]
    fn retention_packs_mode_and_value_together() {
        set_retention(Retention::Megabytes(80));
        assert_eq!(current_retention(), Retention::Megabytes(80));
        set_retention(Retention::Files(7));
        assert_eq!(current_retention(), Retention::Files(7));
        set_retention(Retention::Files(DEFAULT_MAX_FILES));
    }
}
