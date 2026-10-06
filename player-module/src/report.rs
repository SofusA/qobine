//! Streaming reports, sent the way the web player sends them: a start event when a track starts and an end event for every stretch played without interruption, posted in batches.

use std::{sync::Arc, time::Duration};

use controls_module::{PositionReceiver, Status, StatusReceiver, TracklistReceiver};
use qobuz_client::client::StreamingEnd;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{select, sync::watch, task::JoinHandle, time::Instant};

use crate::client::StreamClient;

const FLUSH: Duration = Duration::from_secs(10);
/// Flushes skipped after each failed one in a row, the web player's schedule; the batch is dropped after the last.
const RETRIES: [usize; 4] = [1, 3, 12, 180];
const BATCH: usize = 50;
/// How long a report may take, so that a stalled request never holds the client or the exit.
const TIMEOUT: Duration = Duration::from_secs(3);
/// A position behind the previous one, or further ahead than the time elapsed plus this, is a seek, which ends a stretch.
const JUMP: Duration = Duration::from_secs(2);

/// The reporting task of a player.
pub(crate) struct Reports {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Reports {
    /// Reports from what the watches say the player does; `finish` reports the stretch being played when the player stops.
    pub(crate) fn spawn(
        client: Arc<StreamClient>,
        active: watch::Receiver<bool>,
        status: StatusReceiver,
        position: PositionReceiver,
        tracklist: TracklistReceiver,
    ) -> Self {
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(run(client, active, status, position, tracklist, stopped));
        Self { stop, task }
    }

    pub(crate) async fn finish(self) {
        let _ = self.stop.send(true);
        let _ = tokio::time::timeout(TIMEOUT, self.task).await;
    }
}

async fn run(
    client: Arc<StreamClient>,
    mut active: watch::Receiver<bool>,
    mut status: StatusReceiver,
    mut position: PositionReceiver,
    mut tracklist: TracklistReceiver,
    mut stopped: watch::Receiver<bool>,
) {
    let current = |tracklist: &TracklistReceiver| {
        tracklist
            .borrow()
            .current_track()
            .map(|track| (track.id, track.duration_seconds))
    };
    let mut reporter = Reporter {
        client,
        active: *active.borrow(),
        track: current(&tracklist),
        stretch: None,
        announced: None,
        events: Vec::new(),
        failures: 0,
        skip: 0,
    };
    let mut flush = tokio::time::interval(FLUSH);
    loop {
        select! {
            biased;
            changed = active.changed() => {
                if changed.is_err() {
                    break;
                }
                let on = *active.borrow();
                reporter.activate(on);
            }
            changed = tracklist.changed() => {
                if changed.is_err() {
                    break;
                }
                let playing = *status.borrow() == Status::Playing;
                reporter.track(current(&tracklist), playing).await;
            }
            changed = status.changed() => {
                if changed.is_err() {
                    break;
                }
                let playing = *status.borrow() == Status::Playing;
                reporter.playing(playing).await;
            }
            changed = position.changed() => {
                if changed.is_err() {
                    break;
                }
                let at = *position.borrow();
                reporter.moved(at).await;
            }
            _ = flush.tick() => reporter.flush().await,
            _ = stopped.changed() => break,
        }
    }
    reporter.end();
    reporter.skip = 0;
    reporter.flush().await;
}

struct Reporter {
    client: Arc<StreamClient>,
    active: bool,
    track: Option<(u32, u32)>,
    stretch: Option<Stretch>,
    announced: Option<String>,
    events: Vec<StreamingEnd>,
    failures: usize,
    skip: usize,
}

/// A track playing since a start, resume or seek. `from` is the first position seen, `last` the latest, at `seen`.
struct Stretch {
    track_id: u32,
    format_id: Option<i32>,
    blob: Option<String>,
    track_duration: u32,
    since: OffsetDateTime,
    from: Option<Duration>,
    last: Duration,
    seen: Instant,
}

impl Reporter {
    /// While another device is the active one, what the watches carry is its playback, not ours.
    fn activate(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.end();
        }
    }

    async fn track(&mut self, track: Option<(u32, u32)>, playing: bool) {
        if self.track.map(|(id, _)| id) == track.map(|(id, _)| id) {
            return;
        }
        self.end();
        self.track = track;
        if self.active && playing {
            self.begin().await;
        }
    }

    async fn playing(&mut self, playing: bool) {
        if !playing {
            self.end();
        } else if self.active && self.stretch.is_none() {
            self.begin().await;
        }
    }

    async fn moved(&mut self, position: Duration) {
        if !self.active {
            return;
        }
        let Some(stretch) = self.stretch.as_ref() else {
            return;
        };
        let ahead = stretch
            .last
            .saturating_add(stretch.seen.elapsed())
            .saturating_add(JUMP);
        if stretch.from.is_some() && (position < stretch.last || position > ahead) {
            tracing::trace!("Position jumped from {:?} to {position:?}", stretch.last);
            self.end();
            self.begin().await;
        }
        let Some(stretch) = self.stretch.as_mut() else {
            return;
        };
        stretch.last = position;
        stretch.seen = Instant::now();
        if stretch.from.is_some() {
            return;
        }
        stretch.from = Some(position);
        let (track_id, format_id, blob) =
            (stretch.track_id, stretch.format_id, stretch.blob.clone());
        self.report_start(track_id, format_id, blob);
    }

    async fn begin(&mut self) {
        let Some((track_id, track_duration)) = self.track else {
            return;
        };
        let token = self.client.stream_token(track_id).await;
        self.stretch = Some(Stretch {
            track_id,
            format_id: token.as_ref().and_then(|token| token.format_id),
            blob: token.and_then(|token| token.blob),
            track_duration,
            since: OffsetDateTime::now_utc(),
            from: None,
            last: Duration::ZERO,
            seen: Instant::now(),
        });
    }

    /// The start event, once per stream `file/url` granted, off the loop so that a slow request delays nothing.
    fn report_start(&mut self, track_id: u32, format_id: Option<i32>, blob: Option<String>) {
        let (Some(format_id), Some(blob)) = (format_id, blob) else {
            return;
        };
        if self.announced.as_ref() == Some(&blob) {
            return;
        }
        self.announced = Some(blob);
        let client = self.client.clone();
        tokio::spawn(async move {
            let report = client.report_streaming_start(track_id, format_id);
            let reported = tokio::time::timeout(TIMEOUT, report).await;
            if !matches!(reported, Ok(Ok(()))) {
                tracing::debug!("Reporting the start of track {track_id} failed: {reported:?}");
            }
        });
    }

    fn end(&mut self) {
        let Some(stretch) = self.stretch.take() else {
            return;
        };
        let (Some(blob), Some(from)) = (stretch.blob, stretch.from) else {
            return;
        };
        let duration = stretch
            .last
            .saturating_sub(from)
            .as_secs()
            .min(u64::from(stretch.track_duration));
        let Ok(start_stream) = stretch.since.format(&Rfc3339) else {
            return;
        };
        tracing::trace!(
            "Track {} streamed from {from:?} to {:?}",
            stretch.track_id,
            stretch.last
        );
        if duration > 0 {
            self.events.push(StreamingEnd {
                blob,
                track_context_uuid: String::new(),
                start_stream,
                online: true,
                local: false,
                duration,
            });
        }
    }

    async fn flush(&mut self) {
        if self.events.is_empty() {
            return;
        }
        if self.skip > 0 {
            self.skip = self.skip.saturating_sub(1);
            return;
        }
        let batch = self.events.len().min(BATCH);
        let Some(events) = self.events.get(..batch) else {
            return;
        };
        let report = self.client.report_streaming_end(events);
        match tokio::time::timeout(TIMEOUT, report).await {
            Ok(Ok(())) => {
                let seconds: Vec<u64> = events.iter().map(|event| event.duration).collect();
                tracing::debug!("Reported {batch} streamed stretches of {seconds:?} seconds");
                self.events.drain(..batch);
                self.failures = 0;
            }
            failed => {
                self.failures = self.failures.saturating_add(1);
                let Some(skip) = RETRIES.get(self.failures.saturating_sub(1)) else {
                    tracing::debug!("Giving up on {batch} streamed stretches: {failed:?}");
                    self.events.drain(..batch);
                    self.failures = 0;
                    return;
                };
                tracing::debug!("Reporting {batch} streamed stretches failed: {failed:?}");
                self.skip = *skip;
            }
        }
    }
}
