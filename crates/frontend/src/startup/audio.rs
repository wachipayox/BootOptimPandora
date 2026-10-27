//! Vorbis decoding and the common playback boundary. No UI/rendering runs here.
use std::{
    io::Cursor,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

pub(super) struct Pcm {
    pub samples: Vec<i16>,
}

pub(super) fn decode(bytes: &[u8]) -> anyhow::Result<Pcm> {
    anyhow::ensure!(bytes.len() <= 8 * 1024 * 1024, "Intro track exceeds size limit");
    let mut reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes))?;
    let mut samples = Vec::new();
    while let Some(packet) = reader.read_dec_packet_itl()? {
        anyhow::ensure!(
            reader.ident_hdr.audio_channels == 2 && reader.ident_hdr.audio_sample_rate == 48_000,
            "Intro tracks must be stereo 48 kHz Ogg Vorbis"
        );
        anyhow::ensure!(samples.len() + packet.len() <= 48_000 * 2 * 12, "Intro track exceeds duration limit");
        samples.extend(packet);
    }
    anyhow::ensure!(!samples.is_empty(), "Intro track has no samples");
    Ok(Pcm { samples })
}

trait SoundOutput {
    fn play(
        self,
        pcm: Pcm,
        commands: mpsc::Receiver<Command>,
        clock: Arc<Mutex<Option<Instant>>>,
    ) -> anyhow::Result<()>;
}
enum Command {
    Start,
    Fade,
    Stop,
}

pub(super) struct Audio {
    commands: mpsc::Sender<Command>,
    pub clock: Arc<Mutex<Option<Instant>>>,
    failed: Arc<AtomicBool>,
}
impl Audio {
    pub fn prepare(bytes: Vec<u8>) -> Self {
        let (send, receive) = mpsc::channel();
        let clock = Arc::new(Mutex::new(None));
        let worker_clock = clock.clone();
        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        // COM and the audio device stay on this thread for their whole lifetime.
        let _ = std::thread::Builder::new().name("startup-audio".into()).spawn(move || {
            let result = (|| {
                let pcm = decode(&bytes)?;
                #[cfg(windows)]
                {
                    wasapi::Output::new()?.play(pcm, receive, worker_clock.clone())
                }
                #[cfg(not(windows))]
                {
                    Silent.play(pcm, receive, worker_clock.clone())
                }
            })();
            if let Err(error) = result {
                log::warn!("Intro music unavailable: {error}");
                // Failure does not start the intro before its first poster paint.
                // The UI chooses the silent clock when it actually starts.
                worker_failed.store(true, Ordering::Release);
            }
        });
        Self {
            commands: send,
            clock,
            failed,
        }
    }
    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub fn start(&self) {
        let _ = self.commands.send(Command::Start);
    }
    pub fn fade(&self) {
        let _ = self.commands.send(Command::Fade);
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Stop);
    }
}

#[cfg(not(windows))]
struct Silent;
#[cfg(not(windows))]
impl SoundOutput for Silent {
    fn play(self, _: Pcm, commands: mpsc::Receiver<Command>, clock: Arc<Mutex<Option<Instant>>>) -> anyhow::Result<()> {
        if matches!(commands.recv(), Ok(Command::Start)) {
            *clock.lock().unwrap() = Some(Instant::now());
        }
        Ok(())
    }
}

fn fade_gain(frame: usize) -> f32 {
    let frames = (48_000. * super::policy::AUDIO_FADE.as_secs_f32()) as usize;
    1. - (frame as f32 / frames as f32).min(1.)
}

#[cfg(windows)]
mod wasapi {
    use super::*;
    use windows::Win32::{Media::Audio::*, System::Com::*};

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }
    pub(super) struct Output {
        // Drop device interfaces before uninitializing COM.
        client: IAudioClient,
        render: IAudioRenderClient,
        _apartment: Apartment,
    }
    impl Output {
        pub fn new() -> anyhow::Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
                let apartment = Apartment;
                let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                let format = WAVEFORMATEX {
                    wFormatTag: 1,
                    nChannels: 2,
                    nSamplesPerSec: 48_000,
                    nAvgBytesPerSec: 192_000,
                    nBlockAlign: 4,
                    wBitsPerSample: 16,
                    cbSize: 0,
                };
                client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    200_000,
                    0,
                    &format,
                    None,
                )?;
                let render: IAudioRenderClient = client.GetService()?;
                Ok(Self {
                    client,
                    render,
                    _apartment: apartment,
                })
            }
        }
    }
    impl SoundOutput for Output {
        fn play(
            self,
            pcm: Pcm,
            commands: mpsc::Receiver<Command>,
            clock: Arc<Mutex<Option<Instant>>>,
        ) -> anyhow::Result<()> {
            if !matches!(commands.recv(), Ok(Command::Start)) {
                return Ok(());
            }
            unsafe {
                let capacity = self.client.GetBufferSize()?;
                let mut cursor = 0usize;
                let mut fade_frame = None;
                let mut started = false;
                let mut playback_started = None;
                loop {
                    while let Ok(command) = commands.try_recv() {
                        match command {
                            Command::Stop => {
                                self.client.Stop()?;
                                return Ok(());
                            },
                            Command::Fade => {
                                fade_frame.get_or_insert(cursor / 2);
                            },
                            Command::Start => {},
                        }
                    }
                    let padding = self.client.GetCurrentPadding()?;
                    let available = capacity.saturating_sub(padding).min(960);
                    let source_left = (pcm.samples.len().saturating_sub(cursor) / 2) as u32;
                    let frames = available.min(source_left);
                    if frames != 0 {
                        let buffer = self.render.GetBuffer(frames)? as *mut i16;
                        for frame in 0..frames as usize {
                            let gain = fade_frame.map(|start| fade_gain(cursor / 2 + frame - start)).unwrap_or(1.);
                            for channel in 0..2 {
                                *buffer.add(frame * 2 + channel) =
                                    (pcm.samples[cursor + frame * 2 + channel] as f32 * gain) as i16;
                            }
                        }
                        self.render.ReleaseBuffer(frames, 0)?;
                        cursor += frames as usize * 2;
                    }
                    if !started {
                        self.client.Start()?;
                        let start = Instant::now();
                        *clock.lock().unwrap() = Some(start);
                        playback_started = Some(start);
                        started = true;
                        log::info!("startup-audio-start pid={} queued_frames={}", std::process::id(), frames);
                    }
                    let faded = fade_frame.is_some_and(|start| fade_gain(cursor / 2 - start) == 0.);
                    if source_left == 0 || faded {
                        // Drain queued frames before Stop; the last fade sample is zero.
                        let drain_deadline = Instant::now() + Duration::from_millis(500);
                        while self.client.GetCurrentPadding()? != 0 && Instant::now() < drain_deadline {
                            if matches!(commands.try_recv(), Ok(Command::Stop)) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        self.client.Stop()?;
                        if let Some(start) = playback_started {
                            log::info!(
                                "startup-audio-finished pid={} playback_ms={}",
                                std::process::id(),
                                start.elapsed().as_millis()
                            );
                        }
                        return Ok(());
                    }
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, fade_gain};
    #[test]
    fn bad_or_non_vorbis_audio_is_silently_recoverable() {
        assert!(decode(b"").is_err());
        assert!(decode(b"OggS OpusHead").is_err());
    }
    #[test]
    fn decode_failure_cannot_start_the_intro_clock_before_the_poster() {
        let audio = super::Audio::prepare(Vec::new());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !audio.failed() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(audio.failed());
        assert!(audio.clock.lock().unwrap().is_none());
    }
    #[test]
    fn fade_has_monotone_gain_and_ends_at_150_ms() {
        assert_eq!(fade_gain(0), 1.);
        assert_eq!(fade_gain(3600), 0.5);
        assert_eq!(fade_gain(7200), 0.);
        assert_eq!(fade_gain(8000), 0.);
        assert!((0..7200).all(|f| fade_gain(f + 1) <= fade_gain(f)));
    }
}
