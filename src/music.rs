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
        self.player.set_volume(0.24);
    }

    fn restart_procedural(&mut self) {
        let track = TRACKS[self.track_index % TRACKS.len()];
        self.current_name = format!("{} (generated)", track.name);
        self.player.append(CyberSource::new(track));
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
