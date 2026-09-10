//! Incremental transcript scanning — the imperative shell around the pure
//! parser.
//!
//! Transcripts are append-only, so the [`Scanner`] remembers where it stopped
//! reading each file (a byte offset plus a hash of the bytes ending there) and
//! on the next pass reads only what was appended. A file whose size is
//! unchanged is skipped without even opening it. That is what keeps a live,
//! once-a-second refresh cheap while a session writes a growing rollout — and
//! what makes reading ~1.4 GB of cold transcripts a one-time cost.
//!
//! Records are held per file so a rotated or rewritten file (its guard bytes no
//! longer matching) is re-read from the start and simply replaces that file's
//! contribution, never double-counting.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::core::parse::{might_carry_usage, parse_claude_line};
use crate::core::record::UsageRecord;

/// 64 bytes of the tail is ample to notice a file that was replaced rather than
/// appended to.
const GUARD_LEN: usize = 64;

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in bytes {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Where a parse stopped in one file, with enough to resume from there.
struct Cursor {
    /// Byte offset just past the last newline-terminated line consumed.
    offset: u64,
    /// FNV-1a hash of the up-to-[`GUARD_LEN`] bytes ending at `offset`.
    guard: u32,
    guard_len: usize,
}

struct FileState {
    cursor: Cursor,
    records: Vec<UsageRecord>,
}

/// Scans a set of roots for usage records, re-reading only appended bytes on
/// each [`refresh`](Scanner::refresh).
pub struct Scanner {
    roots: Vec<PathBuf>,
    files: HashMap<PathBuf, FileState>,
}

impl Scanner {
    #[must_use]
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Scanner { roots, files: HashMap::new() }
    }

    /// Every record scanned so far, borrowed for the pure core to fold.
    pub fn records(&self) -> impl Iterator<Item = &UsageRecord> {
        self.files.values().flat_map(|state| state.records.iter())
    }

    /// Re-reads changed files and drops vanished ones. Cheap when little has
    /// changed: unchanged files are skipped by size, changed files read only
    /// their appended tail.
    pub fn refresh(&mut self) {
        let present = self.discover();

        let present_set: HashSet<&PathBuf> = present.iter().collect();
        self.files.retain(|path, _| present_set.contains(path));

        for path in present {
            let Ok(meta) = fs::metadata(&path) else { continue };
            let len = meta.len();
            // Fast path: nothing appended since we last consumed this file.
            if self.files.get(&path).is_some_and(|s| s.cursor.offset == len) {
                continue;
            }
            self.scan_file(&path, len);
        }
    }

    /// Walks the roots collecting every `.jsonl` path. Per-entry errors are
    /// swallowed — files rotate mid-walk and a partial listing beats aborting.
    fn discover(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        for root in &self.roots {
            walk(root, &mut out);
        }
        out
    }

    /// Reads one file, resuming from its cursor when the guard still matches.
    fn scan_file(&mut self, path: &Path, len: u64) {
        let prev = self.files.remove(path);

        let resume = match &prev {
            Some(state)
                if state.cursor.offset > 0
                    && state.cursor.offset <= len
                    && state.cursor.guard_len > 0 =>
            {
                read_window(path, state.cursor.offset, state.cursor.guard_len)
                    == Some(state.cursor.guard)
            }
            _ => false,
        };

        let (start, mut records) = match (resume, prev) {
            (true, Some(state)) => (state.cursor.offset, state.records),
            _ => (0, Vec::new()),
        };

        // A readable-earlier file that now won't open: keep what we had, at its
        // old cursor, rather than losing it.
        let Ok(file) = File::open(path) else {
            return;
        };
        let dir_project = decode_project_dir(path);

        let mut reader = BufReader::new(file);
        if reader.seek(SeekFrom::Start(start)).is_err() {
            return;
        }

        let mut offset = start;
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line).unwrap_or(0);
            if read == 0 || line.last() != Some(&b'\n') {
                // EOF, or a trailing line the writer has not finished. Leave it
                // unconsumed: counting a half line now and its full form later
                // would double-count.
                break;
            }
            offset += read as u64;
            if let Some(record) = parse_line(&line, &dir_project) {
                records.push(record);
            }
        }

        let guard_len = GUARD_LEN.min(usize::try_from(offset).unwrap_or(usize::MAX));
        let guard = read_window(path, offset, guard_len).unwrap_or(0);

        self.files.insert(path.to_path_buf(), FileState { cursor: Cursor { offset, guard, guard_len }, records });
    }
}

/// Parses one raw line, filling in a project fallback when the turn's `cwd` was
/// absent.
fn parse_line(raw_line: &[u8], dir_project: &str) -> Option<UsageRecord> {
    let bytes = strip_line_endings(raw_line);
    let text = std::str::from_utf8(bytes).ok()?;
    if !might_carry_usage(text) {
        return None;
    }
    let mut record = parse_claude_line(text)?;
    if record.project.is_empty() {
        record.project = dir_project.to_string();
    }
    Some(record)
}

fn strip_line_endings(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

/// Turns Claude Code's encoded transcript directory name (e.g.
/// `-home-guilherme-project`) back into a readable path for the project label.
fn decode_project_dir(path: &Path) -> String {
    let Some(name) = path.parent().and_then(Path::file_name).and_then(|n| n.to_str()) else {
        return String::new();
    };
    // The encoding replaces path separators with dashes and keeps a leading
    // one; this is lossy (real dashes are indistinguishable) but readable.
    let trimmed = name.strip_prefix('-').unwrap_or(name);
    format!("/{}", trimmed.replace('-', "/"))
}

/// FNV-1a of the `len` bytes ending at `end` in `path`, or `None` on any error.
fn read_window(path: &Path, end: u64, len: usize) -> Option<u32> {
    if len == 0 || len > GUARD_LEN || end < len as u64 {
        return None;
    }
    let mut file = File::open(path).ok()?;
    file.seek(SeekFrom::Start(end - len as u64)).ok()?;
    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).ok()?;
    Some(fnv1a(&buf))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ctracker-scan-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(dir.join("proj")).unwrap();
        dir
    }

    fn assistant_line(msg_id: &str, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","requestId":"r","sessionId":"s","cwd":"/work","timestamp":"2026-09-10T12:00:00Z","message":{{"model":"claude-opus-4-8","id":"{msg_id}","usage":{{"output_tokens":{output}}}}}}}"#
        )
    }

    #[test]
    fn reads_appended_lines_incrementally() {
        let root = temp_root();
        let file = root.join("proj").join("session.jsonl");
        {
            let mut f = File::create(&file).unwrap();
            writeln!(f, "{}", assistant_line("m1", 10)).unwrap();
        }

        let mut scanner = Scanner::new(vec![root.clone()]);
        scanner.refresh();
        assert_eq!(scanner.records().count(), 1);
        let first_total: u64 = scanner.records().map(|r| r.totals.output).sum();
        assert_eq!(first_total, 10);

        // Append a second turn; a refresh should pick up only the new line.
        {
            let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
            writeln!(f, "{}", assistant_line("m2", 5)).unwrap();
        }
        scanner.refresh();
        assert_eq!(scanner.records().count(), 2);
        assert_eq!(scanner.records().map(|r| r.totals.output).sum::<u64>(), 15);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unterminated_trailing_line_is_not_counted_until_finished() {
        let root = temp_root();
        let file = root.join("proj").join("session.jsonl");
        {
            let mut f = File::create(&file).unwrap();
            writeln!(f, "{}", assistant_line("m1", 10)).unwrap();
            // No trailing newline: the writer is mid-line.
            write!(f, "{}", assistant_line("m2", 5)).unwrap();
        }
        let mut scanner = Scanner::new(vec![root.clone()]);
        scanner.refresh();
        assert_eq!(scanner.records().count(), 1);

        // Finish the line; now it counts.
        {
            let mut f = fs::OpenOptions::new().append(true).open(&file).unwrap();
            writeln!(f).unwrap();
        }
        scanner.refresh();
        assert_eq!(scanner.records().count(), 2);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_vanished_file_drops_out() {
        let root = temp_root();
        let file = root.join("proj").join("session.jsonl");
        {
            let mut f = File::create(&file).unwrap();
            writeln!(f, "{}", assistant_line("m1", 10)).unwrap();
        }
        let mut scanner = Scanner::new(vec![root.clone()]);
        scanner.refresh();
        assert_eq!(scanner.records().count(), 1);

        fs::remove_file(&file).unwrap();
        scanner.refresh();
        assert_eq!(scanner.records().count(), 0);

        fs::remove_dir_all(&root).ok();
    }
}
