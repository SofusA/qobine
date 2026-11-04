#[cfg(all(feature = "rodio", feature = "mpv"))]
compile_error!("features `rodio` and `mpv` cannot be enabled at the same time");

#[cfg(not(any(feature = "rodio", feature = "mpv")))]
compile_error!("either feature `rodio` or feature `mpv` must be enabled");

#[derive(Debug, Clone, Copy)]
pub struct TrackFinished {
    pub generation: usize,
    pub queue_id: u64,
}

pub enum QueryTrackResult {
    Queued,
    #[cfg(feature = "rodio")]
    RecreateStreamRequired,
}

#[cfg(feature = "rodio")]
mod implementation {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use controls_module::VolumeReceiver;
    use rodio::source::SeekError;
    use tokio::sync::mpsc::UnboundedSender;
    use tokio::task::JoinHandle;

    use crate::AppResult;
    use crate::error::PlayerError;
    use crate::sink::{QueryTrackResult, TrackFinished};

    use std::fs;
    use std::io::{Read, Seek};
    use std::num::NonZero;

    use parking_lot::Mutex;
    use qobuz_client::stream::flac_source_stream::SeekableStreamReader;
    use rodio::cpal::traits::HostTrait;
    use rodio::queue::queue;
    use rodio::{ChannelCount, Decoder, DeviceTrait, Player, Sample, SampleRate, Source};
    use tokio::time::sleep;

    use crate::stderr_redirect::silence_stderr;

    use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
    use std::thread;

    const CHUNK_DURATION_MS: u32 = 100;
    const CHUNKS_AHEAD: usize = 20;

    type Chunk = (u64, Vec<Sample>);

    pub struct Prefetch {
        chunks: Receiver<Chunk>,
        seeks: Sender<(u64, Duration)>,
        current: std::vec::IntoIter<Sample>,
        epoch: u64,
        silence: u16,
        channels: ChannelCount,
        sample_rate: SampleRate,
        total_duration: Option<Duration>,
    }

    impl Prefetch {
        pub fn new<S: Source + Send + 'static>(source: S) -> Self {
            let channels = source.channels();
            let sample_rate = source.sample_rate();
            let total_duration = source.total_duration();
            let frames = sample_rate
                .get()
                .saturating_mul(CHUNK_DURATION_MS)
                .div_ceil(1000);
            let chunk_len = usize::try_from(frames)
                .unwrap_or(usize::MAX)
                .saturating_mul(usize::from(channels.get()));

            let (chunk_sender, chunks) = mpsc::sync_channel(CHUNKS_AHEAD);
            let (seeks, seek_receiver) = mpsc::channel();

            thread::spawn(move || decode(source, chunk_len, &chunk_sender, &seek_receiver));

            Self {
                chunks,
                seeks,
                current: Vec::new().into_iter(),
                epoch: 0,
                silence: 0,
                channels,
                sample_rate,
                total_duration,
            }
        }
    }

    fn decode<S: Source>(
        mut source: S,
        chunk_len: usize,
        chunks: &SyncSender<Chunk>,
        seeks: &Receiver<(u64, Duration)>,
    ) {
        let mut epoch = 0;

        loop {
            let chunk: Vec<Sample> = source.by_ref().take(chunk_len).collect();
            let finished = chunk.is_empty();

            if chunks.send((epoch, chunk)).is_err() {
                return;
            }

            let waiting = if finished {
                match seeks.recv() {
                    Ok(seek) => Some(seek),
                    Err(_) => return,
                }
            } else {
                None
            };

            if let Some((seek_epoch, position)) = seeks.try_iter().last().or(waiting) {
                epoch = seek_epoch;
                if let Err(err) = source.try_seek(position) {
                    tracing::warn!("Seek error: {err:?}");
                }
            }
        }
    }

    impl Iterator for Prefetch {
        type Item = Sample;

        fn next(&mut self) -> Option<Sample> {
            if let Some(sample) = self.current.next() {
                return Some(sample);
            }

            if self.silence > 0 {
                self.silence = self.silence.saturating_sub(1);
                return Some(0.0);
            }

            loop {
                match self.chunks.try_recv() {
                    Ok((epoch, _)) if epoch != self.epoch => {}
                    Ok((_, chunk)) if chunk.is_empty() => return None,
                    Ok((_, chunk)) => {
                        self.current = chunk.into_iter();
                        return self.current.next();
                    }
                    Err(TryRecvError::Empty) => {
                        self.silence = self.channels.get() - 1;
                        return Some(0.0);
                    }
                    Err(TryRecvError::Disconnected) => return None,
                }
            }
        }
    }

    impl Source for Prefetch {
        fn current_span_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> ChannelCount {
            self.channels
        }

        fn sample_rate(&self) -> SampleRate {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            self.total_duration
        }

        fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
            self.epoch = self.epoch.wrapping_add(1);
            self.current = Vec::new().into_iter();
            self.silence = 0;
            let _ = self.seeks.send((self.epoch, position));
            Ok(())
        }
    }

    struct Playback {
        player: Player,
        output_stream: rodio::MixerDeviceSink,
        sender: Arc<rodio::queue::SourcesQueueInput>,
    }

    pub struct Sink {
        playback: Option<Playback>,
        volume: VolumeReceiver,
        track_finished_tx: UnboundedSender<TrackFinished>,
        current_track_handle: Option<JoinHandle<()>>,
        queued_track_handle: Option<JoinHandle<()>>,
        duration_played: Arc<Mutex<Duration>>,
        playback_generation: Arc<AtomicUsize>,
        preferred_device_id: Option<String>,
    }

    impl Sink {
        pub fn new(
            volume: VolumeReceiver,
            preferred_device_id: Option<String>,
            track_finished_tx: UnboundedSender<TrackFinished>,
        ) -> Self {
            Self {
                playback: None,
                volume,
                track_finished_tx,
                current_track_handle: None,
                queued_track_handle: None,
                duration_played: Arc::default(),
                playback_generation: Arc::new(AtomicUsize::new(0)),
                preferred_device_id,
            }
        }

        pub fn playback_generation(&self) -> usize {
            self.playback_generation.load(Ordering::SeqCst)
        }

        pub fn position(&self) -> Duration {
            let position = self
                .playback
                .as_ref()
                .map(|x| &x.player)
                .map(rodio::Player::get_pos)
                .unwrap_or_default();

            let duration_played = *self.duration_played.lock();

            if position < duration_played {
                return Duration::default();
            }

            position.checked_sub(duration_played).unwrap_or_default()
        }

        pub fn play(&self) {
            if let Some(playback) = &self.playback {
                playback.player.play();
            }
        }

        pub fn pause(&self) {
            if let Some(playback) = &self.playback {
                playback.player.pause();
            }
        }

        pub fn seek(&self, duration: Duration) -> AppResult<()> {
            if let Some(playback) = &self.playback {
                let player = &playback.player;

                let current_volume = *self.volume.borrow();
                let was_paused = player.is_paused();

                player.set_volume(0.0);
                player.pause();

                let result = player.try_seek(duration);

                if !was_paused {
                    player.play();
                }

                set_volume(player, current_volume);

                match result {
                    Ok(()) => {
                        *self.duration_played.lock() = Duration::default();
                    }
                    Err(err) => {
                        tracing::warn!("rodio seek error: {err:?}");
                        return Err(err.into());
                    }
                }
            }

            Ok(())
        }

        pub fn clear(&mut self) {
            tracing::info!("Clearing sink");

            self.playback_generation.fetch_add(1, Ordering::SeqCst);

            if let Some(handle) = self.current_track_handle.take() {
                handle.abort();
            }

            if let Some(handle) = self.queued_track_handle.take() {
                handle.abort();
            }

            if let Some(playback) = self.playback.as_ref() {
                playback.sender.clear();
            }

            self.playback = None;
            *self.duration_played.lock() = Duration::default();
        }

        pub fn clear_queue(&mut self) {
            tracing::info!("Clearing sink queue");

            if let Some(handle) = self.queued_track_handle.take() {
                handle.abort();
            }

            if let Some(playback) = self.playback.as_ref() {
                playback.sender.clear();
            }
        }

        pub fn promote_queued_track(&mut self) {
            self.current_track_handle = self.queued_track_handle.take();
        }

        pub const fn is_empty(&self) -> bool {
            self.playback.is_none()
        }

        pub fn query_track(
            &mut self,
            track_path: &Path,
            queue_id: u64,
        ) -> AppResult<QueryTrackResult> {
            tracing::info!("Sink query track: {}", track_path.to_string_lossy());

            let file = fs::File::open(track_path).map_err(|err| PlayerError::StreamError {
                message: format!("Failed to read file: {}: {err}", track_path.display()),
            })?;

            let source = Decoder::try_from(file)?;
            self.queue_decoder(source, queue_id)
        }

        pub fn query_track_stream(
            &mut self,
            reader: SeekableStreamReader,
            queue_id: u64,
        ) -> AppResult<QueryTrackResult> {
            tracing::info!("Sink query track (streaming)");

            let byte_len = reader.content_length();

            let source = Decoder::builder()
                .with_data(reader)
                .with_byte_len(byte_len)
                .with_seekable(true)
                .build()
                .map_err(|err| PlayerError::StreamError {
                    message: format!("Failed to decode streaming FLAC: {err}"),
                })?;

            self.queue_decoder(source, queue_id)
        }

        fn queue_decoder<T>(
            &mut self,
            source: Decoder<T>,
            queue_id: u64,
        ) -> AppResult<QueryTrackResult>
        where
            T: Read + Seek + Send + 'static,
        {
            let sample_rate = source.sample_rate();

            let same_sample_rate = self.playback.as_ref().is_none_or(|playback| {
                playback.output_stream.config().sample_rate() == sample_rate
            });

            if !same_sample_rate {
                return Ok(QueryTrackResult::RecreateStreamRequired);
            }

            let is_queued_track = self.playback.is_some();

            if self.playback.is_none() {
                let mut mixer = if let Some(preferred_device_name) =
                    self.preferred_device_id.as_deref()
                {
                    silence_stderr(|| open_preferred_stream(sample_rate, preferred_device_name))?
                } else {
                    open_default_stream(sample_rate)?
                };

                mixer.log_on_drop(false);

                let (sender, receiver) = queue(true);
                let player = rodio::Player::connect_new(mixer.mixer());

                player.append(receiver);
                set_volume(&player, *self.volume.borrow());

                self.playback = Some(Playback {
                    player,
                    output_stream: mixer,
                    sender,
                });
            }

            let playback = self.playback.as_ref().ok_or(PlayerError::SinkDeviceError {
                message: "Playback not initialized".to_string(),
            })?;

            let track_finished_tx = self.track_finished_tx.clone();
            let track_duration = source.total_duration().unwrap_or_default();
            let duration_played = self.duration_played.clone();
            let playback_generation = self.playback_generation.clone();
            let expected_generation = playback_generation.load(Ordering::SeqCst);

            let signal = playback.sender.append_with_signal(Prefetch::new(source));

            let handle = tokio::spawn(async move {
                loop {
                    if signal.try_recv().is_ok() {
                        if playback_generation.load(Ordering::SeqCst) != expected_generation {
                            tracing::debug!(queue_id, "Ignoring obsolete track-finished signal");
                            return;
                        }

                        {
                            let mut duration_played = duration_played.lock();
                            *duration_played = duration_played.saturating_add(track_duration);
                        }

                        let _ = track_finished_tx.send(TrackFinished {
                            generation: expected_generation,
                            queue_id,
                        });

                        return;
                    }

                    sleep(Duration::from_millis(200)).await;
                }
            });

            if is_queued_track {
                if let Some(old_handle) = self.queued_track_handle.replace(handle) {
                    old_handle.abort();
                }
            } else if let Some(old_handle) = self.current_track_handle.replace(handle) {
                old_handle.abort();
            }

            Ok(QueryTrackResult::Queued)
        }

        pub fn sync_volume(&self) {
            if let Some(playback) = &self.playback {
                set_volume(&playback.player, *self.volume.borrow());
            }
        }
    }

    fn set_volume(player: &rodio::Player, volume: f32) {
        let volume = volume.clamp(0.0, 1.0).powi(3);
        player.set_volume(volume);
    }

    fn open_default_stream(sample_rate: NonZero<u32>) -> AppResult<rodio::MixerDeviceSink> {
        rodio::DeviceSinkBuilder::from_default_device()
            .and_then(|builder| {
                builder
                    .with_sample_rate(sample_rate)
                    .with_error_callback(|err| tracing::warn!("Audio stream error: {err}"))
                    .open_stream()
            })
            .or_else(|original_err| {
                let mut devices = rodio::cpal::default_host().output_devices()?;

                Ok(devices
                    .find_map(|device| {
                        rodio::DeviceSinkBuilder::from_device(device)
                            .and_then(|builder| {
                                builder
                                    .with_sample_rate(sample_rate)
                                    .with_error_callback(|err| {
                                        tracing::warn!("Audio stream error: {err}");
                                    })
                                    .open_sink_or_fallback()
                            })
                            .ok()
                    })
                    .ok_or(original_err)?)
            })
    }

    fn open_preferred_stream(
        sample_rate: NonZero<u32>,
        preferred_device_name: &str,
    ) -> AppResult<rodio::MixerDeviceSink> {
        let devices = rodio::cpal::default_host().output_devices()?;

        for device in devices {
            if device.description().map(|x| x.to_string()).ok().as_deref()
                == Some(preferred_device_name)
            {
                let Ok(stream) =
                    rodio::DeviceSinkBuilder::from_device(device).and_then(|builder| {
                        builder
                            .with_sample_rate(sample_rate)
                            .with_error_callback(|err| {
                                tracing::warn!("Audio stream error: {err}");
                            })
                            .open_sink_or_fallback()
                    })
                else {
                    break;
                };

                return Ok(stream);
            }
        }

        let devices = rodio::cpal::default_host().output_devices()?;

        let available_devices: Vec<String> = devices
            .flat_map(|device| device.description().map(|x| x.to_string()))
            .collect();

        Err(PlayerError::SinkDeviceError {
            message: format!(
                "Unable to find device. Available devices: {}",
                available_devices.join(", ")
            ),
        })
    }

    impl Drop for Sink {
        fn drop(&mut self) {
            self.clear();
        }
    }
}

#[cfg(feature = "mpv")]
mod implementation {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use controls_module::VolumeReceiver;
    use libmpv2::Mpv;
    use tokio::sync::mpsc::UnboundedSender;
    use tokio::task::JoinHandle;

    use crate::AppResult;
    use crate::error::PlayerError;
    use crate::sink::{QueryTrackResult, TrackFinished};

    use tokio::time::sleep;

    struct Playback {
        mpv: Arc<Mpv>,
    }

    pub struct Sink {
        playback: Option<Playback>,
        volume: VolumeReceiver,
        track_finished_tx: UnboundedSender<TrackFinished>,
        current_track_handle: Option<JoinHandle<()>>,
        queued_track_handle: Option<JoinHandle<()>>,
        playback_generation: Arc<AtomicUsize>,
        preferred_device_id: Option<String>,
    }

    impl Sink {
        pub fn new(
            volume: VolumeReceiver,
            preferred_device_id: Option<String>,
            track_finished_tx: UnboundedSender<TrackFinished>,
        ) -> Self {
            Self {
                playback: None,
                volume,
                track_finished_tx,
                current_track_handle: None,
                queued_track_handle: None,
                playback_generation: Arc::new(AtomicUsize::new(0)),
                preferred_device_id,
            }
        }

        pub fn playback_generation(&self) -> usize {
            self.playback_generation.load(Ordering::SeqCst)
        }

        pub fn position(&self) -> Duration {
            let Some(playback) = &self.playback else {
                return Duration::default();
            };

            let Ok(position) = playback.mpv.get_property::<f64>("time-pos") else {
                return Duration::default();
            };

            if !position.is_finite() || position <= 0.0 {
                return Duration::default();
            }

            Duration::from_secs_f64(position)
        }

        pub fn play(&self) {
            if let Some(playback) = &self.playback
                && let Err(err) = playback.mpv.set_property("pause", false)
            {
                tracing::warn!("mpv play error: {err:?}");
            }
        }

        pub fn pause(&self) {
            if let Some(playback) = &self.playback
                && let Err(err) = playback.mpv.set_property("pause", true)
            {
                tracing::warn!("mpv pause error: {err:?}");
            }
        }

        pub fn seek(&self, duration: Duration) -> AppResult<()> {
            let Some(playback) = &self.playback else {
                return Ok(());
            };

            playback
                .mpv
                .set_property("time-pos", duration.as_secs_f64())
                .map_err(|err| PlayerError::StreamError {
                    message: format!("mpv seek error: {err:?}"),
                })
        }

        pub fn clear(&mut self) {
            tracing::info!("Clearing sink");

            self.playback_generation.fetch_add(1, Ordering::SeqCst);

            if let Some(handle) = self.current_track_handle.take() {
                handle.abort();
            }

            if let Some(handle) = self.queued_track_handle.take() {
                handle.abort();
            }

            if let Some(playback) = self.playback.take() {
                if let Err(err) = playback.mpv.command("stop", &[]) {
                    tracing::warn!("mpv stop error: {err:?}");
                }

                if let Err(err) = playback.mpv.command("playlist-clear", &[]) {
                    tracing::warn!("mpv playlist clear error: {err:?}");
                }
            }
        }

        pub fn clear_queue(&mut self) {
            tracing::info!("Clearing sink queue");

            if let Some(handle) = self.queued_track_handle.take() {
                handle.abort();
            }

            self.fun_name();
        }

        fn fun_name(&mut self) {
            if let Some(playback) = &self.playback
                && let Err(err) = playback.mpv.command("playlist-clear", &[])
            {
                tracing::warn!("mpv playlist clear error: {err:?}");
            }
        }

        pub fn promote_queued_track(&mut self) {
            self.current_track_handle = self.queued_track_handle.take();
        }

        pub const fn is_empty(&self) -> bool {
            self.playback.is_none()
        }

        pub fn query_track(
            &mut self,
            track_path: &Path,
            queue_id: u64,
        ) -> AppResult<QueryTrackResult> {
            tracing::info!("Sink query track: {}", track_path.to_string_lossy());

            if !track_path.is_file() {
                return Err(PlayerError::StreamError {
                    message: format!("Failed to read file: {}", track_path.display()),
                });
            }

            let path = track_path
                .to_str()
                .ok_or_else(|| PlayerError::StreamError {
                    message: format!("Track path is not valid UTF-8: {}", track_path.display()),
                })?;

            let is_queued_track = self.playback.is_some();

            if self.playback.is_none() {
                let mpv = Mpv::new().map_err(|err| PlayerError::SinkDeviceError {
                    message: format!("Failed to initialize mpv: {err:?}"),
                })?;

                mpv.set_property("vid", "no")
                    .map_err(|err| PlayerError::SinkDeviceError {
                        message: format!("Failed to disable mpv video output: {err:?}"),
                    })?;

                mpv.set_property("audio-display", "no").map_err(|err| {
                    PlayerError::SinkDeviceError {
                        message: format!("Failed to disable mpv album-art display: {err:?}"),
                    }
                })?;

                mpv.set_property("gapless-audio", "yes").map_err(|err| {
                    PlayerError::SinkDeviceError {
                        message: format!("Failed to enable mpv gapless playback: {err:?}"),
                    }
                })?;

                if let Some(device_id) = self.preferred_device_id.as_deref() {
                    mpv.set_property("audio-device", device_id).map_err(|err| {
                        PlayerError::SinkDeviceError {
                            message: format!(
                                "Failed to select mpv audio device {device_id}: {err:?}"
                            ),
                        }
                    })?;
                }

                set_volume(&mpv, *self.volume.borrow())?;

                self.playback = Some(Playback { mpv: Arc::new(mpv) });
            }

            let playback = self.playback.as_ref().ok_or(PlayerError::SinkDeviceError {
                message: "Playback not initialized".to_string(),
            })?;

            let load_mode = if is_queued_track { "append" } else { "replace" };

            playback
                .mpv
                .command("loadfile", &[path, load_mode])
                .map_err(|err| PlayerError::StreamError {
                    message: format!("Failed to load {} with mpv: {err:?}", track_path.display()),
                })?;

            let target_playlist_position = i64::from(is_queued_track);
            let mpv = playback.mpv.clone();
            let track_finished_tx = self.track_finished_tx.clone();
            let playback_generation = self.playback_generation.clone();
            let expected_generation = playback_generation.load(Ordering::SeqCst);

            let handle = tokio::spawn(async move {
                let mut observed_track = false;

                loop {
                    if playback_generation.load(Ordering::SeqCst) != expected_generation {
                        tracing::debug!(queue_id, "Ignoring obsolete track-finished signal");
                        return;
                    }

                    let playlist_position = mpv.get_property::<i64>("playlist-pos").ok();
                    let idle = mpv.get_property::<bool>("idle-active").unwrap_or(false);

                    if playlist_position == Some(target_playlist_position) {
                        observed_track = true;
                    }

                    let finished = observed_track
                        && (idle
                            || playlist_position.is_none()
                            || playlist_position
                                .is_some_and(|position| position > target_playlist_position));

                    if finished {
                        if playback_generation.load(Ordering::SeqCst) != expected_generation {
                            tracing::debug!(queue_id, "Ignoring obsolete track-finished signal");
                            return;
                        }

                        let _ = track_finished_tx.send(TrackFinished {
                            generation: expected_generation,
                            queue_id,
                        });

                        return;
                    }

                    sleep(Duration::from_millis(100)).await;
                }
            });

            if is_queued_track {
                if let Some(old_handle) = self.queued_track_handle.replace(handle) {
                    old_handle.abort();
                }
            } else if let Some(old_handle) = self.current_track_handle.replace(handle) {
                old_handle.abort();
            }

            Ok(QueryTrackResult::Queued)
        }

        pub fn sync_volume(&self) {
            if let Some(playback) = &self.playback
                && let Err(err) = set_volume(&playback.mpv, *self.volume.borrow())
            {
                tracing::warn!("mpv volume error: {err:?}");
            }
        }
    }

    fn set_volume(mpv: &Mpv, volume: f32) -> AppResult<()> {
        let volume = f64::from(volume.clamp(0.0, 1.0).powi(3)) * 100.0;

        mpv.set_property("volume", volume)
            .map_err(|err| PlayerError::SinkDeviceError {
                message: format!("Failed to set mpv volume: {err:?}"),
            })
    }

    impl Drop for Sink {
        fn drop(&mut self) {
            self.clear();
        }
    }
}

pub use implementation::Sink;
