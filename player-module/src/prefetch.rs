use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::thread;
use std::time::Duration;

use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

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
