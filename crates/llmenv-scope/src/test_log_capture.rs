//! Test-only tracing log capture helper for this crate's unit tests.

#![expect(clippy::expect_used, reason = "test-only helper")]

pub(crate) fn capture_logs(f: impl FnOnce()) -> String {
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;
    use tracing_subscriber::fmt::writer::MutexGuardWriter;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl<'a> MakeWriter<'a> for Captured {
        type Writer = MutexGuardWriter<'a, Vec<u8>>;

        fn make_writer(&'a self) -> Self::Writer {
            self.0.make_writer()
        }
    }

    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(captured.clone())
        .with_max_level(tracing::Level::TRACE)
        .without_time()
        .finish();
    tracing::subscriber::with_default(subscriber, f);
    let buf = captured.0.lock().expect("test log buffer mutex poisoned");
    String::from_utf8_lossy(&buf).into_owned()
}
