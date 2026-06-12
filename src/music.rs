use std::{
    f32::consts::TAU,
    fs,
    fs::File,
    io::Write,
    num::NonZero,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use rodio::{
    cpal::{
        self,
        traits::{DeviceTrait, HostTrait},
        BufferSize, StreamError,
    },
    Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source,
};
use serde::Deserialize;

use crate::error::{EutherError, Result};

const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const TRACKS: [CyberTrack; 10] = [
    CyberTrack::new("Midnight Uplink", 72.0, 41.2, [0, 3, 7, 10], 11),
    CyberTrack::new("Chrome Alley", 78.0, 43.65, [0, 2, 7, 9], 23),
    CyberTrack::new("Neon Rain", 68.0, 36.71, [0, 5, 7, 10], 37),
    CyberTrack::new("Null District", 84.0, 49.0, [0, 3, 8, 10], 41),
    CyberTrack::new("Battery Shrine", 74.0, 38.89, [0, 4, 7, 11], 53),
    CyberTrack::new("Ghost Terminal", 80.0, 46.25, [0, 3, 5, 10], 67),
    CyberTrack::new("Vapor Circuit", 66.0, 34.65, [0, 2, 5, 9], 79),
    CyberTrack::new("Ion Market", 86.0, 51.91, [0, 3, 7, 12], 83),
    CyberTrack::new("Blackout Metro", 70.0, 30.87, [0, 5, 8, 10], 97),
    CyberTrack::new("Synthetic Dawn", 76.0, 55.0, [0, 4, 7, 10], 109),
];

pub struct AudioEngine {
    device_sink: Option<MixerDeviceSink>,
    player: Option<Player>,
    pipewire_child: Option<Child>,
    mpv_socket: Option<PathBuf>,
    mpv_applied_volume: Option<f32>,
    pipewire_started_at: Option<Instant>,
    pipewire_track_duration: Option<Duration>,
    track_index: usize,
    file_tracks: Vec<MusicTrack>,
    current_name: String,
    volume: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct CyberTrack {
    name: &'static str,
    bpm: f32,
    root: f32,
    scale: [i32; 4],
    seed: u32,
}

#[derive(Debug, Clone)]
struct CyberSource {
    track: CyberTrack,
    frame: u64,
    channel: u16,
}

#[derive(Debug, Clone, Deserialize)]
struct MusicManifest {
    #[serde(default)]
    track: Vec<MusicTrack>,
}

#[derive(Debug, Clone, Deserialize)]
struct MusicTrack {
    title: String,
    #[serde(default)]
    author: Option<String>,
    file: PathBuf,
    license: String,
    source: String,
    #[serde(default)]
    start_offset_seconds: f32,
}

impl AudioEngine {
    pub fn start_random(volume: f32) -> Result<Self> {
        let file_tracks = load_music_tracks();
        let track_count = file_tracks.len().max(TRACKS.len());
        let (device_sink, player) = if file_tracks.is_empty() {
            let mut device_sink = open_audio_sink()?;
            device_sink.log_on_drop(false);
            log_music_event(format!("sink_opened: config={:?}", device_sink.config()));
            let player = Player::connect_new(device_sink.mixer());
            (Some(device_sink), Some(player))
        } else {
            log_music_event("pipewire_primary: true");
            (None, None)
        };
        let mut engine = Self {
            device_sink,
            player,
            pipewire_child: None,
            mpv_socket: None,
            mpv_applied_volume: None,
            pipewire_started_at: None,
            pipewire_track_duration: None,
            track_index: random_index(track_count),
            file_tracks,
            current_name: String::new(),
            volume: volume.clamp(0.0, 1.0),
        };
        engine.restart_current_track();
        Ok(engine)
    }

    pub fn next_track(&mut self) {
        let track_count = self.file_tracks.len().max(TRACKS.len());
        self.track_index = (self.track_index + 1 + random_index(track_count)) % track_count;
        self.restart_current_track();
    }

    pub fn track_name(&self) -> &str {
        &self.current_name
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(player) = &self.player {
            player.set_volume(self.volume);
        }
        log_music_event(format!("set_volume: {:.2}", self.volume));
        if self.pipewire_child.is_some() {
            self.apply_mpv_volume();
        }
    }

    pub fn tick(&mut self) {
        let Some(child) = &mut self.pipewire_child else {
            return;
        };

        let finished = match child.try_wait() {
            Ok(Some(status)) => {
                log_music_event(format!("mpv_exited: {status}"));
                if self
                    .pipewire_started_at
                    .is_some_and(|started_at| started_at.elapsed() < Duration::from_secs(1))
                {
                    self.pipewire_child = None;
                    self.current_name = "Music playback failed".to_string();
                    return;
                }
                true
            }
            Ok(None) => self
                .pipewire_started_at
                .zip(self.pipewire_track_duration)
                .is_some_and(|(started_at, duration)| started_at.elapsed() >= duration),
            Err(err) => {
                log_music_event(format!("mpv_poll_failed: {err}"));
                true
            }
        };

        if finished {
            self.pipewire_child = None;
            self.restart_current_track();
        } else {
            self.apply_mpv_volume();
        }
    }

    pub fn shutdown(&mut self) {
        log_music_event("shutdown");
        self.stop_pipewire_child();
        if let Some(player) = &self.player {
            player.set_volume(0.0);
            player.stop();
        }
    }

    fn restart_current_track(&mut self) {
        self.stop_pipewire_child();
        if let Some(player) = &self.player {
            player.stop();
        }

        if !self.file_tracks.is_empty() {
            let track = self.file_tracks[self.track_index % self.file_tracks.len()].clone();
            match File::open(&track.file)
                .map_err(|err| err.to_string())
                .and_then(|file| Decoder::try_from(file).map_err(|err| err.to_string()))
            {
                Ok(source) => {
                    let author = track.author.as_deref().unwrap_or("unknown artist");
                    self.current_name = format!(
                        "{} - {} ({}, {})",
                        track.title,
                        author,
                        track.license,
                        source_label(&track.source)
                    );
                    let offset = Duration::from_secs_f32(track.start_offset_seconds.max(0.0));
                    log_music_event(format!(
                        "start_file: path={} volume={:.2} offset={:.2}",
                        track.file.display(),
                        self.volume,
                        track.start_offset_seconds.max(0.0)
                    ));
                    if self.try_start_mpv_track(&track) {
                        return;
                    }
                    if self.ensure_rodio().is_err() {
                        return;
                    }
                    let Some(player) = &self.player else {
                        return;
                    };
                    player.set_volume(self.volume);
                    player.append(ProbeSource::new(
                        source.skip_duration(offset).repeat_infinite(),
                        "file",
                    ));
                }
                Err(err) => {
                    log_music_event(format!(
                        "file_decode_failed: path={} error={err}",
                        track.file.display()
                    ));
                    if self.ensure_rodio().is_err() {
                        self.current_name = "Audio unavailable".to_string();
                        return;
                    }
                    self.restart_procedural();
                }
            }
        } else {
            if self.ensure_rodio().is_err() {
                self.current_name = "Audio unavailable".to_string();
                return;
            }
            self.restart_procedural();
        }

        if let Some(player) = &self.player {
            player.play();
            player.set_volume(self.volume);
        }
    }

    fn restart_procedural(&mut self) {
        let track = TRACKS[self.track_index % TRACKS.len()];
        self.current_name = format!("{} (generated)", track.name);
        log_music_event(format!(
            "start_procedural: track={} volume={:.2}",
            track.name, self.volume
        ));
        if let Some(player) = &self.player {
            player.append(ProbeSource::new(CyberSource::new(track), "procedural"));
        }
    }

    fn ensure_rodio(&mut self) -> Result<()> {
        if let (Some(device_sink), Some(_)) = (&self.device_sink, &self.player) {
            let mixer = device_sink.mixer();
            self.player = Some(Player::connect_new(mixer));
            return Ok(());
        }

        let mut device_sink = open_audio_sink()?;
        device_sink.log_on_drop(false);
        log_music_event(format!("sink_opened: config={:?}", device_sink.config()));
        let player = Player::connect_new(device_sink.mixer());
        self.device_sink = Some(device_sink);
        self.player = Some(player);
        Ok(())
    }

    fn try_start_mpv_track(&mut self, track: &MusicTrack) -> bool {
        let volume = format!("{:.0}", self.volume.clamp(0.0, 1.0) * 100.0);
        let socket = mpv_socket_path();
        let _ = fs::remove_file(&socket);
        let child = Command::new("mpv")
            .arg("--no-video")
            .arg("--really-quiet")
            .arg("--ao=pipewire")
            .arg("--loop-file=inf")
            .arg("--input-terminal=no")
            .arg(format!("--input-ipc-server={}", socket.display()))
            .arg(format!("--volume={volume}"))
            .arg(&track.file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(child) => {
                let pid = child.id();
                log_music_event(format!(
                    "mpv_started: pid={} path={} volume={volume}",
                    pid,
                    track.file.display()
                ));
                self.pipewire_child = Some(child);
                self.mpv_socket = Some(socket);
                self.mpv_applied_volume = None;
                self.pipewire_started_at = Some(Instant::now());
                self.pipewire_track_duration = None;
                self.apply_mpv_volume();
                true
            }
            Err(err) => {
                log_music_event(format!("mpv_start_failed: {err}"));
                false
            }
        }
    }

    fn stop_pipewire_child(&mut self) {
        if let Some(mut child) = self.pipewire_child.take() {
            log_music_event(format!("mpv_stop: pid={}", child.id()));
            let _ = child.kill();
            let _ = child.wait();
        }
        self.pipewire_started_at = None;
        self.pipewire_track_duration = None;
        if let Some(socket) = self.mpv_socket.take() {
            let _ = fs::remove_file(socket);
        }
        self.mpv_applied_volume = None;
    }

    fn apply_mpv_volume(&mut self) {
        let Some(socket) = &self.mpv_socket else {
            return;
        };
        let volume = self.volume.clamp(0.0, 1.0) * 100.0;
        if self
            .mpv_applied_volume
            .is_some_and(|applied| (applied - volume).abs() < 0.5)
        {
            return;
        }
        match UnixStream::connect(socket) {
            Ok(mut stream) => {
                let command = serde_json::json!({
                    "command": ["set_property", "volume", volume]
                });
                if writeln!(stream, "{command}").is_ok() {
                    self.mpv_applied_volume = Some(volume);
                    log_music_event(format!("mpv_volume_set: volume={volume:.0}"));
                }
            }
            Err(err) => {
                log_music_event(format!("mpv_volume_pending: {err}"));
            }
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn default_music_volume() -> f32 {
    0.12
}

fn log_music_event(message: impl AsRef<str>) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/eutheretcher-audio.log")
    {
        let _ = writeln!(file, "{}", message.as_ref());
    }
}

fn audio_stream_error(err: StreamError) {
    log_music_event(format!("stream_error: {err}"));
}

fn mpv_socket_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    PathBuf::from(format!(
        "/tmp/eutheretcher-mpv-{}-{nanos}.sock",
        std::process::id()
    ))
}

fn open_audio_sink() -> Result<MixerDeviceSink> {
    let host = cpal::default_host();
    log_music_event(format!("audio_host: {}", host.id().name()));

    let device = host
        .default_output_device()
        .ok_or_else(|| EutherError::Audio("No default output audio device".to_string()))?;
    match device.description() {
        Ok(description) => log_music_event(format!("audio_device: {description}")),
        Err(err) => log_music_event(format!("audio_device_description_error: {err}")),
    }

    DeviceSinkBuilder::from_device(device)
        .map(|builder| builder.with_error_callback(audio_stream_error))
        .map(|builder| builder.with_buffer_size(BufferSize::Fixed(2048)))
        .and_then(|builder| builder.open_sink_or_fallback())
        .map_err(|err| EutherError::Audio(err.to_string()))
}

#[derive(Debug, Clone)]
struct ProbeSource<S> {
    inner: S,
    label: &'static str,
    samples_seen: u64,
    logged_first: bool,
    logged_half_second: bool,
}

impl<S> ProbeSource<S> {
    fn new(inner: S, label: &'static str) -> Self {
        Self {
            inner,
            label,
            samples_seen: 0,
            logged_first: false,
            logged_half_second: false,
        }
    }
}

impl<S> Iterator for ProbeSource<S>
where
    S: Source,
{
    type Item = S::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next();
        if sample.is_some() {
            self.samples_seen = self.samples_seen.saturating_add(1);
            if !self.logged_first {
                self.logged_first = true;
                log_music_event(format!("source_pulled_first: {}", self.label));
            }
            let half_second_samples = u64::from(self.inner.sample_rate().get())
                * u64::from(self.inner.channels().get())
                / 2;
            if !self.logged_half_second && self.samples_seen >= half_second_samples {
                self.logged_half_second = true;
                log_music_event(format!(
                    "source_pulled_500ms: {} samples={}",
                    self.label, self.samples_seen
                ));
            }
        }
        sample
    }
}

impl<S> Source for ProbeSource<S>
where
    S: Source,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> NonZero<u16> {
        self.inner.channels()
    }

    fn sample_rate(&self) -> NonZero<u32> {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

impl CyberTrack {
    const fn new(name: &'static str, bpm: f32, root: f32, scale: [i32; 4], seed: u32) -> Self {
        Self {
            name,
            bpm,
            root,
            scale,
            seed,
        }
    }
}

impl CyberSource {
    fn new(track: CyberTrack) -> Self {
        Self {
            track,
            frame: 0,
            channel: 0,
        }
    }

    fn sample(&self) -> f32 {
        let seconds = self.frame as f32 / SAMPLE_RATE as f32;
        let beat = seconds * self.track.bpm / 60.0;
        let global_step = (beat * 2.0).floor() as usize;
        let step = global_step % 16;
        let long_step = global_step % 128;
        let bar = (global_step / 16) % 8;
        let step_phase = (beat * 2.0).fract();
        let bar_phase = (beat / 4.0).fract();
        let phrase_phase = ((beat / 64.0).fract() * TAU).sin();
        let root = self.track.root;

        let progression = [
            self.track.scale[0],
            self.track.scale[1],
            self.track.scale[2],
            self.track.scale[(bar + 1) % self.track.scale.len()],
            self.track.scale[0] - 12,
            self.track.scale[3],
            self.track.scale[1],
            self.track.scale[2] + 12,
        ];
        let chord_root = progression[bar];
        let bass_pattern = [0, 0, 0, 7, 10, 10, 7, 3, 0, 0, 5, 5, 7, 7, 10, 7];
        let bass_step = bass_pattern[(step + bar * 3 + self.track.seed as usize) % 16] + chord_root;
        let arp_index = (long_step + bar + self.track.seed as usize) % self.track.scale.len();
        let arp_step = self.track.scale[arp_index] + 12 + if bar >= 4 { 12 } else { 0 };
        let lead_step = self.track.scale[(long_step + bar * 2) % self.track.scale.len()]
            + 24
            + if long_step.is_multiple_of(23) { 7 } else { 0 };

        let swell = 0.48 + 0.22 * phrase_phase.abs();
        let bass = sine(seconds, semitone(root, bass_step - 12), 0.16)
            * smooth_gate(step_phase, 0.08, 0.84);
        let sub = sine(seconds, semitone(root, chord_root - 24), 0.1) * swell;
        let pad = chord_pad(seconds, root, chord_root, &self.track.scale, bar) * swell;

        let arp_gate = if long_step.is_multiple_of(2) {
            0.18
        } else {
            0.0
        };
        let arp = triangle(seconds, semitone(root, arp_step), 0.12)
            * smooth_gate(step_phase, 0.1, 0.52)
            * arp_gate;
        let lead_gate = if bar >= 3 && long_step % 8 == 2 {
            0.12
        } else {
            0.0
        };
        let lead = triangle(seconds, semitone(root, lead_step), 0.14)
            * smooth_gate(step_phase, 0.18, 0.72)
            * lead_gate;
        let echo = triangle(
            seconds - 0.19,
            semitone(root, lead_step - 12 + (bar % 2) as i32 * 7),
            0.06,
        ) * smooth_gate((step_phase + 0.42).fract(), 0.2, 0.75);

        let kick = soft_kick(step_phase, step.is_multiple_of(8)) * 0.22;
        let hat_open = long_step % 16 == 14;
        let hat = soft_noise(self.frame ^ self.track.seed as u64)
            * smooth_gate(step_phase, 0.05, if hat_open { 0.72 } else { 0.18 })
            * if hat_open { 0.025 } else { 0.012 };
        let snare = noise(self.frame.wrapping_mul(31 + self.track.seed as u64))
            * soft_kick(step_phase, step == 8)
            * 0.035;
        let air = soft_noise(self.frame.wrapping_mul(17 + self.track.seed as u64))
            * (0.012 + 0.008 * (bar_phase * TAU).sin().abs());
        let fill = if long_step == 127 {
            triangle(seconds, semitone(root, chord_root + 19), 0.04)
                * smooth_gate(step_phase, 0.25, 0.9)
        } else {
            0.0
        };

        soft_clip(bass + sub + pad + arp + lead + echo + kick + hat + snare + air + fill) * 0.62
    }
}

impl Iterator for CyberSource {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let pan = if self.channel == 0 { 0.92 } else { 1.0 };
        let sample = self.sample() * pan;

        self.channel += 1;
        if self.channel >= CHANNELS {
            self.channel = 0;
            self.frame = self.frame.wrapping_add(1);
        }

        Some(sample)
    }
}

impl Source for CyberSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZero<u16> {
        NonZero::new(CHANNELS).expect("CHANNELS is non-zero")
    }

    fn sample_rate(&self) -> NonZero<u32> {
        NonZero::new(SAMPLE_RATE).expect("SAMPLE_RATE is non-zero")
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn semitone(root: f32, offset: i32) -> f32 {
    root * 2_f32.powf(offset as f32 / 12.0)
}

fn sine(seconds: f32, frequency: f32, amount: f32) -> f32 {
    (seconds * frequency * TAU).sin() * amount
}

fn triangle(seconds: f32, frequency: f32, amount: f32) -> f32 {
    let phase = (seconds * frequency).fract();
    let value = if phase < 0.5 {
        phase * 4.0 - 1.0
    } else {
        3.0 - phase * 4.0
    };
    value * amount
}

fn chord_pad(seconds: f32, root: f32, chord_root: i32, scale: &[i32; 4], bar: usize) -> f32 {
    let chord = [
        chord_root,
        chord_root + scale[(bar + 1) % scale.len()],
        chord_root + scale[(bar + 2) % scale.len()] + 12,
    ];
    chord
        .into_iter()
        .enumerate()
        .map(|(index, note)| {
            let drift = 1.0 + (index as f32 - 1.0) * 0.003;
            sine(seconds, semitone(root, note + 12) * drift, 0.07)
                + triangle(seconds, semitone(root, note + 24) * drift, 0.025)
        })
        .sum::<f32>()
}

fn smooth_gate(phase: f32, attack: f32, release: f32) -> f32 {
    if phase < attack {
        (phase / attack).clamp(0.0, 1.0)
    } else if phase > release {
        (1.0 - (phase - release) / (1.0 - release)).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn soft_kick(phase: f32, enabled: bool) -> f32 {
    if !enabled || phase > 0.42 {
        0.0
    } else {
        let envelope = (1.0 - phase / 0.42).powf(2.4);
        envelope * (phase * TAU * 9.0).sin()
    }
}

fn soft_noise(frame: u64) -> f32 {
    noise(frame) * 0.65 + noise(frame / 3) * 0.35
}

fn noise(frame: u64) -> f32 {
    let mut value = frame
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd);
    (((value >> 32) as u32 as f32 / u32::MAX as f32) * 2.0) - 1.0
}

fn soft_clip(value: f32) -> f32 {
    value / (1.0 + value.abs())
}

fn load_music_tracks() -> Vec<MusicTrack> {
    for manifest_path in music_manifest_paths() {
        let Ok(data) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let Ok(manifest) = toml::from_str::<MusicManifest>(&data) else {
            continue;
        };
        let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let tracks = manifest
            .track
            .into_iter()
            .filter_map(|track| resolve_music_track(manifest_dir, track))
            .collect::<Vec<_>>();
        if !tracks.is_empty() {
            return tracks;
        }
    }

    Vec::new()
}

fn music_manifest_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    paths.push(PathBuf::from("assets/music/music.toml"));

    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        paths.push(PathBuf::from(data_home).join("eutheretcher/music/music.toml"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".local/share/eutheretcher/music/music.toml"));
    }

    paths.push(PathBuf::from(
        "/usr/local/share/eutheretcher/music/music.toml",
    ));
    paths.push(PathBuf::from("/usr/share/eutheretcher/music/music.toml"));

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            paths.push(exe_dir.join("assets/music/music.toml"));
            paths.push(exe_dir.join("../share/eutheretcher/music/music.toml"));
        }
    }

    paths
}

fn resolve_music_track(manifest_dir: &Path, mut track: MusicTrack) -> Option<MusicTrack> {
    let resolved = if track.file.is_absolute() {
        track.file.clone()
    } else {
        manifest_dir.join(&track.file)
    };

    if resolved.exists() {
        track.file = resolved;
        Some(track)
    } else if track.file.exists() {
        Some(track)
    } else {
        None
    }
}

fn source_label(source: &str) -> &str {
    source
        .strip_prefix("https://")
        .or_else(|| source.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
        .filter(|host| !host.is_empty())
        .unwrap_or(source)
}

fn random_index(max: usize) -> usize {
    if max == 0 {
        return 0;
    }

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as usize % max)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_start_offset() {
        let manifest: MusicManifest = toml::from_str(
            r#"
            [[track]]
            title = "Intro Trim"
            author = "Tester"
            file = "intro.ogg"
            license = "CC0"
            source = "https://example.invalid"
            start_offset_seconds = 1.25
            "#,
        )
        .expect("manifest should parse");

        assert_eq!(manifest.track.len(), 1);
        assert_eq!(manifest.track[0].start_offset_seconds, 1.25);
    }
}
