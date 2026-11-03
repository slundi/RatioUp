# Code Review — RatioUp (whole project)

## Overview

A lean Rust CLI that fakes BitTorrent upload stats to maintain tracker ratios. The code is generally readable, async-idiomatic, and has solid test coverage for the parser. The architecture is straightforward. The main concerns are dead code accumulation, a few latent panics, and one real performance problem.

---

## Bugs / Correctness

### 1. `Torrent::to_json()` produces invalid JSON when `urls` is empty (`src/torrent.rs:175-184`)

The closing `]}\n` is only appended inside the loop's `else` branch (last iteration). An empty `urls` list silently leaves the JSON object unclosed. In practice, `from_bencode_bytes` rejects empty-URL torrents, so this is never triggered — but it's still a latent trap worth fixing.

```rust
// Fix: append `]}\n` unconditionally after the loop
result.push_str(", \"urls\": [");
for (index, url) in self.urls.iter().enumerate() { ... }
result.push_str("]}\n");
```

### 2. `full_url` is computed but never used (`src/announcer/tracker.rs:232-234`)

`full_url` is built from `url_template` but `built_url` (line 235) is what actually goes into the request. `full_url` is dead code and `url_template` / `headers_to_set` from `client.get_query()` (line 231) are never consumed.

---

## Panics in Non-test Code

### 3. `from_file()` calls `expect()` (`src/torrent.rs:153`)

```rust
let data = std::fs::read(&path).expect("Cannot read torrent file");
```

This unwraps an `io::Error` into a panic inside a function that returns `Result<_, TorrentError>`. The error should be mapped and returned instead.

### 4. `reqwest` client built with `expect()` (`src/announcer/tracker.rs:229`)

A new `ReqwestClient` is built on every single HTTP announce call with `.expect("Failed to build reqwest client")`. This is both a panic risk and a performance problem (see below).

### 5. Two `unwrap()` calls in `json_output::writable()` (`src/json_output.rs:13, 18`)

- `p.parent().unwrap()` panics if the path has no parent component (e.g. `"ratioup.json"`).
- `fs::metadata(parent).unwrap()` panics on any filesystem error reading metadata.

---

## Performance

### 6. New `reqwest::Client` per announce (`src/announcer/tracker.rs:225-229`)

`ReqwestClient` spins up a connection pool internally. Creating one per announce call wastes resources and prevents HTTP keep-alive. The client should be created once in `config::init_client()` and stored (e.g. alongside `CLIENT`, or passed through).

---

## Dead Code — Remove It

There is a substantial amount of commented-out code that should be deleted:

| Location | Description |
|---|---|
| `tracker.rs:11-26` | `print_request_error` function |
| `tracker.rs:178-223` | Old `reqwest` query-building approach |
| `tracker.rs:405-411` | Commented-out `byte_unit` log |
| `scheduler.rs:50-102` | Old sync `announce()` function |
| `torrent.rs:43-51` | `Peer` struct |
| `torrent.rs:119-150` | `from_torrent()` method |
| `main.rs:33` | `std::thread::sleep` |

---

## Unused Dependencies

`once_cell`, `futures-util`, and `sanitize-filename` are declared in `Cargo.toml` but have no `use` anywhere in `src/`:

- `once_cell` — replaced by `tokio::sync::OnceCell`
- `futures-util` — unused
- `sanitize-filename` — unused

---

## Minor Issues

### 7. Typos in log messages

- `tracker.rs:148`: `"Announced:"` → `"Announced:"`
- `tracker.rs:412`: `"Announce at:"` → `"Announce at:"`

### 8. Wrong log level (`main.rs:54`)

```rust
tracing::error!("Warning: Unknown argument: {}, Ignoring", other_arg);
```

Should be `warn!`, not `error!`.

### 9. Redundant debug log (`tracker.rs:49`)

```rust
info!("Time: {}", wait_time);
```

Inside `announce_started()` in a per-torrent loop. Should be `debug!` or removed.

### 10. `#[allow(non_snake_case)]` at crate root (`main.rs:1`)

The statics (`CONFIG`, `TORRENTS`, etc.) are already correct `SCREAMING_SNAKE_CASE` for Rust statics — the allow attribute is not needed and masks any real naming violations.

### 11. `TorrentError` doesn't implement `std::error::Error` (`src/torrent.rs`)

It implements `Display` and `From<BencodeDecoderError>` but not `std::error::Error`. This prevents using it with `?` in functions that return `Box<dyn Error>` or `anyhow::Error`.

### 12. `build_url` takes `key: String` but also re-reads `CLIENT` (`src/announcer/tracker.rs:356, 372`)

The function signature takes `key` as a parameter, but on line 372 it ignores that and clones the `CLIENT` global to get a fresh key. One of these two should be removed.

### 13. `try_lock` fallback silently keeps removed torrents (`src/watcher.rs:218-225`)

```rust
list.retain(|m| {
    if let Ok(t) = m.try_lock() {
        t.info_hash_urlencoded != hash
    } else {
        true // Keep if we can't lock (shouldn't happen)
    }
});
```

"shouldn't happen" is optimistic — at shutdown the announce task holds each torrent lock. The STOPPED announce (lines 203-212) does release the lock before the `write()` guard is acquired at line 217, so it's safe today, but this is subtle and fragile.

---

## Architecture Note

`main.rs:26`:

```rust
static TORRENTS: RwLock<Vec<Mutex<Torrent>>> = RwLock::const_new(Vec::new()); // TODO: replace with mutex
```

The existing TODO is valid: holding the read lock on `TORRENTS` while taking individual `Mutex<Torrent>` locks serializes all announcement work. An `Arc<DashMap<[u8;20], Mutex<Torrent>>>` or a channel-based design would allow parallel per-torrent announcements.

**Update:** The DashMap refactor landed in `dd25c2a`. This note is now stale and can be removed once the items above are addressed.

---

## Nice-to-Have / Future Improvements

### 14. Info hash missing from JSON output (`src/torrent.rs:186`)

There is already a `// TODO: add info hash?` comment at the end of `to_json()`. Exposing the info hash (as a hex string) would let a web UI deep-link to a torrent or match entries against a local library.

### 15. `min_interval` parsed but never honoured

`min_interval` is extracted from tracker responses and stored on `Torrent`, but `should_announce()` only compares against `interval`. Per BEP 3, clients must not re-announce more frequently than `min_interval` even if the user forces a refresh. The scheduler should use `max(interval, min_interval)` as the effective wait.

### 16. Tracker URL validation at startup

Invalid or malformed tracker URLs are only detected on the first announce failure, after the session is already running. Validating each URL during `from_file()` / `from_bencode_bytes()` (using `url::Url::parse`) would surface bad configuration immediately and allow callers to skip or reject broken torrents before the announce loop starts.

### 17. Downloaded is always zero

`Torrent` has an `uploaded` counter updated each cycle, but `downloaded` is hardcoded to `0` in every announce. Some trackers track the download/upload ratio separately. A config option to report a plausible downloaded value (e.g. equal to the torrent length, as if the user just finished downloading) would make announces more realistic.

---

---

## Test Coverage Gaps

> Run coverage with `just coverage-check` (uses `cargo-llvm-cov`, already in the devShell).
> **Do not install `cargo-tarpaulin`** — it requires OpenSSL headers not present in the Nix shell.
> The threshold target is 80% line coverage (`coverage-check` enforces this).

The suite currently has 70 passing tests, but they are concentrated in `bencode.rs`, `utils.rs`, and the pure-function parts of `torrent.rs` / `scheduler.rs`. Large swaths of parsing, configuration loading, URL building, and I/O logic have **zero coverage**.

---

### 1. `torrent.rs` — `from_bencode_bytes()` and `from_file()` (0 tests)

`from_bencode_bytes()` is the most complex function in the project and has no direct tests. Both real `.torrent` files already exist in `tests/` — use them.

**What to add:**

```rust
// In torrent.rs #[cfg(test)]
#[test]
fn test_parse_single_file_torrent() {
    let data = include_bytes!("../tests/ubuntu-20.04.4-desktop-amd64.iso.torrent");
    let t = Torrent::from_bencode_bytes(data).unwrap();
    assert!(!t.name.is_empty());
    assert!(t.length > 0);
    assert!(!t.urls.is_empty());
}

#[test]
fn test_parse_multi_file_torrent() {
    let data = include_bytes!("../tests/Audio_20160422_archive.torrent");
    let t = Torrent::from_bencode_bytes(data).unwrap();
    assert!(t.length > 0); // sum of all file lengths
}
```

**Error paths to cover** (via crafted bencode inputs):
- Top-level not a dictionary → `InvalidFieldType`
- Missing `info` key → `MissingField("info")`
- Missing `info.name` → `MissingField("info.name")`
- Negative `length` → `ParseError`
- `announce` and `announce-list` both absent or all unsupported URLs → `MissingField`
- All `TorrentError` Display variants (one `format!("{e}")` per variant)

---

### 2. `config.rs` — `load_from_file()` (0 tests)

The entire async config-loading function is untested. It has ~10 distinct branches (missing file, invalid TOML, each field valid/invalid, swapped rates). These are all pure I/O + parsing — easy to cover with a temp file.

**What to add:**

```rust
#[tokio::test]
async fn test_load_from_file_valid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    tokio::fs::write(&path, b"port = 51413\nclient = \"qBittorrent_4_60\"\n").await.unwrap();
    let cfg = Config::load_from_file(&path).await;
    assert_eq!(cfg.port, 51413);
    assert_eq!(cfg.client, "qBittorrent_4_60");
}

#[tokio::test]
async fn test_load_from_file_missing() {
    let cfg = Config::load_from_file(&PathBuf::from("/nonexistent/config.toml")).await;
    assert_eq!(cfg.port, Config::default().port); // falls back
}

#[tokio::test]
async fn test_load_from_file_swapped_rates() {
    // min > max → they get swapped back
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cfg.toml");
    tokio::fs::write(&path, b"min_upload_rate = 2000000\nmax_upload_rate = 8192\n").await.unwrap();
    let cfg = Config::load_from_file(&path).await;
    assert!(cfg.min_upload_rate <= cfg.max_upload_rate);
}
```

Also cover: invalid port (0 or >65535), invalid `numwant`, non-integer fields, `output_stats` path.

> Add `tempfile` as a dev-dependency (`cargo add --dev tempfile`) to avoid manual tmp-dir cleanup.

---

### 3. `announcer/tracker.rs` — `build_url()` (0 tests)

`build_url()` is a pure function (no network, no global CLIENT needed) that assembles the full tracker query string. It is completely untested despite containing all the placeholder-replacement logic.

**What to add:**

```rust
#[test]
fn test_build_url_started_event() {
    // Needs a fake Client — use fake_torrent_client directly
    let mut client = fake_torrent_client::Client::default();
    client.build(fake_torrent_client::clients::ClientVersion::from_str("Transmission_3_00").unwrap());
    let mut torrent = /* make_torrent helper */;
    let url = build_url("http://tracker.example.com/announce", &mut torrent, Some(Event::Started), &client);
    assert!(url.contains("event=started"));
    assert!(url.contains("uploaded=0")); // elapsed=0 for Started
    assert!(url.contains("downloaded=0"));
}

#[test]
fn test_build_url_none_event() {
    // event=None → {event} replaced with ""
    // URL with existing '?' → separator is '&', not '?'
}

#[test]
fn test_build_url_appends_separator() {
    // URL without '?' → '?' appended
    // URL with existing '?' → '&' appended
}
```

---

### 4. `directory.rs` — `load_torrents()` (0 tests)

No test file exists for this module. It loads `.torrent` files from a directory and populates the global `TORRENTS` map.

**What to add** (use the real `.torrent` fixtures):

```rust
#[tokio::test]
async fn test_load_torrents_counts_valid_files() {
    // Copy test fixtures into a temp dir, call load_torrents(), check count
}

#[tokio::test]
async fn test_load_torrents_skips_non_torrent_files() {
    // Place a .txt file in the dir, verify it's not counted
}
```

Note: `load_torrents()` has an `expect()` call on `std::fs::read_dir` — that panic path is also untested.

---

### 5. `json_output.rs` — `writable()` missing branches

The existing test misses two branches:

| Branch | Code path | Missing test |
|---|---|---|
| Path ending with `/` | `if path.ends_with('/')` → early `false` | `writable("/tmp/")` should return `false` |
| `p.parent()` returns `None` | The `let Some(parent) = p.parent() else { … }` arm | Unreachable in practice — see note below |

> **Note:** `Path::new("file.txt").parent()` returns `Some(Path::new(""))`, not `None`, so the `None` branch is dead code for all non-empty inputs. The guard can be simplified to remove it, or a comment should explain when it triggers (only for the root path on some platforms).

---

### 6. `announcer/udp.rs` — error paths and `TrackerError` Display

The six existing tests cover struct creation and DNS resolution. Missing:

- `TrackerError` Display for each variant (`IoError`, `Timeout`, `InvalidResponse`, `Remote`, `ParseError`) — one assert per variant, no network needed.
- Peer parsing logic in `announce_with_connection_id` — can be tested by constructing a synthetic response buffer (20-byte header + 6 bytes per peer).
- Short peer ID (< 20 bytes) branch in `announce_udp` — the `arr[..peer_id_bytes.len()]` copy path.

---

### 7. `torrent.rs` — tracker response parsing (existing TODO)

The comment at `torrent.rs:372` already flags this:

```
// TODO: test tracker response "d8:completei0e10:downloadedi0e10:incompletei1e..."
```

The response-parsing logic lives in `announce_http()` (tracker.rs:277–341). Extract it into a pure helper function `parse_tracker_response(bytes: &[u8]) -> TrackerUpdate` so it can be unit-tested without a live HTTP server. Then cover:

- Normal response with `interval`, `min interval`, `complete`, `incomplete`
- `failure reason` response
- `warning message` response
- Non-dictionary response (returns error)

---

### Priority Order

| Priority | Module | Why |
|---|---|---|
| 1 | `torrent.rs` — `from_bencode_bytes` | Core parsing, fixtures already exist, no stubs needed |
| 2 | `announcer/tracker.rs` — `build_url` | Pure function, high complexity, zero coverage |
| 3 | `config.rs` — `load_from_file` | ~10 untested branches, easy with temp files |
| 4 | `directory.rs` | Zero tests, uses real fixtures |
| 5 | `json_output.rs` — missing `writable` branches | Two-line fixes |
| 6 | `announcer/udp.rs` — error paths | No network needed |
| 7 | Tracker response parsing | Requires refactor of `announce_http` first |

---

## Positive Notes

- Bencode parser is robust, well-tested, and handles edge cases correctly
- UDP BEP 15 implementation is complete and correctly structured
- XDG compliance is handled properly
- The jitter logic in `scheduler.rs` is correct and well-tested
- The notify-based watcher correctly bridges sync/async boundaries
- Graceful SIGINT shutdown with STOPPED announces is a nice touch

---

Minor Polish

- Typo: "Announced:" → "Announced:" (tracker.rs:148)
- Typo: "Announce at:" → "Announce at:" (tracker.rs:412)
- Wrong level: tracing::error! for unknown arg → warn! (main.rs:54)
- Wrong level: info!("Time: ...") inside per-torrent loop → debug! (tracker.rs:49)
- #[allow(non_snake_case)] at crate root is unnecessary — statics are already SCREAMING_SNAKE_CASE

Nice-to-Have Features

1. Info hash in JSON output — there's already a // TODO: add info hash? at torrent.rs:186
2. Parallel announces — the DashMap is in place but announces are still serial; removing the outer RwLock<Vec> wrapping would let each torrent announce independently
3. min_interval respected in scheduling — it's parsed from tracker responses but not used in should_announce()
decisions
4. Startup URL validation — validate tracker URLs when loading torrents, not only on first announce failure
5. Downloaded tracking — currently always 0; even a rough estimate (seeding-only: report 0) is correct, but the
field exists for completeness
