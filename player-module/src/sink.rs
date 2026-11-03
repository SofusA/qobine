use std::fs;
use std::io::{Read, Seek};
use std::num::NonZero;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use controls_module::VolumeReceiver;
use parking_lot::Mutex;
use qobuz_client::stream::flac_source_stream::SeekableStreamReader;
use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::queue::queue;
use rodio::{Decoder, Player, Source};
use tokio::sync::watch::{self, Receiver, Sender};
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::AppResult;
use crate::error::PlayerError;
use crate::prefetch::Prefetch;
use crate::stderr_redirect::silence_stderr;

struct Playback {
    player: Player,
    output_stream: rodio::MixerDeviceSink,
    sender: Arc<rodio::queue::SourcesQueueInput>,
}

pub struct Sink {
    playback: Option<Playback>,
    volume: VolumeReceiver,
    track_finished: Sender<()>,
    track_handle: Option<JoinHandle<()>>,
    duration_played: Arc<Mutex<Duration>>,
    playback_generation: Arc<AtomicUsize>,
    preferred_device_id: Option<String>,
}

impl Sink {
    pub fn new(volume: VolumeReceiver, preferred_device_id: Option<String>) -> Self {
        let (track_finished, _) = watch::channel(());

        Self {
            playback: None,
            volume,
            track_finished,
            track_handle: None,
            duration_played: Arc::default(),
            playback_generation: Arc::new(AtomicUsize::new(0)),
            preferred_device_id,
        }
    }

    pub fn track_finished(&self) -> Receiver<()> {
        self.track_finished.subscribe()
    }

    pub fn position(&self) -> Duration {
        let position = self
            .playback
            .as_ref()
            .map(|playback| &playback.player)
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

        self.clear_queue();
        self.playback = None;

        *self.duration_played.lock() = Duration::default();

        if let Some(handle) = self.track_handle.take() {
            handle.abort();
        }
    }

    pub fn clear_queue(&mut self) {
        tracing::info!("Clearing sink queue");

        if let Some(playback) = self.playback.as_ref() {
            playback.sender.clear();
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.playback.is_none()
    }

    pub fn query_track(&mut self, track_path: &Path) -> AppResult<QueryTrackResult> {
        tracing::info!("Sink query track: {}", track_path.to_string_lossy());

        let file = fs::File::open(track_path).map_err(|err| PlayerError::StreamError {
            message: format!("Failed to read file: {}: {err}", track_path.display()),
        })?;

        let source = Decoder::try_from(file)?;
        self.queue_decoder(source)
    }

    pub fn query_track_stream(
        &mut self,
        reader: SeekableStreamReader,
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

        self.queue_decoder(source)
    }

    fn queue_decoder<R: Read + Seek + Send + Sync + 'static>(
        &mut self,
        source: Decoder<R>,
    ) -> AppResult<QueryTrackResult> {
        let sample_rate = source.sample_rate();

        let same_sample_rate = self
            .playback
            .as_ref()
            .is_none_or(|playback| playback.output_stream.config().sample_rate() == sample_rate);

        if !same_sample_rate {
            return Ok(QueryTrackResult::RecreateStreamRequired);
        }

        if self.playback.is_none() {
            let mut mixer = if let Some(preferred_device_id) = self.preferred_device_id.as_deref() {
                silence_stderr(|| open_preferred_stream(sample_rate, preferred_device_id))?
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

        let track_finished = self.track_finished.clone();
        let track_duration = source.total_duration().unwrap_or_default();
        let duration_played = self.duration_played.clone();

        let playback_generation = self.playback_generation.clone();
        let expected_generation = playback_generation.load(Ordering::SeqCst);

        let signal = playback.sender.append_with_signal(Prefetch::new(source));

        let track_handle = tokio::spawn(async move {
            loop {
                if signal.try_recv().is_ok() {
                    if playback_generation.load(Ordering::SeqCst) != expected_generation {
                        tracing::debug!(
                            "Ignoring track-finished signal from obsolete playback generation"
                        );
                        break;
                    }

                    {
                        let mut duration_played = duration_played.lock();
                        *duration_played = duration_played.saturating_add(track_duration);
                    }

                    let _ = track_finished.send(());
                    break;
                }

                sleep(Duration::from_millis(200)).await;
            }
        });

        self.track_handle = Some(track_handle);

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

fn open_preferred_stream(
    sample_rate: NonZero<u32>,
    preferred_device_id: &str,
) -> AppResult<rodio::MixerDeviceSink> {
    let device_id = cpal::DeviceId::from_str(preferred_device_id).map_err(|err| {
        PlayerError::SinkDeviceError {
            message: format!(
                "Invalid preferred audio device ID \
                     '{preferred_device_id}': {err}"
            ),
        }
    })?;

    let host_id = device_id.host();

    let host = cpal::host_from_id(host_id).map_err(|err| PlayerError::SinkDeviceError {
        message: format!(
            "Unable to initialize audio host '{host_id}' for preferred \
                 device '{preferred_device_id}': {err}"
        ),
    })?;

    let device = host
        .device_by_id(&device_id)
        .ok_or_else(|| PlayerError::SinkDeviceError {
            message: format!(
                "Preferred audio device '{preferred_device_id}' is not \
                 available through host '{host_id}'"
            ),
        })?;

    let description = device_description(&device);

    tracing::info!(
        host = %host_id,
        device_id = %preferred_device_id,
        device = %description,
        sample_rate = sample_rate.get(),
        "Opening preferred audio output"
    );

    open_device_stream(device, sample_rate)
}

fn open_default_stream(sample_rate: NonZero<u32>) -> AppResult<rodio::MixerDeviceSink> {
    let host = preferred_host();

    tracing::info!(
        host = %host.id(),
        sample_rate = sample_rate.get(),
        "Opening automatically selected audio output"
    );

    let default_device =
        host.default_output_device()
            .ok_or_else(|| PlayerError::SinkDeviceError {
                message: format!("Audio host '{}' has no default output device", host.id()),
            })?;

    let default_device_id = device_id_string(&default_device);
    let default_description = device_description(&default_device);

    tracing::info!(
        host = %host.id(),
        device_id = %default_device_id,
        device = %default_description,
        sample_rate = sample_rate.get(),
        "Trying default audio output device"
    );

    match open_device_stream(default_device, sample_rate) {
        Ok(stream) => Ok(stream),

        Err(original_err) => {
            tracing::warn!(
                host = %host.id(),
                sample_rate = sample_rate.get(),
                "Unable to open default audio output: {original_err}; \
                 trying the remaining devices"
            );

            for device in host.output_devices()? {
                let device_id = device_id_string(&device);
                let description = device_description(&device);

                match open_device_stream(device, sample_rate) {
                    Ok(stream) => {
                        tracing::info!(
                            host = %host.id(),
                            device_id = %device_id,
                            device = %description,
                            sample_rate = sample_rate.get(),
                            "Opened fallback audio output device"
                        );

                        return Ok(stream);
                    }

                    Err(err) => {
                        tracing::debug!(
                            host = %host.id(),
                            device_id = %device_id,
                            device = %description,
                            sample_rate = sample_rate.get(),
                            "Unable to open audio output device: {err}"
                        );
                    }
                }
            }

            Err(original_err)
        }
    }
}

fn open_device_stream(
    device: cpal::Device,
    sample_rate: NonZero<u32>,
) -> AppResult<rodio::MixerDeviceSink> {
    let stream = rodio::DeviceSinkBuilder::from_device(device).and_then(|builder| {
        builder
            .with_sample_rate(sample_rate)
            .with_error_callback(|err| {
                tracing::warn!("Audio stream error: {err}");
            })
            .open_sink_or_fallback()
    })?;

    Ok(stream)
}

fn device_id_string(device: &cpal::Device) -> String {
    device
        .id()
        .map_or_else(|_| "<unknown>".to_string(), |id| id.to_string())
}

fn device_description(device: &cpal::Device) -> String {
    device.description().map_or_else(
        |_| "<unknown>".to_string(),
        |description| description.to_string(),
    )
}

fn preferred_host() -> cpal::Host {
    pipewire_host().unwrap_or_else(|| {
        let host = cpal::default_host();

        tracing::info!(
            host = %host.id(),
            "Using platform-default audio host"
        );

        host
    })
}

#[cfg(all(
    feature = "pipewire",
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd"
    )
))]
fn pipewire_host() -> Option<cpal::Host> {
    if !cpal::available_hosts().contains(&cpal::HostId::PipeWire) {
        tracing::debug!("Native PipeWire audio host was not compiled in or is unavailable");

        return None;
    }

    match cpal::host_from_id(cpal::HostId::PipeWire) {
        Ok(host) => {
            tracing::info!("Using native PipeWire audio host");
            Some(host)
        }

        Err(err) => {
            tracing::warn!(
                "Unable to initialize native PipeWire audio host: {err}; \
                 using platform-default host"
            );

            None
        }
    }
}

#[cfg(not(all(
    feature = "pipewire",
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd"
    )
)))]
fn pipewire_host() -> Option<cpal::Host> {
    None
}

pub enum QueryTrackResult {
    Queued,
    RecreateStreamRequired,
}

impl Drop for Sink {
    fn drop(&mut self) {
        self.clear();
    }
}
