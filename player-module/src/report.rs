use std::{sync::Arc, time::Duration};

use qobuz_client::client::StreamingEnd;
use time::{OffsetDateTime, macros::format_description};
use tokio::sync::watch;
use uuid::Uuid;

use crate::client::StreamClient;

const QOBUZ_TIMESTAMP: &[time::format_description::FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z");

/// Reports Qobuz streaming events directly from Player lifecycle hooks.
///
/// Call `track_finished`, `stopped`, or `paused` before replacing the current
/// track with `track_started`.
pub struct StreamReporter {
    client: Arc<StreamClient>,
    active: watch::Receiver<bool>,
    session: Option<StreamSession>,
    prepared: Option<PreparedStream>,
}

struct PreparedStream {
    track_id: u32,
    track_duration: Duration,
    local: bool,
}

struct StreamSession {
    track_id: u32,
    blob: String,
    track_context_uuid: String,
    track_duration: Duration,
    local: bool,
    stretch: Option<Stretch>,
}

struct Stretch {
    started_at: OffsetDateTime,
    started_position: Duration,
}

impl StreamReporter {
    pub fn new(client: Arc<StreamClient>, active: watch::Receiver<bool>) -> Self {
        Self {
            client,
            active,
            session: None,
            prepared: None,
        }
    }

    pub fn prepare_track(&mut self, track_id: u32, track_duration: Duration, local: bool) {
        if !self.is_active() {
            self.prepared = None;
            return;
        }

        self.prepared = Some(PreparedStream {
            track_id,
            track_duration,
            local,
        });
    }

    pub async fn prepared_track_started(&mut self, position: Duration) {
        if !self.is_active() {
            self.prepared = None;
            self.session = None;
            return;
        }

        let Some(prepared) = self.prepared.take() else {
            return;
        };

        self.track_started(
            prepared.track_id,
            prepared.track_duration,
            position,
            prepared.local,
        )
        .await;
    }

    /// Starts reporting a newly acquired Qobuz stream.
    ///
    /// This sends the streaming-start event once and begins the first
    /// uninterrupted playback stretch.
    pub async fn track_started(
        &mut self,
        track_id: u32,
        track_duration: Duration,
        position: Duration,
        local: bool,
    ) {
        if !self.is_active() {
            self.session = None;
            self.prepared = None;
            return;
        }

        // A previous session should normally have been ended explicitly.
        if self.session.is_some() {
            tracing::warn!("Replacing an unfinished streaming session with track {track_id}");
            self.session = None;
        }

        let Some(token) = self.client.stream_token(track_id).await else {
            tracing::debug!("Cannot report track {track_id}: no Qobuz stream token");
            return;
        };

        let (Some(format_id), Some(blob)) = (token.format_id, token.blob) else {
            tracing::debug!(
                "Cannot report track {track_id}: stream token has no format ID or blob"
            );
            return;
        };

        let client = self.client.clone();

        tokio::spawn(async move {
            if let Err(error) = client.report_streaming_start(track_id, format_id).await {
                tracing::warn!("Failed to report the start of track {track_id}: {error:?}");
            }
        });

        self.session = Some(StreamSession {
            track_id,
            blob,
            track_context_uuid: Uuid::new_v4().to_string(),
            track_duration,
            local,
            stretch: Some(Stretch {
                started_at: OffsetDateTime::now_utc(),
                started_position: position,
            }),
        });
    }

    /// Ends the current uninterrupted stretch.
    pub fn paused(&mut self, position: Duration) {
        self.report_stretch_end(position);
    }

    /// Begins another uninterrupted stretch without sending another
    /// streaming-start event.
    pub fn resumed(&mut self, position: Duration) {
        if !self.is_active() {
            self.session = None;
            self.prepared = None;
            return;
        }

        let Some(session) = self.session.as_mut() else {
            return;
        };

        if session.stretch.is_some() {
            return;
        }

        session.stretch = Some(Stretch {
            started_at: OffsetDateTime::now_utc(),
            started_position: position,
        });
    }

    /// Ends the stretch before the seek and begins a new one at its destination.
    pub fn seeked(&mut self, previous_position: Duration, new_position: Duration) {
        self.report_stretch_end(previous_position);
        self.resumed(new_position);
    }

    /// Ends the final stretch and closes the track session.
    pub fn track_finished(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };

        let end_position = session.track_duration;
        self.report_stretch_end(end_position);
        self.session = None;
    }

    /// Ends playback without the track finishing naturally.
    pub fn stopped(&mut self, position: Duration) {
        self.report_stretch_end(position);
        self.session = None;
        self.prepared = None;
    }

    /// Ends playback because this player is no longer the active device.
    pub fn deactivated(&mut self, position: Duration) {
        self.report_stretch_end(position);
        self.session = None;
        self.prepared = None;
    }

    fn is_active(&self) -> bool {
        *self.active.borrow()
    }

    fn report_stretch_end(&mut self, end_position: Duration) {
        let Some(session) = self.session.as_mut() else {
            return;
        };

        let Some(stretch) = session.stretch.take() else {
            return;
        };

        let duration = end_position
            .saturating_sub(stretch.started_position)
            .min(session.track_duration)
            .as_secs();

        if duration == 0 {
            return;
        }

        let Ok(start_stream) = stretch.started_at.format(QOBUZ_TIMESTAMP) else {
            tracing::warn!(
                "Failed to format stream start time for track {}",
                session.track_id
            );
            return;
        };

        let track_id = session.track_id;

        let event = StreamingEnd {
            blob: session.blob.clone(),
            track_context_uuid: session.track_context_uuid.clone(),
            start_stream,
            online: true,
            local: session.local,
            duration,
        };

        let client = self.client.clone();

        tokio::spawn(async move {
            if let Err(error) = client.report_streaming_end(&[event]).await {
                tracing::warn!("Failed to report the end of track {track_id}: {error:?}");
            }
        });
    }
}
