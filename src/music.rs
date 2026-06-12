use std::{
    f32::consts::TAU,
    fs,
    fs::File,
    num::NonZero,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use serde::Deserialize;

use crate::error::{EutherError, Result};

const SAMPLE_RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const TRACKS: [CyberTrack; 10] = [
    CyberTrack::new("Midnight Uplink", 92.0, 41.2, [0, 3, 7, 10]),
    CyberTrack::new("Chrome Alley", 104.0, 43.65, [0, 2, 7, 9]),
    CyberTrack::new("Neon Rain", 88.0, 36.71, [0, 5, 7, 10]),
    CyberTrack::new("Null District", 118.0, 49.0, [0, 3, 8, 10]),
    CyberTrack::new("Battery Shrine", 96.0, 38.89, [0, 4, 7, 11]),
    CyberTrack::new("Ghost Terminal", 110.0, 46.25, [0, 3, 5, 10]),
    CyberTrack::new("Vapor Circuit", 82.0, 34.65, [0, 2, 5, 9]),
    CyberTrack::new("Ion Market", 126.0, 51.91, [0, 3, 7, 12]),
    CyberTrack::new("Blackout Metro", 100.0, 30.87, [0, 5, 8, 10]),
    CyberTrack::new("Synthetic Dawn", 112.0, 55.0, [0, 4, 7, 10]),
];

pub struct AudioEngine {
    device_sink: MixerDeviceSink,
    player: Player,
    track_index: usize,
    file_tracks: Vec<MusicTrack>,
    current_name: String,
}

#[derive(Debug, Clone, Copy)]
pub struct CyberTrack {
    name: &'static str,
    bpm: f32,
    root: f32,
    scale: [i32; 4],
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
    file: PathBuf,
    license: String,
    source: String,
}

impl AudioEngine {
    pub fn start_random() -> Result<Self> {
        let mut device_sink = DeviceSinkBuilder::open_default_sink()
            .map_err(|err| EutherError::Audio(err.to_string()))?;
        device_sink.log_on_drop(false);
        let player = Player::connect_new(device_sink.mixer());
        let file_tracks = load_music_tracks();
        let track_count = file_tracks.len().max(TRACKS.len());
        let mut engine = Self {
            device_sink,
            player,
            track_index: random_index(track_count),
            file_tracks,
            current_name: String::new(),
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

    fn restart_current_track(&mut self) {
        self.player.stop();
        self.player = Player::connect_new(self.device_sink.mixer());

        if !self.file_tracks.is_empty() {
            let track = self.file_tracks[self.track_index % self.file_tracks.len()].clone();
            match File::open(&track.file)
                .map_err(|err| err.to_string())
                .and_then(|file| Decoder::try_from(file).map_err(|err| err.to_string()))
            {
                Ok(source) => {
                    self.current_name =
                        format!("{} ({}, {})", track.title, track.license, track.source);
                    self.player.append(source.repeat_infinite());
                }
                Err(_) => {
                    self.restart_procedural();
                }
            }
        } else {
            self.restart_procedural();
        }

        self.player.play();
        self.player.set_volume(0.35);
    }

    fn restart_procedural(&mut self) {
        let track = TRACKS[self.track_index % TRACKS.len()];
        self.current_name = format!("{} (generated)", track.name);
        self.player.append(CyberSource::new(track));
    }
}

impl CyberTrack {
    const fn new(name: &'static str, bpm: f32, root: f32, scale: [i32; 4]) -> Self {
        Self {
            name,
            bpm,
            root,
            scale,
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
        let step = ((beat * 4.0).floor() as usize) % 16;
        let step_phase = (beat * 4.0).fract();
        let bar_phase = (beat / 4.0).fract();
        let root = self.track.root;

        let bass_step = [0, 0, 7, 0, 10, 0, 7, 3, 0, 0, 5, 0, 7, 0, 10, 7][step];
        let arp_step = self.track.scale[(step * 3 + 1) % self.track.scale.len()] + 12;
        let bass = square(seconds, semitone(root, bass_step), 0.42) * gate(step_phase, 0.62);
        let arp = saw(seconds, semitone(root, arp_step), 0.22) * gate(step_phase, 0.34);
        let pad = saw(
            seconds,
            semitone(root, self.track.scale[step % 4] + 24),
            0.08,
        ) * (0.55 + 0.45 * (bar_phase * TAU).sin());
        let kick = thump(step_phase, step.is_multiple_of(4)) * 0.55;
        let hat = noise(self.frame) * gate(step_phase, 0.12) * 0.08;
        let snare =
            noise(self.frame.wrapping_mul(31)) * thump(step_phase, step == 4 || step == 12) * 0.16;

        soft_clip(bass + arp + pad + kick + hat + snare) * 0.7
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

fn square(seconds: f32, frequency: f32, width: f32) -> f32 {
    if (seconds * frequency).fract() < width {
        1.0
    } else {
        -1.0
    }
}

fn saw(seconds: f32, frequency: f32, amount: f32) -> f32 {
    (((seconds * frequency).fract() * 2.0) - 1.0) * amount
}

fn gate(phase: f32, width: f32) -> f32 {
    if phase > width {
        0.0
    } else {
        1.0 - phase / width
    }
}

fn thump(phase: f32, enabled: bool) -> f32 {
    if !enabled || phase > 0.35 {
        return 0.0;
    }

    (1.0 - phase / 0.35).powf(3.0) * (phase * TAU * 18.0).sin()
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
    let manifest_path = Path::new("assets/music/music.toml");
    let Ok(data) = fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    let Ok(manifest) = toml::from_str::<MusicManifest>(&data) else {
        return Vec::new();
    };

    manifest
        .track
        .into_iter()
        .filter(|track| track.file.exists())
        .collect()
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
