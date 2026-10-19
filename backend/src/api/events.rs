use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures_util::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

use crate::state::AppState;

/// Live run updates, progress and log lines. A `resync` message asks the client to refetch
/// because it fell behind.
pub async fn stream(State(state): State<AppState>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let events = BroadcastStream::new(state.events.subscribe()).map(|message| {
        let data = match message {
            Ok(event) => serde_json::to_string(&event).unwrap_or_else(|_| "{\"type\":\"resync\"}".into()),
            Err(BroadcastStreamRecvError::Lagged(_)) => "{\"type\":\"resync\"}".to_string(),
        };
        Ok(SseEvent::default().data(data))
    });
    let hello = futures_util::stream::once(async { Ok(SseEvent::default().data("{\"type\":\"hello\"}")) });
    let stream = hello.chain(events).take_until(state.shutdown.clone().cancelled_owned());
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
