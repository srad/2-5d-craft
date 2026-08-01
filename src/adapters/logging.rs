//! Session logging: filter composition, per-session JSON files, and retention.
//!
//! This is a platform adapter, not a service the game injects. Call sites use the `tracing`
//! facade directly; the composition root decides where those events go. Everything here except
//! [`file_layer`] is plain Rust so it can be unit-tested without a Bevy app.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::log::{BoxedLayer, DEFAULT_FILTER};
use bevy::prelude::*;
use tracing_subscriber::Layer;

/// Directory used when `SIDECRAFT_LOG_DIR` is unset.
pub const DEFAULT_LOG_DIR: &str = "logs";

/// Value of `SIDECRAFT_LOG_DIR` that turns the file sink off entirely.
const DISABLED: &str = "off";

/// Session files kept before the oldest are pruned.
const RETAINED_SESSIONS: usize = 10;

const FILE_PREFIX: &str = "sidecraft-";
const FILE_SUFFIX: &str = ".jsonl";

/// Directives appended to [`DEFAULT_FILTER`] when `SIDECRAFT_LOG` is unset.
///
/// Periodic diagnostics are `debug`, so the default run stays quiet; an acceptance run opts in
/// with `SIDECRAFT_LOG`.
const DEFAULT_SIDECRAFT_DIRECTIVES: &str = "sidecraft=info";

/// Keeps the non-blocking writer thread alive.
///
/// Dropping the guard flushes buffered lines, so it must live as long as the app or the tail of
/// every session is lost. Bevy owns app lifetime, so the guard becomes a resource.
#[derive(Resource)]
struct LogWriterGuard(#[allow(dead_code)] tracing_appender::non_blocking::WorkerGuard);

/// Builds the JSON file layer and registers its writer guard.
///
/// Installed through `LogPlugin::custom_layer`, which is a bare `fn` and therefore captures
/// nothing: configuration is read from the environment here. Logging must never prevent the game
/// from starting, so every failure degrades to console-only output.
pub fn file_layer(app: &mut App) -> Option<BoxedLayer> {
    let directory = resolve_log_dir(std::env::var("SIDECRAFT_LOG_DIR").ok().as_deref())?;
    if let Err(error) = std::fs::create_dir_all(&directory) {
        // The subscriber does not exist yet, so `warn!` would be swallowed.
        eprintln!(
            "sidecraft: file logging disabled, cannot create {}: {error}",
            directory.display()
        );
        return None;
    }
    prune_stale_sessions(&directory, RETAINED_SESSIONS);

    let path = directory.join(session_file_name(now_unix_seconds()));
    let file = match std::fs::File::create(&path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!(
                "sidecraft: file logging disabled, cannot create {}: {error}",
                path.display()
            );
            return None;
        }
    };

    let (writer, guard) = tracing_appender::non_blocking(file);
    app.insert_resource(LogWriterGuard(guard));
    Some(Box::new(
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer)
            // The file is machine-read; a span list per line is noise the queries never use.
            .with_current_span(false)
            .with_span_list(false)
            .boxed(),
    ))
}

/// The `EnvFilter` directives for both sinks.
///
/// A configured value composes with [`DEFAULT_FILTER`] rather than replacing it: that constant
/// carries the `wgpu`, `naga`, and `calloop` directives that suppress renderer spam, and dropping
/// them would bury the game's own events.
pub fn configured_filter() -> String {
    compose_filter(std::env::var("SIDECRAFT_LOG").ok().as_deref())
}

fn compose_filter(configured: Option<&str>) -> String {
    let directives = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SIDECRAFT_DIRECTIVES);
    format!("{DEFAULT_FILTER}{directives}")
}

/// Resolves the session directory, or `None` when the file sink is disabled.
fn resolve_log_dir(configured: Option<&str>) -> Option<PathBuf> {
    let value = configured
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_LOG_DIR);
    if value.eq_ignore_ascii_case(DISABLED) {
        return None;
    }
    Some(PathBuf::from(value))
}

fn session_file_name(unix_seconds: u64) -> String {
    format!("{FILE_PREFIX}{unix_seconds}{FILE_SUFFIX}")
}

/// The timestamp a session file name encodes, or `None` if it is not one of ours.
fn session_timestamp(file_name: &str) -> Option<u64> {
    file_name
        .strip_prefix(FILE_PREFIX)?
        .strip_suffix(FILE_SUFFIX)?
        .parse()
        .ok()
}

/// The session files to delete, oldest first, keeping the newest `keep`.
///
/// Ordering is by parsed timestamp rather than by name so it cannot break when the epoch gains a
/// digit, and unrelated files in the directory are ignored rather than deleted.
fn stale_sessions<'a>(file_names: impl IntoIterator<Item = &'a str>, keep: usize) -> Vec<&'a str> {
    let mut sessions = file_names
        .into_iter()
        .filter_map(|name| session_timestamp(name).map(|timestamp| (timestamp, name)))
        .collect::<Vec<_>>();
    sessions.sort_unstable();
    let surplus = sessions.len().saturating_sub(keep);
    sessions
        .into_iter()
        .take(surplus)
        .map(|(_, name)| name)
        .collect()
}

fn prune_stale_sessions(directory: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    let names = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();
    for name in stale_sessions(names.iter().map(String::as_str), keep) {
        // A locked or already-removed file is harmless: the next launch retries.
        let _ = std::fs::remove_file(directory.join(name));
    }
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_filter_keeps_renderer_suppression_and_adds_the_game() {
        let filter = compose_filter(None);
        assert!(filter.contains("wgpu=error"));
        assert!(filter.contains("naga=warn"));
        assert!(filter.ends_with(DEFAULT_SIDECRAFT_DIRECTIVES));
    }

    #[test]
    fn an_override_still_inherits_renderer_suppression() {
        let filter = compose_filter(Some("sidecraft=debug"));
        assert!(filter.contains("wgpu=error"));
        assert!(filter.ends_with("sidecraft=debug"));
        // Blank values are configuration mistakes, not a request for an empty filter.
        assert_eq!(compose_filter(Some("   ")), compose_filter(None));
    }

    #[test]
    fn composed_filters_parse_as_env_filter_directives() {
        for configured in [None, Some("sidecraft=debug"), Some("info,sidecraft=trace")] {
            let filter = compose_filter(configured);
            assert!(
                tracing_subscriber::EnvFilter::builder()
                    .parse(&filter)
                    .is_ok(),
                "{filter} must parse"
            );
        }
    }

    #[test]
    fn the_log_directory_defaults_and_can_be_disabled() {
        assert_eq!(resolve_log_dir(None), Some(PathBuf::from(DEFAULT_LOG_DIR)));
        assert_eq!(resolve_log_dir(Some("   ")), Some(PathBuf::from("logs")));
        assert_eq!(
            resolve_log_dir(Some("custom/place")),
            Some(PathBuf::from("custom/place"))
        );
        assert_eq!(resolve_log_dir(Some("off")), None);
        assert_eq!(resolve_log_dir(Some("OFF")), None);
    }

    #[test]
    fn session_names_round_trip_their_timestamp() {
        assert_eq!(
            session_timestamp(&session_file_name(1_785_602_118)),
            Some(1_785_602_118)
        );
        for foreign in [
            "sidecraft.toml",
            "sidecraft-.jsonl",
            "sidecraft-abc.jsonl",
            "sidecraft-123.log",
            "notes.jsonl",
        ] {
            assert_eq!(session_timestamp(foreign), None, "{foreign}");
        }
    }

    #[test]
    fn retention_keeps_the_newest_and_ignores_foreign_files() {
        let names = [
            session_file_name(300),
            session_file_name(100),
            "worlds.txt".to_string(),
            session_file_name(200),
        ];

        let stale = stale_sessions(names.iter().map(String::as_str), 2);

        assert_eq!(stale, vec![session_file_name(100)]);
    }

    #[test]
    fn retention_orders_by_timestamp_not_by_name_length() {
        // 9-digit and 10-digit epochs sort the wrong way lexically.
        let names = [
            session_file_name(1_000_000_000),
            session_file_name(999_999_999),
        ];

        let stale = stale_sessions(names.iter().map(String::as_str), 1);

        assert_eq!(stale, vec![session_file_name(999_999_999)]);
    }

    #[test]
    fn retention_deletes_nothing_while_under_the_limit() {
        let names = (0..RETAINED_SESSIONS as u64)
            .map(session_file_name)
            .collect::<Vec<_>>();

        assert!(stale_sessions(names.iter().map(String::as_str), RETAINED_SESSIONS).is_empty());
    }

    #[test]
    fn pruning_a_missing_directory_is_harmless() {
        let directory = tempfile::tempdir().unwrap();
        prune_stale_sessions(&directory.path().join("absent"), RETAINED_SESSIONS);
    }

    #[test]
    fn pruning_removes_only_surplus_sessions_from_disk() {
        let directory = tempfile::tempdir().unwrap();
        for timestamp in [10, 20, 30] {
            std::fs::write(directory.path().join(session_file_name(timestamp)), b"{}").unwrap();
        }
        std::fs::write(directory.path().join("keep-me.txt"), b"x").unwrap();

        prune_stale_sessions(directory.path(), 2);

        assert!(!directory.path().join(session_file_name(10)).exists());
        assert!(directory.path().join(session_file_name(20)).exists());
        assert!(directory.path().join(session_file_name(30)).exists());
        assert!(directory.path().join("keep-me.txt").exists());
    }
}
