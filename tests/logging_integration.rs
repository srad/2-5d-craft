//! Pins the JSON line contract that acceptance queries read.
//!
//! The subscriber is installed with `with_default`, which is thread-local and scoped: the global
//! subscriber can be set only once per process, so a global install would make these tests
//! order-dependent and unrepeatable.

use std::io::Write;
use std::sync::{Arc, Mutex};

use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;

/// Collects written bytes so a test can read them without a background writer thread.
#[derive(Clone, Default)]
struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

impl CapturedWriter {
    fn contents(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for CapturedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Mirrors the layer configuration in `adapters::logging::file_layer`.
fn capture(emit: impl FnOnce()) -> Vec<serde_json::Value> {
    let writer = CapturedWriter::default();
    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(writer.clone())
            .with_current_span(false)
            .with_span_list(false)
            .boxed(),
    );

    tracing::subscriber::with_default(subscriber, emit);

    writer
        .contents()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("every log line must be valid JSON"))
        .collect()
}

#[test]
fn each_event_is_one_json_line_with_typed_fields() {
    let lines = capture(|| {
        tracing::debug!(
            world_tick = 40_u64,
            steps_last_frame = 2_usize,
            queued_ticks = 3_usize,
            "simulation step"
        );
    });

    assert_eq!(lines.len(), 1);
    let event = &lines[0];
    assert_eq!(event["fields"]["message"], "simulation step");
    assert_eq!(event["level"], "DEBUG");
    assert_eq!(event["target"], "logging_integration");

    // Numbers stay numbers: a query can compare tick rates without parsing prose.
    assert_eq!(event["fields"]["world_tick"], 40);
    assert_eq!(event["fields"]["steps_last_frame"], 2);
    assert_eq!(event["fields"]["queued_ticks"], 3);
    assert!(
        event.get("timestamp").is_some(),
        "a timestamp is required to check tick rate against wall time"
    );
}

#[test]
fn events_below_the_filter_never_reach_the_file() {
    let writer = CapturedWriter::default();
    let subscriber = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(writer.clone())
                .boxed(),
        )
        .with(tracing_subscriber::EnvFilter::new(
            "logging_integration=info",
        ));

    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!("periodic diagnostic");
        tracing::info!("world saved");
    });

    let contents = writer.contents();
    assert!(contents.contains("world saved"));
    assert!(
        !contents.contains("periodic diagnostic"),
        "one global filter governs the file sink too"
    );
}

#[test]
fn multiple_events_stay_one_per_line_and_in_order() {
    let lines = capture(|| {
        for world_tick in [20_u64, 40, 60] {
            tracing::debug!(world_tick, "simulation step");
        }
    });

    assert_eq!(
        lines
            .iter()
            .map(|line| line["fields"]["world_tick"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![20, 40, 60]
    );
}
