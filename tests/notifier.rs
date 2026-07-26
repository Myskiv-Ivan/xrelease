//! Integration test: a downstream type can implement [`Notifier`].
//!
//! This is the canary for the trait's `-> impl Future + Send` (RPITIT) design —
//! if an external `impl` ever stops compiling, the public notifier API has
//! regressed.

use std::future::Future;
use std::sync::Mutex;

use xrelease::error::NotifyError;
use xrelease::notify::{Event, Notifier};

#[derive(Default)]
struct CaptureNotifier {
    titles: Mutex<Vec<String>>,
}

impl Notifier for CaptureNotifier {
    fn notify(&self, event: &Event) -> impl Future<Output = Result<(), NotifyError>> + Send {
        let title = event.title.clone();
        async move {
            self.titles.lock().expect("lock").push(title);
            Ok(())
        }
    }
}

fn sample_event(title: &str) -> Event {
    Event {
        source_id: "source".into(),
        source_kind: "Test".into(),
        title: title.into(),
        body: "body".into(),
        url: None,
        routing_tag: None,
    }
}

#[tokio::test]
async fn external_notifier_should_receive_events() {
    let notifier = CaptureNotifier::default();

    notifier
        .notify(&sample_event("release 1"))
        .await
        .expect("notify");

    assert_eq!(
        notifier.titles.lock().expect("lock").as_slice(),
        ["release 1"]
    );
}
