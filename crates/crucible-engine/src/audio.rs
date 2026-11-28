//! Audio files and the offline mix: import, place, waveform peaks, render, export.
//!
//! Rules from `docs/04-audio-quality.md` this file keeps:
//! - **Unity is exact.** 0 dB and centre pan multiply by exactly 1.0, so a lone clip at unity
//!   exports the same samples it imported (the null test).
//! - **No silent resampling.** A file at another sample rate is refused with E5017 unless the
//!   person agrees to convert it (`--convert`; the window asks). A converted clip says so.
//! - **No hidden processing.** Dither only when the user exports 16-bit with it on (the default
//!   there, shown in the reply); no limiter, no normalising. Overs are clipped in fixed-point files
//!   and reported (W5020), never squashed.
//! - **One render path.** `render` mixes one block and gets each clip's samples through a
//!   callback: from files during export, from a disk thread's buffers during playback (M2).
//!
//! Where the sound comes from while mixing (2026-11-25, D-039):
//! - A WAV at the project's rate is streamed from disk in blocks, never loaded whole.
//! - Anything else (MP3, FLAC, or a file at another rate) is decoded, and converted to the
//!   project's rate if needed, once, into memory (`Sources`), and played from there. The file in
//!   the project folder stays exactly as it was imported. Memory: about 23 MB per stereo minute
//!   at 48 kHz, four times that at 192 kHz.
//!
//! Conversion: rubato's FFT resampler, in 64-bit floating point, one channel at a time. The
//! rates Crucible offers all have exact ratios, which is what that resampler is made for.
//!
//! Pan law (D-030, proposed): balance. Centre passes both channels unchanged; turning toward one
//! side fades the other with a cosine, reaching silence at the hard side. Nothing is boosted.
//!
//! Imported files are copied, byte for byte and in their own format, into the project's `audio`
//! folder, so a project folder is self-contained and opens the same on every machine. Before
//! 2026-11-25 MP3 and FLAC files were decoded into 32-bit float WAVs there: five to ten times
//! their size, and slow to write on a slow drive. Placing a file on a track (`clip.place`)
//! is an ordinary journaled edit that names the copy, so crash recovery works.
//!
//! Portability: std file I/O, the pure-Rust `hound` (WAV) and `symphonia` (MP3, FLAC). No
//! OS-specific code.

use super::{Engine, clean_name};
use crate::command::{Args, Kind, Reply, Spec, opt, req};
use crate::Effect;
use crucible_diagnostics::{self as diag, Code, DawError};
use crucible_project::{Clip, Content, Project, TrackKind};
use serde_json::json;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(crate) const AUDIO_DIR: &str = "audio";
/// Peak resolution for waveforms: one value per this many frames (5.3 ms at 48 kHz).
const FRAMES_PER_PEAK: usize = 256;
const BLOCK: usize = 4096;

pub(crate) static SPECS: &[Spec<Engine>] = &[
    Spec {
        name: "audio.import",
        args: &[req("file", Kind::Text, "path to a WAV, MP3 or FLAC file"), opt("convert", Kind::Bool, "agree to convert it to the project's rate if it has another one")],
        help: "Bring a file into the project's audio folder, copied unchanged in its own format (WAV, MP3, FLAC). Then clip.place.",
        effect: Effect::File,
        run: audio_import,
    },
    Spec {
        name: "clip.place",
        args: &[
            req("track", Kind::Track, "an audio track: name, position or id"),
            req("at", Kind::Number, "position in beats from the song start"),
            req("source", Kind::Text, "file name in the project's audio folder, as audio.import reported it"),
            opt("name", Kind::Text, "clip name (default: the file name)"),
            opt("convert", Kind::Bool, "agree to convert it to the project's rate if it has another one"),
        ],
        help: "Put an imported audio file on an audio track as a new clip, full length.",
        effect: Effect::Edit,
        run: clip_place,
    },
    Spec {
        name: "audio.peaks",
        args: &[req("source", Kind::Text, "file name in the project's audio folder")],
        help: "Waveform outline of an imported file: one 0-255 level per 256 frames, base64.",
        effect: Effect::Read,
        run: audio_peaks,
    },
    Spec {
        name: "project.export",
        args: &[
            req("file", Kind::Text, "where to write the WAV file"),
            opt("bits", Kind::Choice(&["16", "24", "32f"]), "16, 24 (default) or 32f (32-bit float)"),
            opt("dither", Kind::Choice(&["on", "off"]), "TPDF dither for 16-bit (default on there; ignored otherwise)"),
            opt("start", Kind::Number, "from this beat (default 0)"),
            opt("end", Kind::Number, "to this beat (default: the end of the last audible clip)"),
        ],
        help: "Render the mix offline to a stereo WAV at the project rate: clip and track gain, pan, mute, solo, master.",
        effect: Effect::File,
        run: project_export,
    },
];

// ---------------------------------------------------------------- files

pub(crate) struct Info {
    pub frames: u64,
    pub rate: u32,
    pub channels: u16,
    pub kind: String,
}

fn unreadable(path: &Path, why: impl std::fmt::Display) -> DawError {
    DawError::new(Code::AudioFileUnreadable, format!("{}: {why}", path.display()))
}

/// The sample format as text, or why it cannot be read. Clips are mono or stereo; impulse
/// responses for IR Reverb may also have four channels (true stereo), so `info` allows four and
/// `Reader` (clip playback) checks for two.
fn check_spec(path: &Path, s: hound::WavSpec) -> Result<String, DawError> {
    let kind = match (s.sample_format, s.bits_per_sample) {
        (hound::SampleFormat::Int, 8 | 16 | 24 | 32) => format!("{}-bit", s.bits_per_sample),
        (hound::SampleFormat::Float, 32) => "32-bit float".to_string(),
        (f, b) => return Err(unreadable(path, format!("{b}-bit {f:?} samples are not supported"))),
    };
    if ![1, 2, 4].contains(&s.channels) {
        return Err(unreadable(path, format!("{} channels; Crucible reads mono and stereo files, and 4-channel impulse responses", s.channels)));
    }
    Ok(kind)
}

/// Header facts of a WAV file: length, rate, channels, format.
pub(crate) fn info(path: &Path) -> Result<Info, DawError> {
    if !path.is_file() {
        return Err(DawError::new(Code::AudioMissing, path.display().to_string()));
    }
    let r = hound::WavReader::open(path).map_err(|e| unreadable(path, e))?;
    let s = r.spec();
    Ok(Info { frames: u64::from(r.duration()), rate: s.sample_rate, channels: s.channels, kind: check_spec(path, s)? })
}

/// A whole impulse response, one buffer per channel. IRs are short, so reading it all is fine;
/// longer than 30 s is refused rather than quietly eating memory.
pub(crate) fn read_ir(path: &Path) -> Result<(Vec<Vec<f32>>, u32), DawError> {
    let i = info(path)?;
    if i.frames as f64 / f64::from(i.rate) > 30.0 {
        return Err(DawError::new(Code::EffectRefused, format!("{}: {:.0} s is too long for an impulse response (30 s at most)", path.display(), i.frames as f64 / f64::from(i.rate))));
    }
    read_wav(path)
}

/// Every channel of a WAV file, as floats, and its rate.
fn read_wav(path: &Path) -> Result<(Vec<Vec<f32>>, u32), DawError> {
    let i = info(path)?;
    let mut r = hound::WavReader::open(path).map_err(|e| unreadable(path, e))?;
    let s = r.spec();
    let ch = usize::from(s.channels);
    let mut out = vec![Vec::with_capacity(i.frames as usize); ch];
    let samples: Vec<f32> = match s.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let k = 1.0 / (1u64 << (s.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|v| v.map(|v| v as f32 * k)).collect::<Result<_, _>>()
        }
    }
    .map_err(|e| unreadable(path, e))?;
    for (n, v) in samples.into_iter().enumerate() {
        out[n % ch].push(v);
    }
    Ok((out, s.sample_rate))
}

/// One open WAV file, read front to back as a clip plays. Seeks only when asked for another
/// position. Integer samples are divided by a power of two, so 16 and 24-bit values come back
/// exactly when written out again at the same depth.
pub(crate) struct WavStream {
    r: hound::WavReader<BufReader<File>>,
    path: PathBuf,
    stereo: bool,
    /// `None` for float files; otherwise the factor that maps the integers to -1..1.
    scale: Option<f32>,
    pos: u64,
}

impl WavStream {
    pub(crate) fn open(path: &Path) -> Result<WavStream, DawError> {
        if !path.is_file() {
            return Err(DawError::new(Code::AudioMissing, path.display().to_string()));
        }
        let r = hound::WavReader::open(path).map_err(|e| unreadable(path, e))?;
        let s = r.spec();
        check_spec(path, s)?;
        if s.channels > 2 {
            return Err(unreadable(path, format!("{} channels: an impulse response for IR Reverb, not a clip", s.channels)));
        }
        let scale = (s.sample_format == hound::SampleFormat::Int).then(|| 1.0 / (1u64 << (s.bits_per_sample - 1)) as f32);
        Ok(WavStream { r, path: path.to_path_buf(), stereo: s.channels == 2, scale, pos: 0 })
    }

    /// Frames starting at `from` into `l` and `r`; a mono file fills both. Past the end of the
    /// file the rest is silence.
    pub(crate) fn read(&mut self, from: u64, l: &mut [f32], r: &mut [f32]) -> Result<(), DawError> {
        if from != self.pos {
            let to = u32::try_from(from).unwrap_or(u32::MAX).min(self.r.duration());
            self.r.seek(to).map_err(|e| unreadable(&self.path, e))?;
            self.pos = u64::from(to);
        }
        let (stereo, path) = (self.stereo, &self.path);
        let got = match self.scale {
            None => pull(self.r.samples::<f32>(), stereo, l, r, |v| v),
            Some(k) => pull(self.r.samples::<i32>(), stereo, l, r, |v| v as f32 * k),
        }
        .map_err(|e| unreadable(path, e))?;
        l[got..].fill(0.0);
        r[got..].fill(0.0);
        self.pos += got as u64;
        Ok(())
    }
}

/// Audio held in memory at the project's rate, shared by every clip that plays it.
pub(crate) struct Decoded {
    /// The file's name in the audio folder, for the log.
    pub name: String,
    /// One buffer per channel (one or two), at the project's rate.
    pub ch: Vec<Vec<f32>>,
    /// What the file itself is: its rate, its length in its own frames, its format.
    pub file_rate: u32,
    pub file_frames: u64,
    pub kind: String,
}

impl Decoded {
    pub(crate) fn frames(&self) -> u64 {
        self.ch[0].len() as u64
    }
    pub(crate) fn megabytes(&self) -> f64 {
        (self.ch.len() * self.ch[0].len() * 4) as f64 / 1_048_576.0
    }
}

/// Where a clip's sound comes from while mixing.
#[derive(Clone)]
pub(crate) enum Src {
    /// A WAV at the project's rate, streamed from disk.
    File(PathBuf),
    /// Decoded or converted into memory.
    Memory(Arc<Decoded>),
}

impl Src {
    pub(crate) fn name(&self) -> String {
        match self {
            Src::File(p) => p.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned()),
            Src::Memory(d) => format!("{} (in memory)", d.name),
        }
    }
}

/// Reads a clip's sound: from a WAV on disk, or from memory.
pub(crate) enum Reader {
    Wav(WavStream),
    Memory(Arc<Decoded>),
}

impl Reader {
    pub(crate) fn open(src: &Src) -> Result<Reader, DawError> {
        Ok(match src {
            Src::File(p) => Reader::Wav(WavStream::open(p)?),
            Src::Memory(d) => Reader::Memory(d.clone()),
        })
    }

    /// Frames starting at `from` into `l` and `r`; mono fills both; past the end, silence.
    pub(crate) fn read(&mut self, from: u64, l: &mut [f32], r: &mut [f32]) -> Result<(), DawError> {
        match self {
            Reader::Wav(w) => w.read(from, l, r),
            Reader::Memory(d) => {
                let (a, b) = (&d.ch[0], d.ch.get(1).unwrap_or(&d.ch[0]));
                let start = (from as usize).min(a.len());
                let n = l.len().min(a.len() - start);
                l[..n].copy_from_slice(&a[start..start + n]);
                r[..n].copy_from_slice(&b[start..start + n]);
                l[n..].fill(0.0);
                r[n..].fill(0.0);
                Ok(())
            }
        }
    }
}

/// The frames of a WAV at `rate` with one or two channels: one that plays straight from disk.
/// `None` for anything else.
pub(crate) fn plain_wav(path: &Path, rate: u32) -> Option<u64> {
    let r = hound::WavReader::open(path).ok()?;
    let s = r.spec();
    (s.sample_rate == rate && s.channels <= 2 && check_spec(path, s).is_ok()).then(|| u64::from(r.duration()))
}

/// Every channel of a WAV, MP3 or FLAC file, its rate, and its format in words.
fn load_whole(path: &Path) -> Result<(Vec<Vec<f32>>, u32, String), DawError> {
    if !path.is_file() {
        return Err(DawError::new(Code::AudioMissing, path.display().to_string()));
    }
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "wav" || ext == "wave" {
        let i = info(path)?;
        if i.channels > 2 {
            return Err(unreadable(path, format!("{} channels: an impulse response for IR Reverb, not a clip", i.channels)));
        }
        let (ch, rate) = read_wav(path)?;
        Ok((ch, rate, format!("{} WAV", i.kind)))
    } else {
        let (ch, rate) = decode_file(path)?;
        Ok((ch, rate, ext.to_uppercase()))
    }
}

/// Convert one channel from `from` Hz to `to` Hz: rubato's FFT resampler in 64-bit floating
/// point, a chunk at a time, its start-up delay trimmed, its length the exact ratio rounded up.
fn convert_channel(x: &[f32], from: u32, to: u32) -> Result<Vec<f32>, DawError> {
    use rubato::audioadapter_buffers::direct::SequentialSlice;
    use rubato::{Fft, FixedSync, Indexing, Resampler};
    let fail = |e: String| DawError::new(Code::ConvertFailed, format!("{from} Hz to {to} Hz: {e}"));
    let mut rs = Fft::<f64>::new(from as usize, to as usize, 4096, 1, FixedSync::Input).map_err(|e| fail(e.to_string()))?;
    let expected = (x.len() as f64 * f64::from(to) / f64::from(from)).ceil() as usize;
    let mut skip = rs.output_delay();
    let (mut inbuf, mut outbuf) = (vec![0.0f64; rs.input_frames_max()], vec![0.0f64; rs.output_frames_max()]);
    let mut out = Vec::with_capacity(expected);
    let mut pos = 0;
    while out.len() < expected {
        let need = rs.input_frames_next();
        let n = need.min(x.len() - pos);
        for (d, s) in inbuf[..n].iter_mut().zip(&x[pos..pos + n]) {
            *d = f64::from(*s);
        }
        pos += n;
        let input = SequentialSlice::new(&inbuf[..need], 1, need).map_err(|e| fail(e.to_string()))?;
        let cap = outbuf.len();
        let mut output = SequentialSlice::new_mut(&mut outbuf[..], 1, cap).map_err(|e| fail(e.to_string()))?;
        let idx = Indexing { partial_len: (n < need).then_some(n), ..Default::default() };
        let (_, made) = rs.process_into_buffer(&input, &mut output, Some(&idx)).map_err(|e| fail(e.to_string()))?;
        let dropped = skip.min(made);
        skip -= dropped;
        out.extend(outbuf[dropped..made].iter().take(expected - out.len()).map(|v| *v as f32));
    }
    Ok(out)
}

/// File and rate to the sound held for them.
type Held_ = std::collections::HashMap<(PathBuf, u32), Arc<Decoded>>;

/// Sounds held in memory, by file and rate. Shared between the engine and the mixers it builds;
/// kept until another project is opened, so undoing a removed clip does not decode it again.
#[derive(Clone, Default)]
pub(crate) struct Sources(Arc<std::sync::Mutex<Held_>>);

impl Sources {
    fn lock(&self) -> std::sync::MutexGuard<'_, Held_> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The file at `path`, at `rate`, loading (decoding, converting) it first if needed.
    pub(crate) fn get(&self, path: &Path, rate: u32) -> Result<Arc<Decoded>, DawError> {
        if let Some(d) = self.lock().get(&(path.to_path_buf(), rate)) {
            return Ok(d.clone());
        }
        let (ch, file_rate, kind) = load_whole(path)?;
        self.hold(path, rate, ch, file_rate, kind)
    }

    /// Hold sound already read from `path` (at `file_rate`), converting it to `rate` if needed.
    pub(crate) fn hold(&self, path: &Path, rate: u32, ch: Vec<Vec<f32>>, file_rate: u32, kind: String) -> Result<Arc<Decoded>, DawError> {
        let t0 = std::time::Instant::now();
        let file_frames = ch.first().map_or(0, |c| c.len() as u64);
        if file_frames == 0 {
            return Err(unreadable(path, "no audio in the file"));
        }
        let ch = if file_rate == rate { ch } else { ch.iter().map(|c| convert_channel(c, file_rate, rate)).collect::<Result<Vec<_>, _>>()? };
        let name = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
        let d = Arc::new(Decoded { name, ch, file_rate, file_frames, kind });
        diag::event(
            "AUDIO",
            format!(
                "into memory: {} ({}, {} Hz{}, {:.1} s) | {:.0} MB | {:.0} ms",
                d.name,
                d.kind,
                file_rate,
                if file_rate == rate { String::new() } else { format!(", converted to {rate} Hz") },
                file_frames as f64 / f64::from(file_rate),
                d.megabytes(),
                t0.elapsed().as_secs_f64() * 1000.0
            ),
        );
        self.lock().insert((path.to_path_buf(), rate), d.clone());
        Ok(d)
    }

    pub(crate) fn clear(&self) {
        self.lock().clear();
    }

    /// Let go of the sound held for these files, at any rate. Returns the megabytes let go.
    pub(crate) fn release(&self, files: &[PathBuf]) -> f64 {
        let mut held = self.lock();
        let before: f64 = held.values().map(|d| d.megabytes()).sum();
        held.retain(|(p, _), _| !files.contains(p));
        before - held.values().map(|d| d.megabytes()).sum::<f64>()
    }

    /// The files under `from` were copied to `to` (a save somewhere new): hold their sound under
    /// the new paths, so it is not decoded again.
    pub(crate) fn moved(&self, from: &Path, to: &Path) {
        let mut held = self.lock();
        let old: Vec<(PathBuf, u32)> = held.keys().filter(|(p, _)| p.starts_with(from)).cloned().collect();
        for key in old {
            if let (Ok(rel), Some(d)) = (key.0.strip_prefix(from).map(Path::to_path_buf), held.remove(&key)) {
                held.insert((to.join(rel), key.1), d);
            }
        }
    }

    /// Memory held, in MB.
    pub(crate) fn megabytes(&self) -> f64 {
        self.lock().values().map(|d| d.megabytes()).sum()
    }
}

/// Fill `l` and `r` from interleaved samples. Returns the frames read before the file ended.
fn pull<S>(mut it: impl Iterator<Item = hound::Result<S>>, stereo: bool, l: &mut [f32], r: &mut [f32], f: impl Fn(S) -> f32) -> hound::Result<usize> {
    for k in 0..l.len() {
        let Some(a) = it.next() else { return Ok(k) };
        let a = f(a?);
        let b = if stereo { it.next().transpose()?.map_or(a, &f) } else { a };
        l[k] = a;
        r[k] = b;
    }
    Ok(l.len())
}

/// Decode a whole MP3 or FLAC file: one buffer per channel, and its rate. A damaged frame is
/// skipped, as players do.
fn decode_file(path: &Path) -> Result<(Vec<Vec<f32>>, u32), DawError> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    let file = File::open(path).map_err(|e| unreadable(path, e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|x| x.to_str()) {
        hint.with_extension(ext);
    }
    let what = "not an audio format Crucible reads (WAV, MP3, FLAC)";
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| unreadable(path, format!("{what}: {e}")))?;
    let (id, params) = {
        let t = format.default_track(TrackType::Audio).ok_or_else(|| unreadable(path, "no audio track"))?;
        let p = t.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(|| unreadable(path, what))?;
        (t.id, p.clone())
    };
    let mut dec = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(|e| unreadable(path, e))?;
    let (mut ch, mut rate, mut buf) = (Vec::<Vec<f32>>::new(), 0u32, Vec::<f32>::new());
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(unreadable(path, e)),
        };
        if packet.track_id != id {
            continue;
        }
        let b = match dec.decode(&packet) {
            Ok(b) => b,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(unreadable(path, e)),
        };
        if ch.is_empty() {
            let n = b.spec().channels().count();
            if !(1..=2).contains(&n) {
                return Err(unreadable(path, format!("{n} channels; Crucible reads mono and stereo files")));
            }
            rate = b.spec().rate();
            ch = vec![Vec::new(); n];
        }
        buf.resize(b.samples_interleaved(), 0.0);
        b.copy_to_slice_interleaved(&mut buf);
        let n = ch.len();
        for (k, v) in buf.iter().enumerate() {
            ch[k % n].push(*v);
        }
    }
    if ch.is_empty() || ch[0].is_empty() {
        return Err(unreadable(path, "no audio in the file"));
    }
    Ok((ch, rate))
}

/// The project's audio folder; for a project never saved, its own folder until then (D-042).
fn audio_dir(e: &mut Engine) -> Result<PathBuf, DawError> {
    Ok(e.work_folder()?.join(AUDIO_DIR))
}

/// Copy the audio folder of the project folder `from` into `to`, for a save somewhere new. A file
/// already there with the same bytes is left alone. Returns how many files were copied, and bytes.
pub(crate) fn copy_audio(from: &Path, to: &Path) -> Result<(usize, u64), DawError> {
    let (src, dst) = (from.join(AUDIO_DIR), to.join(AUDIO_DIR));
    let Ok(entries) = std::fs::read_dir(&src) else { return Ok((0, 0)) };
    let fail = |what: &Path, e: std::io::Error| DawError::new(Code::ProjectSaveFailed, format!("copying {} into {}: {e}", what.display(), dst.display()));
    std::fs::create_dir_all(&dst).map_err(|e| fail(&src, e))?;
    let (mut files, mut bytes) = (0, 0);
    for f in entries.flatten().map(|e| e.path()).filter(|p| p.is_file()) {
        let Some(name) = f.file_name() else { continue };
        let there = dst.join(name);
        if same_bytes(&f, &there) {
            continue;
        }
        bytes += std::fs::copy(&f, &there).map_err(|e| fail(&f, e))?;
        files += 1;
    }
    Ok((files, bytes))
}

/// After an edit: let go of sound held in memory for files that were on the timeline before it
/// and are on it no longer (a clip or a track removed, an undo). Found by Dzyan 2026-11-26: two
/// long songs converted to 192 kHz held 1.1 GB, and still did after both were removed. The sound
/// used to be kept until another project was opened, so that undoing a removal was instant; now
/// an undone removal reads its file again the next time it plays.
pub(crate) fn release_unused(e: &Engine, before: &Project) {
    let files = |p: &Project| -> std::collections::HashSet<String> {
        p.clips.iter().filter(|c| c.has_audio_file()).filter_map(|c| match &c.content {
            Content::Audio { source, .. } => Some(source.to_string()),
            _ => None,
        }).collect()
    };
    let (was, now) = (files(before), files(&e.project));
    let gone: Vec<&String> = was.difference(&now).collect();
    let Some(dir) = e.work().map(|w| w.join(AUDIO_DIR)) else { return };
    if gone.is_empty() {
        return;
    }
    let freed = e.sources.release(&gone.iter().map(|f| dir.join(f)).collect::<Vec<_>>());
    if freed > 0.0 {
        diag::event("AUDIO", format!("out of memory: {} | {freed:.0} MB let go, no clip uses it now", gone.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")));
    }
}

/// Where playback stops, in frames (D-043): after the last sound, effect tails included; after
/// the last clip of any kind, muted or not (a MIDI clip is silent until instruments are hosted,
/// but the song is still that long, and the window draws its end there); or where the person
/// moved the end to, if that is further out. 0 for a project with nothing in it.
pub(crate) fn song_end(p: &Project, m: &Mixer) -> u64 {
    let last_clip = p.clips.iter().map(|c| c.start + c.length).max().unwrap_or(0).max(0);
    m.end_frame().max((last_clip as f64 * p.frames_per_tick()).round() as u64).max(set_end(p))
}

/// The frame the person moved the song's end to (D-043), or 0 if it is at the last sound.
pub(crate) fn set_end(p: &Project) -> u64 {
    (p.end.max(0) as f64 * p.frames_per_tick()).round() as u64
}

/// Whether two files hold the same bytes. Sizes first, then 64 KB at a time.
fn same_bytes(a: &Path, b: &Path) -> bool {
    let size = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    if size(a).is_none() || size(a) != size(b) {
        return false;
    }
    let (Ok(fa), Ok(fb)) = (File::open(a), File::open(b)) else { return false };
    let (mut fa, mut fb) = (BufReader::new(fa), BufReader::new(fb));
    let (mut x, mut y) = (vec![0u8; 65536], vec![0u8; 65536]);
    loop {
        let n = fa.read(&mut x).unwrap_or(0);
        if n == 0 {
            return true;
        }
        if fb.read_exact(&mut y[..n]).is_err() || x[..n] != y[..n] {
            return false;
        }
    }
}

/// A file name for `wanted` in `dir` that is free, or already holds the same bytes as `file`.
fn free_name(dir: &Path, wanted: &str, file: &Path) -> (String, bool) {
    let p = Path::new(wanted);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("audio");
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("wav");
    for n in 1.. {
        let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem} {n}.{ext}") };
        let there = dir.join(&name);
        if !there.exists() {
            return (name, false);
        }
        if same_bytes(&there, file) {
            return (name, true);
        }
    }
    unreachable!()
}

// ---------------------------------------------------------------- commands

/// E5017 for a file at another rate, saying what can be done about it.
fn rate_refused(what: &str, file_rate: u32, rate: u32, seconds: f64) -> DawError {
    DawError::new(
        Code::SampleRateMismatch,
        format!("{what} is {file_rate} Hz, {seconds:.1} s long; the project is {rate} Hz. Add --convert to convert it to {rate} Hz while it plays (the file itself is kept as it is), or set the project to {file_rate} Hz with project.rate"),
    )
}

fn audio_import(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let src = PathBuf::from(a.need_text("file")?);
    let dir = audio_dir(e)?;
    if !src.is_file() {
        return Err(unreadable(&src, "no such file"));
    }
    let ext = src.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    let stem = src.file_stem().and_then(|n| n.to_str()).unwrap_or("audio").to_string();
    let (rate, convert) = (e.project.sample_rate, a.flag("convert") == Some(true));
    std::fs::create_dir_all(&dir).map_err(|err| unreadable(&dir, err))?;
    // What is in the file. A WAV says so in its header; MP3 and FLAC are decoded now, which is
    // needed anyway for the waveform and for playing.
    let wav = ext == "wav" || ext == "wave";
    let (file_rate, channels, mut raw) = if wav {
        let i = info(&src)?;
        (i.rate, i.channels, None)
    } else {
        let (ch, r) = decode_file(&src)?;
        (r, ch.len() as u16, Some(ch))
    };
    // A four-channel WAV is an impulse response for IR Reverb (fx.file): copied, never a clip.
    let ir = channels > 2;
    if file_rate != rate && !convert && !ir {
        let frames = match &raw { Some(ch) => ch[0].len() as u64, None => info(&src)?.frames };
        return Err(rate_refused(&src.display().to_string(), file_rate, rate, frames as f64 / f64::from(file_rate)));
    }
    // The file itself goes into the audio folder exactly as it is, under a temporary name first,
    // so a crash never leaves a half file under the real one.
    let (name, existed) = free_name(&dir, &format!("{stem}.{ext}"), &src);
    let placed = dir.join(&name);
    if !existed {
        let tmp = dir.join(format!("{stem}.import.part"));
        std::fs::copy(&src, &tmp).map_err(|err| unreadable(&tmp, err))?;
        std::fs::rename(&tmp, &placed).map_err(|err| unreadable(&placed, err))?;
    }
    // A WAV at the project's rate plays from disk; anything else is held in memory.
    let (file_frames, kind, held) = match plain_wav(&placed, rate) {
        _ if ir => {
            let i = info(&placed)?;
            (i.frames, format!("{}, {} channels (an impulse response for IR Reverb)", i.kind, i.channels), None)
        }
        Some(frames) => {
            if let Ok(p) = cached_peaks(&placed) {
                e.peaks.insert(placed.clone(), Arc::new(p));
            }
            (frames, info(&placed)?.kind, None)
        }
        None => {
            let d = match raw.take() {
                Some(ch) => e.sources.hold(&placed, rate, ch, file_rate, ext.to_uppercase())?,
                None => e.sources.get(&placed, rate)?,
            };
            e.peaks.insert(placed.clone(), Arc::new(peaks_from(&d)));
            (d.file_frames, d.kind.clone(), Some(d))
        }
    };
    let secs = file_frames as f64 / f64::from(file_rate);
    let mut what = format!("{kind}, {file_rate} Hz, {}{secs:.2} s", match channels { 1 => "mono, ", 2 => "stereo, ", _ => "" });
    if file_rate != rate && !ir {
        what.push_str(&format!(", converted to {rate} Hz while it plays"));
    }
    if let Some(d) = &held {
        what.push_str(&format!("; held in memory: {:.0} MB", d.megabytes()));
    }
    let text = if existed { format!("Already in the project as '{name}' ({what})") } else { format!("Imported '{name}' ({what})") };
    Ok(Reply::new(
        text,
        json!({ "source": name, "frames": file_frames, "rate": file_rate, "channels": channels, "format": kind, "seconds": secs,
                "converted": file_rate != rate && !ir, "memory_mb": held.map(|d| d.megabytes()) }),
    ))
}

fn clip_place(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let ti = e.track_at(a, "track")?;
    let track = &e.project.tracks[ti];
    if track.kind != TrackKind::Audio {
        return Err(DawError::new(Code::ClipEditRefused, format!("'{}' is an instrument track; audio goes on audio tracks", track.name)));
    }
    let track = track.id;
    let source = a.need_text("source")?.to_string();
    if source.contains(['/', '\\']) || source.starts_with('.') {
        return Err(DawError::new(Code::BadArgument, format!("'{source}' should be a file name in the audio folder, as audio.import reported it")));
    }
    let path = audio_dir(e)?.join(&source);
    let rate = e.project.sample_rate;
    let ext = path.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "wav" || ext == "wave" {
        let i = info(&path)?;
        if i.channels > 2 {
            return Err(DawError::new(Code::ClipEditRefused, format!("{source} has {} channels: it is an impulse response for IR Reverb (fx.file), not a clip", i.channels)));
        }
        if i.rate != rate && a.flag("convert") != Some(true) {
            return Err(rate_refused(&source, i.rate, rate, i.frames as f64 / f64::from(i.rate)));
        }
    }
    // Its length at the project's rate, and what the file itself is.
    let (at_rate, file_frames, file_rate) = match plain_wav(&path, rate) {
        Some(n) => (n, n, rate),
        None => {
            let d = e.sources.get(&path, rate)?;
            if d.file_rate != rate && a.flag("convert") != Some(true) {
                return Err(rate_refused(&source, d.file_rate, rate, d.file_frames as f64 / f64::from(d.file_rate)));
            }
            (d.frames(), d.file_frames, d.file_rate)
        }
    };
    let i = Info { frames: file_frames, rate: file_rate, channels: 2, kind: String::new() };
    let start = super::clips::ticks(a.need_number("at")?)?;
    let source_length = (at_rate as f64 / e.project.frames_per_tick()).ceil().max(1.0) as i64;
    let name = match a.text("name") {
        Some(n) => clean_name(n)?,
        None => clean_name(Path::new(&source).file_stem().and_then(|s| s.to_str()).unwrap_or("Audio"))?,
    };
    let clip = Clip {
        id: 0,
        track,
        name,
        start,
        length: source_length,
        offset: 0,
        gain_db: 0.0,
        content: Content::Audio { source, source_length, frames: i.frames, rate: i.rate },
    };
    e.project.check_clip(&clip).map_err(|m| DawError::new(Code::ClipEditRefused, m))?;
    let id = e.project.add_clip(clip);
    let c = &e.project.clips[e.project.clips.len() - 1];
    Ok(Reply::new(
        format!(
            "Placed clip #{id} '{}' at beat {} ({:.2} s{})",
            c.name,
            super::clips::beats(start),
            i.frames as f64 / f64::from(i.rate),
            if i.rate == rate { String::new() } else { format!(", converted from {} Hz", i.rate) }
        ),
        json!({ "id": id, "created": [id], "track": track, "start": super::clips::beats(start), "length": super::clips::beats(source_length) }),
    ))
}

fn audio_peaks(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let source = a.need_text("source")?.to_string();
    let path = audio_dir(e)?.join(&source);
    let peaks = match e.peaks.get(&path) {
        Some(p) => p.clone(),
        None => {
            // Peaks are at the project's rate: from the disk cache for a WAV that plays from
            // disk, from memory for anything held there.
            let p = Arc::new(match plain_wav(&path, e.project.sample_rate) {
                Some(_) => cached_peaks(&path)?,
                None => peaks_from(&*e.sources.get(&path, e.project.sample_rate)?),
            });
            e.peaks.insert(path.clone(), p.clone());
            p
        }
    };
    Ok(Reply::new(
        format!("{source}: {} peaks, {FRAMES_PER_PEAK} frames each", peaks.len()),
        json!({ "source": source, "frames_per_peak": FRAMES_PER_PEAK, "peaks": base64(&peaks) }),
    ))
}

/// Peaks for a file in the audio folder, from its cache file when that is current. The cache
/// sits in the project's `cache/peaks/<name>.peaks` (02-architecture.md, "Project store"): the
/// file's frame count (8 bytes, little-endian), then one byte per block. Delete it and it is rebuilt.
fn cached_peaks(path: &Path) -> Result<Vec<u8>, DawError> {
    let frames = u64::from(WavStream::open(path)?.r.duration());
    let project = path.parent().and_then(Path::parent).unwrap_or(Path::new("."));
    let cache = project.join("cache").join("peaks").join(format!("{}.peaks", path.file_name().and_then(|n| n.to_str()).unwrap_or("x")));
    if let Ok(bytes) = std::fs::read(&cache)
        && bytes.len() >= 8
        && bytes[..8] == frames.to_le_bytes()
        && (bytes.len() - 8) as u64 == frames.div_ceil(FRAMES_PER_PEAK as u64)
    {
        return Ok(bytes[8..].to_vec());
    }
    let peaks = peaks_of(path)?;
    // Best effort: a read-only project folder still works, it just recomputes next time.
    if cache.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) {
        let tmp = cache.with_extension("part");
        let mut bytes = frames.to_le_bytes().to_vec();
        bytes.extend_from_slice(&peaks);
        if std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &cache)).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    Ok(peaks)
}

/// Loudest sample of each 256-frame block, over both channels, as 0-255 (linear). Streams.
fn peaks_of(path: &Path) -> Result<Vec<u8>, DawError> {
    let mut r = WavStream::open(path)?;
    let frames = u64::from(r.r.duration());
    let (mut l, mut rr) = (vec![0.0f32; FRAMES_PER_PEAK], vec![0.0f32; FRAMES_PER_PEAK]);
    let mut out = Vec::with_capacity(frames.div_ceil(FRAMES_PER_PEAK as u64) as usize);
    let mut pos = 0;
    while pos < frames {
        let n = FRAMES_PER_PEAK.min((frames - pos) as usize);
        r.read(pos, &mut l[..n], &mut rr[..n])?;
        let m = l[..n].iter().chain(&rr[..n]).fold(0.0f32, |m, v| m.max(v.abs()));
        out.push((m.min(1.0) * 255.0).round() as u8);
        pos += n as u64;
    }
    Ok(out)
}

/// The same outline, for sound held in memory.
fn peaks_from(d: &Decoded) -> Vec<u8> {
    let (a, b) = (&d.ch[0], d.ch.get(1).unwrap_or(&d.ch[0]));
    a.chunks(FRAMES_PER_PEAK)
        .zip(b.chunks(FRAMES_PER_PEAK))
        .map(|(x, y)| (x.iter().chain(y).fold(0.0f32, |m, v| m.max(v.abs())).min(1.0) * 255.0).round() as u8)
        .collect()
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        for (k, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if k <= c.len() { T[(n >> shift & 63) as usize] as char } else { '=' });
        }
    }
    out
}

// ---------------------------------------------------------------- the mix

/// Linear gain for a dB value. 0 dB is exactly 1.0. Uses Crucible's own exp, so every machine
/// computes the same factor (priority 5).
fn gain(db: f64) -> f32 {
    crucible_dsp::math::db_to_gain(db) as f32
}

/// Balance pan (D-030): (left, right) gains. Centre is (1, 1) exactly; hard sides are exactly 0.
pub(crate) fn pan_gains(pan: f64) -> (f32, f32) {
    let fade = |p: f64| if p >= 1.0 { 0.0 } else if p <= 0.0 { 1.0 } else { crucible_dsp::math::cos_turns(p / 4.0) as f32 };
    (fade(pan), fade(-pan))
}

/// One audio clip, ready to mix: which file, where it plays on the timeline and how loud.
pub(crate) struct Voice {
    src: Src,
    /// Timeline frame where the clip's first audible frame plays.
    at: u64,
    /// First frame inside the file, and how many frames play.
    from: u64,
    len: u64,
    /// Clip gain. Track gain, pan and master come after the track's effects.
    gain: f32,
}

impl Voice {
    /// The timeline frame just after the clip.
    pub(crate) fn end(&self) -> u64 {
        self.at + self.len
    }
    pub(crate) fn src(&self) -> &Src {
        &self.src
    }
}

/// One audible track: its clips (a range of `Mixer::voices`), its effects, its gain and pan.
struct Strip {
    /// The track's id, for meters.
    track: u32,
    voices: std::ops::Range<usize>,
    effects: Vec<Box<dyn crucible_dsp::Effect>>,
    /// Per effect: its id in the project, and whether it is a stand-in for one the running
    /// mixer already holds (`Mixer::inherit`).
    ids: Vec<(u32, bool)>,
    /// Loudest sample on each side since the meters were last read.
    pub(crate) peak: (f32, f32),
    /// Track gain x pan x master, per side.
    out: (f32, f32),
    buf: (Vec<f32>, Vec<f32>),
    /// Samples the effects keep sounding after the last clip ends.
    tail: u64,
    /// An instrument track's plugin and notes (D-044). Such a strip has no voices.
    inst: Option<Box<crate::rack::InstSource>>,
    /// Where the instrument's last clip note has rung out.
    inst_end: u64,
    /// Where the audio thread leaves this track's level for the meters (live strips only).
    meter: Option<Arc<crate::rack::Meter>>,
}

/// Which tracks a mixer is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    /// The audio clips: what playback's feeder mixes ahead of time.
    Tape,
    /// The instruments: mixed in the audio device's own callback, so a key is heard at once.
    Live,
    /// Everything, for an export.
    All,
}

/// What the audio thread hands a live mixer before each block.
#[derive(Default)]
pub(crate) struct LiveIn {
    /// The song is running: clips' notes play. Otherwise only the keyboards do.
    pub rolling: bool,
    /// Keys played since the last block. Never grows past the room it was made with.
    pub keys: Vec<crate::rack::Key>,
}

/// Everything needed to mix a project, built once (it allocates and reads impulse responses),
/// then run block by block by `render` without allocating.
pub(crate) struct Mixer {
    pub voices: Vec<Voice>,
    strips: Vec<Strip>,
    block: usize,
    pub live: LiveIn,
    /// Mixers the audio thread is done with are strung together here until another thread
    /// frees them (`play.rs`): the audio thread itself never frees memory.
    pub next: Option<Box<Mixer>>,
}

/// Build the mixer. Muted tracks are left out; if any track is soloed, only soloed tracks play.
/// MIDI clips are silent until instruments exist (M3); placeholder audio clips have no file. Each
/// file's header is checked here, so a missing or changed file stops an export before it starts.
///
/// `held` is for playback: the effects the running mixer holds, by id, with the settings and
/// tempo they were built from. An effect found there unchanged is not built again; a stand-in
/// takes its place until `Mixer::inherit` swaps the running one in, sound and all.
pub(crate) fn mixer(p: &Project, folder: Option<&Path>, block: usize, held: Option<&Held>, sources: &Sources) -> Result<Mixer, DawError> {
    mixer_for(p, folder, block, held, sources, Part::Tape, None)
}

/// The mixer for one part of the project. Instruments come from `rack`.
///
/// A live mixer holds a strip for every loaded instrument, also a muted one: the plugin must
/// hear every note-off even while silenced, or it would resume with notes stuck on.
pub(crate) fn mixer_for(p: &Project, folder: Option<&Path>, block: usize, held: Option<&Held>, sources: &Sources, part: Part, rack: Option<&crate::rack::Rack>) -> Result<Mixer, DawError> {
    let any_solo = p.tracks.iter().any(|t| t.solo);
    let fpt = p.frames_per_tick();
    let rate = f64::from(p.sample_rate);
    let mut checked = std::collections::HashMap::<PathBuf, (Src, u64)>::new();
    let (mut voices, mut strips) = (Vec::new(), Vec::new());
    for t in p.tracks.iter() {
        let audible = !t.mute && (!any_solo || t.solo);
        let inst = if part != Part::Tape && (audible || part == Part::Live) { rack.and_then(|r| r.source(p, t, part == Part::All)) } else { None };
        if (part == Part::Live && inst.is_none()) || (part != Part::Live && !audible) {
            continue;
        }
        let first = voices.len();
        for c in p.clips.iter().filter(|c| part != Part::Live && c.track == t.id && c.has_audio_file()) {
            let Content::Audio { source, frames, .. } = &c.content else { continue };
            let folder = folder.ok_or_else(|| DawError::new(Code::ProjectNotSaved, "the project has audio but no folder"))?;
            let path = folder.join(AUDIO_DIR).join(source);
            // A WAV at the project's rate streams from disk; anything else plays from memory,
            // converted to the project's rate if it has another one (agreed to when it was placed).
            let (src, file_frames) = match checked.get(&path) {
                Some(x) => x.clone(),
                None => {
                    if !path.is_file() {
                        return Err(DawError::new(Code::AudioMissing, path.display().to_string()));
                    }
                    let x = match plain_wav(&path, p.sample_rate) {
                        Some(n) => (Src::File(path.clone()), n.min(*frames)),
                        None => {
                            let d = sources.get(&path, p.sample_rate)?;
                            let n = d.frames();
                            (Src::Memory(d), n)
                        }
                    };
                    checked.insert(path.clone(), x.clone());
                    x
                }
            };
            let from = (c.offset as f64 * fpt).round() as u64;
            let len = ((c.length as f64 * fpt).round() as u64).min(file_frames.saturating_sub(from));
            voices.push(Voice { src, at: (c.start as f64 * fpt).round() as u64, from, len, gain: gain(c.gain_db) });
        }
        if voices.len() == first && inst.is_none() {
            continue; // nothing on this track makes sound yet
        }
        let (mut effects, mut ids) = (Vec::new(), Vec::new());
        for f in t.effects.iter().filter(|f| !f.bypass) {
            let same = held.and_then(|h| h.get(&f.id)).is_some_and(|(old, tempo)| old == f && *tempo == p.tempo_bpm.to_bits());
            // Playback only: an IR Reverb that has no file yet passes sound through, instead of
            // stopping the music the moment it is added. An export still refuses it.
            let no_file_yet = held.is_some() && f.file.is_none() && crucible_dsp::kind(&f.kind).is_some_and(|k| k.needs_file);
            effects.push(if same || no_file_yet { Box::new(crucible_dsp::Hole) } else { super::fx::build(f, p, folder, rate)? });
            ids.push((f.id, same));
        }
        let tail = effects.iter().map(|e| e.tail()).sum();
        let (l, r) = pan_gains(t.pan);
        let g = if audible { gain(t.gain_db) * gain(p.master_gain_db) } else { 0.0 };
        let inst_end = inst.as_ref().map_or(0, |i| i.end(p.sample_rate));
        let meter = (part == Part::Live).then(|| Arc::new(crate::rack::Meter { track: t.id, peak: Default::default() }));
        strips.push(Strip { track: t.id, voices: first..voices.len(), effects, ids, peak: (0.0, 0.0), out: (g * l, g * r), buf: (vec![0.0; block], vec![0.0; block]), tail, inst, inst_end, meter });
    }
    Ok(Mixer { voices, strips, block, live: LiveIn { rolling: part == Part::All, keys: Vec::with_capacity(256) }, next: None })
}

impl Mixer {
    /// A mixer with nothing in it: what the audio thread holds before the first real one.
    pub(crate) fn empty() -> Mixer {
        Mixer { voices: Vec::new(), strips: Vec::new(), block: 0, live: LiveIn::default(), next: None }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.strips.is_empty()
    }

    /// The live strips' meters, for whoever shows them.
    pub(crate) fn meters(&self) -> Vec<Arc<crate::rack::Meter>> {
        self.strips.iter().filter_map(|s| s.meter.clone()).collect()
    }

    /// The audio thread, after a block: leave each track's level where the meters read it.
    pub(crate) fn publish_peaks(&mut self) {
        use std::sync::atomic::Ordering::Relaxed;
        for s in &mut self.strips {
            if let Some(m) = &s.meter {
                m.peak[0].fetch_max(s.peak.0.to_bits(), Relaxed);
                m.peak[1].fetch_max(s.peak.1.to_bits(), Relaxed);
            }
            s.peak = (0.0, 0.0);
        }
    }
}

/// Effects a running mixer holds: id to (settings, tempo bits).
pub(crate) type Held = std::collections::HashMap<u32, (crucible_project::Effect, u64)>;

impl Mixer {
    /// What this mixer's effects were built from, to pass as `held` when building its successor.
    pub(crate) fn held(&self, p: &Project) -> Held {
        let ids: std::collections::HashSet<u32> = self.strips.iter().flat_map(|s| s.ids.iter().map(|i| i.0)).collect();
        p.tracks.iter().flat_map(|t| &t.effects).filter(|f| ids.contains(&f.id)).map(|f| (f.id, (f.clone(), p.tempo_bpm.to_bits()))).collect()
    }

    /// Playback, after an edit: take the running effects out of `old`, the mixer this one
    /// replaces. An unchanged effect moves over whole; a changed one was built fresh and adopts
    /// the old one's sound. Pointer swaps only.
    pub(crate) fn inherit(&mut self, old: &mut Mixer) {
        for s in &mut self.strips {
            for (k, (id, stand_in)) in s.ids.iter_mut().enumerate() {
                let Some((o, j)) = old.strips.iter_mut().find_map(|o| o.ids.iter().position(|i| i.0 == *id).map(|j| (o, j))) else { continue };
                if *stand_in {
                    std::mem::swap(&mut s.effects[k], &mut o.effects[j]);
                    // `old` may itself never have run (a mixer replaced before the audio thread
                    // took it): then what it held for this effect was a stand-in too.
                    *stand_in = o.ids[j].1;
                } else {
                    s.effects[k].adopt(&mut *o.effects[j]);
                }
            }
            if let Some(new) = &mut s.inst
                && let Some(was) = old.strips.iter().find_map(|o| o.inst.as_deref().filter(|i| i.track() == new.track()))
            {
                new.adopt(was);
            }
        }
    }

    /// Each audible track's id and its peak since the last call.
    pub(crate) fn take_peaks(&mut self, into: &mut Vec<(u32, f32, f32)>) {
        for s in &mut self.strips {
            match into.iter_mut().find(|m| m.0 == s.track) {
                Some(m) => (m.1, m.2) = (m.1.max(s.peak.0), m.2.max(s.peak.1)),
                None => into.push((s.track, s.peak.0, s.peak.1)),
            }
            s.peak = (0.0, 0.0);
        }
    }
    /// The frame just after the last sound, effect tails included.
    pub(crate) fn end_frame(&self) -> u64 {
        self.strips.iter().map(|s| self.voices[s.voices.clone()].iter().map(|v| v.at + v.len).max().unwrap_or(0).max(s.inst_end) + s.tail).max().unwrap_or(0)
    }

    /// The frame just after the last clip, without tails.
    pub(crate) fn clips_end(&self) -> u64 {
        self.voices.iter().map(|v| v.at + v.len).max().unwrap_or(0)
    }
}

/// Mix timeline frames `start .. start + left.len()` (at most the mixer's block size) into `left`
/// and `right`, overwriting them. Per track: its clips are summed, its effects run, then its gain
/// and pan, into the master. `fetch(voice, file_frame, l, r)` supplies a clip's samples into the
/// scratch buffers. Nothing here allocates or touches files itself, so the audio callback can use
/// it with a fetch that reads buffers a disk thread keeps filled (one render path, rule 5).
pub(crate) fn render(
    m: &mut Mixer,
    start: u64,
    left: &mut [f32],
    right: &mut [f32],
    scratch: (&mut [f32], &mut [f32]),
    mut fetch: impl FnMut(usize, u64, &mut [f32], &mut [f32]) -> Result<(), DawError>,
) -> Result<(), DawError> {
    let n = left.len();
    debug_assert!(n <= m.block);
    left.fill(0.0);
    right.fill(0.0);
    let end = start + n as u64;
    let Mixer { voices, strips, live, .. } = m;
    for s in strips {
        let (tl, tr) = (&mut s.buf.0[..n], &mut s.buf.1[..n]);
        tl.fill(0.0);
        tr.fill(0.0);
        if let Some(inst) = &mut s.inst {
            inst.fill(start, live.rolling, &live.keys, tl, tr);
        }
        for i in s.voices.clone() {
            let v = &voices[i];
            let (a, b) = (v.at.max(start), (v.at + v.len).min(end));
            if a >= b {
                continue;
            }
            let k = (b - a) as usize;
            let (sl, sr) = (&mut scratch.0[..k], &mut scratch.1[..k]);
            fetch(i, v.from + (a - v.at), sl, sr)?;
            let o = (a - start) as usize;
            for j in 0..k {
                tl[o + j] += sl[j] * v.gain;
                tr[o + j] += sr[j] * v.gain;
            }
        }
        for e in &mut s.effects {
            e.process(start, tl, tr);
        }
        let (mut pl, mut pr) = s.peak;
        for j in 0..n {
            let (a, b) = (tl[j] * s.out.0, tr[j] * s.out.1);
            left[j] += a;
            right[j] += b;
            pl = pl.max(a.abs());
            pr = pr.max(b.abs());
        }
        s.peak = (pl, pr);
    }
    Ok(())
}

/// Small deterministic noise source for dither, so two exports of one project are identical.
struct Xorshift(u64);
impl Xorshift {
    fn unit(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn project_export(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let file = PathBuf::from(a.need_text("file")?);
    let bits = a.text("bits").unwrap_or("24").to_string();
    let dither = bits == "16" && a.text("dither") != Some("off");
    let p = &e.project;
    let fpt = p.frames_per_tick();
    let mut m = mixer_for(p, e.work(), BLOCK, None, &e.sources, Part::All, Some(&e.rack))?;
    let start = (super::clips::ticks(a.number("start").unwrap_or(0.0))?.max(0) as f64 * fpt).round() as u64;
    let end = match a.number("end") {
        Some(b) => (super::clips::ticks(b)? as f64 * fpt).round() as u64,
        None => m.end_frame().max(set_end(p)),
    };
    if end <= start {
        return Err(DawError::new(
            Code::ExportFailed,
            if a.number("end").is_none() { "nothing to export: no audible audio clips, and no notes on a track with a loaded instrument".to_string() } else { "the end is not after the start".to_string() },
        ));
    }
    let fail = |why: String| DawError::new(Code::ExportFailed, format!("{}: {why}", file.display()));
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: p.sample_rate,
        bits_per_sample: match bits.as_str() { "16" => 16, "24" => 24, _ => 32 },
        sample_format: if bits == "32f" { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
    };
    // Written beside the target and renamed at the end, so a failed export never leaves a
    // half-file under the real name.
    let tmp = file.with_extension("wav.part");
    let mut w = hound::WavWriter::create(&tmp, spec).map_err(|err| fail(err.to_string()))?;
    let full = match bits.as_str() { "16" => 32768.0f32, "24" => 8_388_608.0, _ => 1.0 };
    let (mut left, mut right, mut sl, mut sr) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
    // One reader per clip, opened when the clip starts and closed when it ends.
    let mut readers: Vec<Option<Reader>> = m.voices.iter().map(|_| None).collect();
    let spans: Vec<(u64, Src)> = m.voices.iter().map(|v| (v.at + v.len, v.src.clone())).collect();
    let mut noise = Xorshift(0x9E37_79B9_7F4A_7C15);
    let (mut peak, mut overs) = (0.0f32, 0u64);
    let mut pos = start;
    let t0 = std::time::Instant::now();
    // Instruments leave the live sound for the length of the export and are told it is one, so
    // a sampled instrument takes the time it needs per block instead of skipping notes.
    let instruments = e.rack.any_loaded();
    if instruments {
        super::play::pause_live(e, true);
        e.rack.offline(true, p.sample_rate);
    }
    let result = (|| -> Result<(), DawError> {
        while pos < end {
            let n = BLOCK.min((end - pos) as usize);
            render(&mut m, pos, &mut left[..n], &mut right[..n], (&mut sl, &mut sr), |i, from, l, r| {
                if readers[i].is_none() {
                    readers[i] = Some(Reader::open(&spans[i].1)?);
                }
                readers[i].as_mut().expect("opened above").read(from, l, r)
            })?;
            for (i, (until, _)) in spans.iter().enumerate() {
                if *until <= pos + n as u64 {
                    readers[i] = None;
                }
            }
            for k in 0..n {
                for s in [left[k], right[k]] {
                    peak = peak.max(s.abs());
                    let r = if bits == "32f" {
                        w.write_sample(s)
                    } else {
                        let d = if dither { noise.unit() - noise.unit() } else { 0.0 };
                        let v = (s * full + d).round();
                        if v > full - 1.0 || v < -full {
                            overs += 1;
                        }
                        w.write_sample(v.clamp(-full, full - 1.0) as i32)
                    };
                    r.map_err(|err| fail(err.to_string()))?;
                }
            }
            pos += n as u64;
        }
        Ok(())
    })();
    let (mixed_end, clips_end) = (m.end_frame(), m.clips_end());
    drop(m);
    if instruments {
        e.rack.offline(false, e.project.sample_rate);
        super::play::pause_live(e, false);
    }
    let p = &e.project;
    if let Err(err) = result {
        drop(w);
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    w.finalize().map_err(|err| fail(err.to_string()))?;
    std::fs::rename(&tmp, &file).map_err(|err| fail(err.to_string()))?;
    let frames = end - start;
    let secs = frames as f64 / f64::from(p.sample_rate);
    let tail_s = if a.number("end").is_none() && clips_end > 0 { mixed_end.saturating_sub(clips_end) as f64 / f64::from(p.sample_rate) } else { 0.0 };
    let peak_db = if peak > 0.0 { 20.0 * f64::from(peak).log10() } else { f64::NEG_INFINITY };
    if overs > 0 {
        diag::warn(Code::ExportClipped, format!("{}: {overs} samples clipped, peak {peak_db:+.2} dBFS", file.display()));
    }
    let format = match bits.as_str() { "16" => if dither { "16-bit, dithered" } else { "16-bit, no dither" }, "24" => "24-bit", _ => "32-bit float" };
    let mut text = format!(
        "Exported {} ({format}, {} Hz, {secs:.2} s{}, peak {}) in {:.2} s",
        file.display(),
        p.sample_rate,
        if tail_s > 0.0 { format!(" including a {tail_s:.1} s effects tail") } else { String::new() },
        if peak > 0.0 { format!("{peak_db:+.2} dBFS") } else { "silence".to_string() },
        t0.elapsed().as_secs_f64()
    );
    if overs > 0 {
        text.push_str(&format!("\nW5020: {overs} samples were above full scale and were clipped. Lower the master by {peak_db:.1} dB or export 32-bit float."));
    }
    Ok(Reply::new(
        text,
        json!({ "file": file, "frames": frames, "seconds": secs, "tail_seconds": tail_s, "format": format, "peak_dbfs": if peak > 0.0 { Some(peak_db) } else { None }, "clipped": overs }),
    ))
}

#[cfg(test)]
mod tests {
    //! The machine-checkable tests from docs/04-audio-quality.md, plus the refusals.
    use super::*;
    use crate::command;

    fn run(e: &mut Engine, line: &str) -> Result<Reply, DawError> {
        let mut last = Reply::text("");
        for t in command::split_statements(line).unwrap() {
            last = e.execute(&t)?;
        }
        Ok(last)
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crucible-audio-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Write interleaved samples. Integer depths are written exactly from values on their grid.
    fn wav(path: &Path, rate: u32, channels: u16, bits: &str, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: match bits { "16" => 16, "24" => 24, _ => 32 },
            sample_format: if bits == "32f" { hound::SampleFormat::Float } else { hound::SampleFormat::Int },
        };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        let full = match bits { "16" => 32768.0f32, "24" => 8_388_608.0, _ => 1.0 };
        for &s in samples {
            if bits == "32f" {
                w.write_sample(s).unwrap()
            } else {
                w.write_sample(((s * full).round() as i32).clamp(-full as i32, full as i32 - 1)).unwrap()
            }
        }
        w.finalize().unwrap();
    }

    fn noise(n: usize) -> Vec<f32> {
        let mut x = Xorshift(12345);
        (0..n).map(|_| (x.unit() - 0.5) * 1.6).collect()
    }

    fn ints(path: &Path) -> Vec<i32> {
        hound::WavReader::open(path).unwrap().samples::<i32>().map(Result::unwrap).collect()
    }
    fn floats(path: &Path) -> Vec<f32> {
        hound::WavReader::open(path).unwrap().samples::<f32>().map(Result::unwrap).collect()
    }

    /// A saved project with one audio track, its folder, and a folder for input files.
    fn project(name: &str) -> (Engine, PathBuf, PathBuf) {
        let root = temp(name);
        let (dir, inputs) = (root.join("song"), root.join("in"));
        std::fs::create_dir_all(&inputs).unwrap();
        let mut e = Engine::new();
        run(&mut e, &format!("track.add Gtr; project.save \"{}\"", dir.display())).unwrap();
        (e, dir, inputs)
    }

    fn import_place(e: &mut Engine, file: &Path, track: &str, at: f64) -> String {
        let r = run(e, &format!("audio.import \"{}\"", file.display())).unwrap();
        let source = r.data["source"].as_str().unwrap().to_string();
        run(e, &format!("clip.place {track} {at} \"{source}\"")).unwrap();
        source
    }

    #[test]
    fn null_test_every_format_round_trips_exactly() {
        for bits in ["16", "24", "32f"] {
            let (mut e, dir, inp) = project(&format!("null{bits}"));
            let src = inp.join("take.wav");
            wav(&src, 48_000, 2, bits, &noise(2 * 20_000));
            import_place(&mut e, &src, "Gtr", 0.0);
            let out = dir.join("mix.wav");
            run(&mut e, &format!("project.export \"{}\" --bits {bits} --dither off", out.display())).unwrap();
            if bits == "32f" {
                assert_eq!(floats(&src), floats(&out), "{bits}");
            } else {
                assert_eq!(ints(&src), ints(&out), "{bits}");
            }
            // The copy in the project's audio folder is the original, byte for byte.
            assert_eq!(std::fs::read(&src).unwrap(), std::fs::read(dir.join("audio").join("take.wav")).unwrap());
        }
    }

    #[test]
    fn gain_and_pan_laws() {
        let (mut e, dir, inp) = project("laws");
        let src = inp.join("dc.wav");
        wav(&src, 48_000, 1, "32f", &[0.5; 4800]);
        import_place(&mut e, &src, "Gtr", 0.0);
        let out = dir.join("x.wav");
        let export = |e: &mut Engine, setup: &str| -> (f32, f32) {
            run(e, setup).unwrap();
            run(e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
            let f = floats(&out);
            (f[100], f[101])
        };
        assert_eq!(export(&mut e, "track.gain Gtr 0"), (0.5, 0.5), "unity, centre: exact");
        let g = crucible_dsp::math::db_to_gain(-6.0) as f32;
        assert!((f64::from(g) / 10f64.powf(-0.3) - 1.0).abs() < 1e-7, "Crucible's exp agrees with the library's");
        assert_eq!(export(&mut e, "track.gain Gtr -6"), (0.5 * g, 0.5 * g), "gain law");
        run(&mut e, "track.gain Gtr 0").unwrap();
        assert_eq!(export(&mut e, "track.pan Gtr -100"), (0.5, 0.0), "hard left");
        assert_eq!(export(&mut e, "track.pan Gtr 100"), (0.0, 0.5), "hard right");
        let half = crucible_dsp::math::cos_turns(0.125) as f32;
        assert!((half - std::f64::consts::FRAC_PI_4.cos() as f32).abs() < 1e-7);
        assert_eq!(export(&mut e, "track.pan Gtr 50"), (0.5 * half, 0.5), "half right: left at -3 dB");
        assert_eq!(export(&mut e, "track.pan Gtr 0; master.gain -6"), (0.5 * g, 0.5 * g), "master gain");
    }

    #[test]
    fn silence_determinism_mute_and_solo() {
        let (mut e, dir, inp) = project("mix");
        let out = dir.join("a.wav");
        run(&mut e, &format!("project.export \"{}\" --end 4", out.display())).unwrap();
        assert!(ints(&out).iter().all(|&s| s == 0), "an empty project renders silence");
        assert!(run(&mut e, &format!("project.export \"{}\"", out.display())).is_err(), "nothing to export");

        let (a, b) = (inp.join("a.wav"), inp.join("b.wav"));
        wav(&a, 48_000, 1, "24", &[0.25; 4800]);
        wav(&b, 48_000, 1, "24", &[0.125; 4800]);
        run(&mut e, "track.add Keys").unwrap();
        import_place(&mut e, &a, "Gtr", 0.0);
        import_place(&mut e, &b, "Keys", 0.0);
        let level = |e: &mut Engine| {
            run(e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
            floats(&out)[10]
        };
        assert_eq!(level(&mut e), 0.375, "both tracks sum");
        run(&mut e, "track.solo Keys").unwrap();
        assert_eq!(level(&mut e), 0.125, "solo plays only Keys");
        run(&mut e, "track.mute Keys").unwrap();
        assert!(run(&mut e, &format!("project.export \"{}\"", out.display())).is_err(), "soloed but muted: nothing audible");
        run(&mut e, "track.mute Keys --state off; track.solo Keys --state off").unwrap();

        // Determinism, including the 16-bit dither noise.
        let (x, y) = (dir.join("x.wav"), dir.join("y.wav"));
        run(&mut e, &format!("project.export \"{}\" --bits 16", x.display())).unwrap();
        run(&mut e, &format!("project.export \"{}\" --bits 16", y.display())).unwrap();
        assert_eq!(std::fs::read(&x).unwrap(), std::fs::read(&y).unwrap());
    }

    /// Q12, decided by Dzyan 2026-11-23: overlapping clips on one track both play (they are
    /// summed). The later clip does not silence the earlier one.
    #[test]
    fn overlapping_clips_on_one_track_both_play() {
        let (mut e, dir, inp) = project("overlap");
        let (a, b) = (inp.join("a.wav"), inp.join("b.wav"));
        wav(&a, 48_000, 1, "24", &[0.25; 4800]);
        wav(&b, 48_000, 1, "24", &[0.125; 4800]);
        import_place(&mut e, &a, "Gtr", 0.0);
        import_place(&mut e, &b, "Gtr", 0.1); // 0.1 beat at 120 BPM and 48 kHz: 2,400 samples in
        let out = dir.join("o.wav");
        run(&mut e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
        let s = floats(&out);
        let at = |frame: usize| s[frame * s.len() / 7200];
        assert_eq!(s.len() % 7200, 0, "7,200 frames: the first clip's start to the second clip's end");
        assert_eq!((at(10), at(2410), at(4810)), (0.25, 0.375, 0.125), "first alone, both, second alone");
    }

    #[test]
    fn split_renders_the_same_audio_and_tempo_keeps_duration() {
        let (mut e, dir, inp) = project("edit");
        let src = inp.join("n.wav");
        wav(&src, 48_000, 2, "24", &noise(2 * 48_000)); // one second of stereo
        import_place(&mut e, &src, "Gtr", 1.0);
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        run(&mut e, &format!("project.export \"{}\"", a.display())).unwrap();
        run(&mut e, "clip.split #1 1.5").unwrap();
        run(&mut e, &format!("project.export \"{}\"", b.display())).unwrap();
        assert_eq!(ints(&a), ints(&b), "a split clip plays the same samples");

        // At 120 BPM one second is two beats. At 60 BPM the clip stays on beat 1 and is one beat long,
        // so it now starts one second in instead of half a second: the same audio, later.
        run(&mut e, "undo; project.tempo 60").unwrap();
        let c = &e.project().clips[0];
        assert_eq!((c.start, c.length), (960, 960));
        run(&mut e, &format!("project.export \"{}\"", b.display())).unwrap();
        let (before, after) = (ints(&a), ints(&b));
        assert_eq!(after.len(), 2 * 96_000);
        assert_eq!(&after[2 * 48_000..], &before[2 * 24_000..], "same samples, starting one second in");
    }

    /// D-042: a project never saved takes files at once; saving takes its audio along, and Save as
    /// to another folder does too (before 2026-11-26 it wrote only project.json).
    #[test]
    fn an_unsaved_project_takes_its_audio_along() {
        let root = temp("unsaved");
        let ok = root.join("ok.wav");
        wav(&ok, 48_000, 1, "16", &noise(4_800));
        let mut e = Engine::new();
        run(&mut e, "track.add Gtr").unwrap();
        let source = import_place(&mut e, &ok, "Gtr", 0.0);
        let scratch = e.work().unwrap().to_path_buf();
        assert!(scratch.join("audio").join(&source).is_file(), "imported into the unsaved project's own folder");
        assert!(e.folder().is_none() && e.is_dirty());
        let before = root.join("before.wav");
        run(&mut e, &format!("project.export \"{}\"", before.display())).unwrap();

        let song = root.join("My Song");
        let r = run(&mut e, &format!("project.save \"{}\"", song.display())).unwrap();
        assert!(r.text.contains("1 audio file copied"), "{}", r.text);
        assert!(song.join("audio").join(&source).is_file(), "the audio went along");
        assert!(!scratch.exists(), "the unsaved project's folder is gone");
        let after = root.join("after.wav");
        run(&mut e, &format!("project.export \"{}\"", after.display())).unwrap();
        assert_eq!(ints(&before), ints(&after), "the saved project plays the same");

        let copy = root.join("Copy");
        run(&mut e, &format!("project.save \"{}\"", copy.display())).unwrap();
        let mut reopened = Engine::new();
        run(&mut reopened, &format!("project.open \"{}\"", copy.display())).unwrap();
        let again = root.join("again.wav");
        run(&mut reopened, &format!("project.export \"{}\"", again.display())).unwrap();
        assert_eq!(ints(&before), ints(&again), "Save as made a copy that opens whole");

        // D-043: the end moved out past the last sound makes a whole-song export that long;
        // nearer than the last clip means "at the last sound" again.
        run(&mut reopened, "project.end 8").unwrap(); // 8 beats at 120 BPM = 4 s = 192,000 stereo frames
        run(&mut reopened, &format!("project.export \"{}\"", again.display())).unwrap();
        assert_eq!(ints(&again).len(), 2 * 192_000);
        assert_eq!(run(&mut reopened, "project.end 0.01").unwrap().data["end"], 0.0);

        // Discarding an unsaved project removes its folder.
        let mut d = Engine::new();
        run(&mut d, "track.add Gtr").unwrap();
        import_place(&mut d, &ok, "Gtr", 0.0);
        let gone = d.work().unwrap().to_path_buf();
        run(&mut d, "project.new --discard").unwrap();
        assert!(!gone.exists());
    }

    /// Removing the last clip that uses a file lets its sound out of memory (2026-11-26); an
    /// undone removal plays the same again.
    #[test]
    fn sound_no_clip_uses_is_let_go() {
        let (mut e, dir, inp) = project("letgo");
        let cd = inp.join("cd.wav");
        wav(&cd, 44_100, 2, "16", &noise(2 * 4_410));
        let r = run(&mut e, &format!("audio.import \"{}\" --convert", cd.display())).unwrap();
        let source = r.data["source"].as_str().unwrap().to_string();
        run(&mut e, &format!("clip.place Gtr 0 \"{source}\" --convert")).unwrap();
        let (a, b) = (dir.join("a.wav"), dir.join("b.wav"));
        run(&mut e, &format!("project.export \"{}\"", a.display())).unwrap();
        assert!(e.sources.megabytes() > 0.0, "a converted file plays from memory");
        run(&mut e, "track.add Other").unwrap();
        assert!(e.sources.megabytes() > 0.0, "an edit that removes nothing lets nothing go");
        run(&mut e, "clip.remove #1").unwrap();
        assert_eq!(e.sources.megabytes(), 0.0, "removed: let go");
        run(&mut e, "undo").unwrap();
        run(&mut e, &format!("project.export \"{}\"", b.display())).unwrap();
        assert_eq!(ints(&a), ints(&b), "undone: read again, the same");
    }

    #[test]
    fn refusals_are_coded() {
        let (mut e, dir, inp) = project("refuse2");
        let f441b = inp.join("cd.wav");
        wav(&f441b, 44_100, 2, "16", &[0.0; 200]);
        assert_eq!(run(&mut e, &format!("audio.import \"{}\"", f441b.display())).unwrap_err().code, Code::SampleRateMismatch);
        let ok = inp.join("ok.wav");
        wav(&ok, 48_000, 1, "16", &[0.1; 200]);
        let source = import_place(&mut e, &ok, "Gtr", 0.0);
        run(&mut e, "track.add Synth --kind instrument").unwrap();
        assert_eq!(run(&mut e, &format!("clip.place Synth 0 \"{source}\"")).unwrap_err().code, Code::ClipEditRefused);
        assert_eq!(run(&mut e, "project.rate 44100").unwrap_err().code, Code::SampleRateMismatch);
        assert_eq!(run(&mut e, "clip.place Gtr 0 ../ok.wav").unwrap_err().code, Code::BadArgument);
        // The same file again reuses the copy; a different file with the same name gets a new name.
        let again = run(&mut e, &format!("audio.import \"{}\"", ok.display())).unwrap();
        assert_eq!(again.data["source"], "ok.wav");
        wav(&ok, 48_000, 1, "16", &[0.2; 200]);
        assert_eq!(run(&mut e, &format!("audio.import \"{}\"", ok.display())).unwrap().data["source"], "ok 2.wav");
        // A missing file is named when exporting.
        std::fs::remove_file(dir.join("audio").join("ok.wav")).unwrap();
        let err = run(&mut e, &format!("project.export \"{}\"", dir.join("m.wav").display())).unwrap_err();
        assert_eq!(err.code, Code::AudioMissing);
        assert!(err.message.contains("ok.wav"));
    }

    #[test]
    fn render_allocates_nothing() {
        let (mut e, dir, inp) = project("noalloc");
        let (a, b) = (inp.join("a.wav"), inp.join("b.wav"));
        wav(&a, 48_000, 2, "24", &noise(2 * 10_000));
        wav(&b, 48_000, 1, "16", &noise(10_000));
        run(&mut e, "track.add Keys; track.pan Keys 30; track.gain Gtr -4; fx.add Gtr reverb; fx.add Keys echo").unwrap();
        import_place(&mut e, &a, "Gtr", 0.0);
        import_place(&mut e, &b, "Keys", 0.25);
        let mut m = mixer(e.project(), Some(&dir), 512, None, &e.sources).unwrap();
        // The callback hands out preloaded samples, as a disk thread's buffers will.
        let data: Vec<Vec<f32>> = m.voices.iter().map(|_| noise(20_000)).collect();
        let (mut l, mut r, mut sl, mut sr) = (vec![0.0f32; 512], vec![0.0f32; 512], vec![0.0f32; 512], vec![0.0f32; 512]);
        let n = crate::alloc_count::during(|| {
            for block in 0..30u64 {
                render(&mut m, block * 512, &mut l, &mut r, (&mut sl, &mut sr), |i, from, dl, dr| {
                    let s = &data[i][from as usize..from as usize + dl.len()];
                    dl.copy_from_slice(s);
                    dr.copy_from_slice(s);
                    Ok(())
                })
                .unwrap();
            }
        });
        assert_eq!(n, 0, "render allocated {n} times");
        assert!(crate::alloc_count::during(|| drop(std::hint::black_box(vec![1u8; 64]))) >= 1, "the counter itself works");
        assert!(l.iter().any(|v| *v != 0.0), "and it did mix something");
    }

    #[test]
    fn effects_are_commands_and_shape_the_export() {
        let (mut e, dir, inp) = project("fx");
        let f = inp.join("click.wav");
        let mut x = vec![0.0f32; 48_000];
        x[0] = 0.5;
        wav(&f, 48_000, 1, "32f", &x);
        import_place(&mut e, &f, "Gtr", 0.0);
        let out = dir.join("x.wav");
        let export = |e: &mut Engine| {
            let r = run(e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
            (floats(&out), r.data["tail_seconds"].as_f64().unwrap())
        };
        let (dry, tail) = export(&mut e);
        assert_eq!(tail, 0.0);
        assert_eq!(dry.len(), 2 * 48_000);

        // Echo: a quarter note at 120 BPM later, the click comes back.
        run(&mut e, "fx.add Gtr delay; fx.set Gtr 1 mix 100; fx.set Gtr 1 sync 1/4; fx.set Gtr 1 feedback 0").unwrap();
        let (wet, tail) = export(&mut e);
        assert!(tail > 0.4, "the export runs on for the echo ({tail} s)");
        assert_eq!(wet[0], 0.0, "mix 100: no dry click");
        assert!((wet[2 * 24_000] - 0.5).abs() < 1e-6, "echo exactly one beat later");

        // Bypass, undo, refusals and the reverb.
        run(&mut e, "fx.bypass Gtr 1").unwrap();
        assert_eq!(export(&mut e).0[0], 0.5, "bypassed: the dry click is back");
        run(&mut e, "undo; fx.add Gtr reverb --at 1; fx.set Gtr 1 character plate; fx.set Gtr 1 decay 1.5").unwrap();
        assert_eq!(e.project().tracks[0].effects.len(), 2);
        assert_eq!(e.project().tracks[0].effects[0].kind, "reverb");
        assert_eq!(run(&mut e, "fx.set Gtr 1 decay 99").unwrap_err().code, Code::EffectRefused);
        assert_eq!(run(&mut e, "fx.set Gtr 1 colour red").unwrap_err().code, Code::EffectRefused);
        assert_eq!(run(&mut e, "fx.remove Gtr 9").unwrap_err().code, Code::EffectNotFound);
        assert_eq!(run(&mut e, "fx.add Gtr chorus").unwrap_err().code, Code::EffectRefused);
        let (rev, _) = export(&mut e);
        assert!(rev.iter().all(|v| v.is_finite()));

        // IR Reverb: refused without a file, then a unit impulse IR passes the sound through.
        run(&mut e, "fx.remove Gtr 1; fx.remove Gtr 1; fx.add Gtr irreverb; fx.set Gtr 1 mix 100; fx.set Gtr 1 normalise off").unwrap();
        let err = run(&mut e, &format!("project.export \"{}\"", out.display())).unwrap_err();
        assert_eq!(err.code, Code::EffectRefused);
        let ir = inp.join("ir.wav");
        let mut h = vec![0.0f32; 4 * 1000];
        h[4 * 100] = 1.0; // 4 channels, left-to-left path, 100 samples in
        wav(&ir, 48_000, 4, "32f", &h);
        run(&mut e, &format!("audio.import \"{}\"; fx.file Gtr 1 ir.wav", ir.display())).unwrap();
        assert_eq!(run(&mut e, "clip.place Gtr 1 ir.wav").unwrap_err().code, Code::ClipEditRefused, "a 4-channel IR is not a clip");
        let (conv, _) = export(&mut e);
        assert!((conv[2 * 100] - 0.5).abs() < 1e-6 && conv[2 * 100 + 1] == 0.0, "left to left only, 100 samples late");

        // The effects survive a save and reopen.
        run(&mut e, "project.save").unwrap();
        let mut again = Engine::new();
        run(&mut again, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(again.project().tracks[0].effects, e.project().tracks[0].effects);
    }

    #[test]
    fn placing_audio_survives_a_crash() {
        let (mut e, dir, inp) = project("crash");
        let f = inp.join("t.wav");
        wav(&f, 48_000, 1, "24", &[0.1; 480]);
        import_place(&mut e, &f, "Gtr", 2.0);
        let expected = e.project().clone();
        drop(e); // never saved: the journal holds the place command, and the file is already in audio/
        let mut again = Engine::new();
        run(&mut again, &format!("project.open \"{}\"", dir.display())).unwrap();
        assert_eq!(again.project(), &expected);
    }

    #[test]
    fn peaks_and_base64() {
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"M"), "TQ==");
        let dir = temp("peaks");
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        let f = dir.join("audio").join("p.wav");
        let mut x = vec![0.0f32; 2 * 300];
        x[2 * 299 + 1] = 0.5; // right channel, last frame: the second block
        x[2 * 10] = -1.0; // left channel, first block
        wav(&f, 48_000, 2, "32f", &x);
        assert_eq!(peaks_of(&f).unwrap(), vec![255, 128]);
        // The cache file is written once and then used; a changed file invalidates it.
        let cache = dir.join("cache").join("peaks").join("p.wav.peaks");
        assert_eq!(cached_peaks(&f).unwrap(), vec![255, 128]);
        assert_eq!(std::fs::read(&cache).unwrap().len(), 8 + 2);
        wav(&f, 48_000, 2, "32f", &[0.0; 2 * 100]);
        assert_eq!(cached_peaks(&f).unwrap(), vec![0]);
    }

    /// The converter: exact length for the ratio, pitch kept, and an error far below hearing.
    /// A 1 kHz sine at 48 kHz converted to 192 kHz is compared with a perfect 1 kHz sine at
    /// 192 kHz, away from the ends (where any converter rings a little).
    #[test]
    fn conversion_is_clean_and_exact_in_length() {
        let sine = |rate: f64, n: usize| (0..n).map(|k| (0.5 * (2.0 * std::f64::consts::PI * 1000.0 * k as f64 / rate).sin()) as f32).collect::<Vec<f32>>();
        let x = sine(48_000.0, 48_000);
        let y = convert_channel(&x, 48_000, 192_000).unwrap();
        assert_eq!(y.len(), 192_000, "one second stays one second");
        let want = sine(192_000.0, 192_000);
        let (mut err, mut sig) = (0.0f64, 0.0f64);
        for k in 20_000..172_000 {
            err += (f64::from(y[k]) - f64::from(want[k])).powi(2);
            sig += f64::from(want[k]).powi(2);
        }
        let snr = 10.0 * (sig / err).log10();
        assert!(snr > 100.0, "the converted sine is off by more than -100 dB: {snr:.1} dB");
        let z = convert_channel(&sine(44_100.0, 44_100), 44_100, 48_000).unwrap();
        assert_eq!(z.len(), 48_000);
        assert_eq!(convert_channel(&x, 48_000, 48_000).unwrap().len(), 48_000, "same rate: same length");
    }

    /// A file at another rate is refused unless the person agrees; agreed, it plays converted, and
    /// the file in the project stays exactly as it was. The project's rate can change the same way.
    #[test]
    fn another_rate_is_converted_only_when_agreed() {
        let (mut e, dir, inp) = project("convert");
        let cd = inp.join("cd.wav");
        let tone: Vec<f32> = (0..44_100).flat_map(|k| { let v = (0.25 * (2.0 * std::f64::consts::PI * 441.0 * k as f64 / 44_100.0).sin()) as f32; [v, v] }).collect();
        wav(&cd, 44_100, 2, "24", &tone);
        assert_eq!(run(&mut e, &format!("audio.import \"{}\"", cd.display())).unwrap_err().code, Code::SampleRateMismatch);
        let r = run(&mut e, &format!("audio.import \"{}\" --convert", cd.display())).unwrap();
        assert_eq!(r.data["converted"], true);
        assert!(r.text.contains("converted to 48000 Hz"), "{}", r.text);
        assert_eq!(std::fs::read(dir.join("audio").join("cd.wav")).unwrap(), std::fs::read(&cd).unwrap(), "the file is kept byte for byte");
        assert_eq!(run(&mut e, "clip.place Gtr 0 cd.wav").unwrap_err().code, Code::SampleRateMismatch, "placing asks again");
        let placed = run(&mut e, "clip.place Gtr 0 cd.wav --convert").unwrap();
        assert!(placed.text.contains("converted from 44100 Hz"), "{}", placed.text);
        let out = dir.join("c.wav");
        run(&mut e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
        let s = floats(&out);
        assert_eq!(s.len() / 2, 48_000, "one second at 48 kHz");
        // 441 Hz: 882 sign changes a second, whatever the rate.
        let crossings = s.chunks(2).map(|f| f[0]).collect::<Vec<_>>().windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((878..=886).contains(&crossings), "pitch changed: {crossings} crossings");
        let peak = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.25).abs() < 0.01, "level changed: peak {peak}");
        // The project's rate: refused while files are at other rates, unless agreed.
        assert_eq!(run(&mut e, "project.rate 96000").unwrap_err().code, Code::SampleRateMismatch);
        assert!(run(&mut e, "project.rate 96000 --convert").unwrap().text.contains("converted"));
        run(&mut e, &format!("project.export \"{}\" --bits 32f", out.display())).unwrap();
        assert_eq!(floats(&out).len() / 2, 96_000, "the same second at 96 kHz");
        // Back at the file's own rate nothing is converted: the export is the file, exactly.
        run(&mut e, "project.rate 44100 --convert").unwrap();
        run(&mut e, &format!("project.export \"{}\" --bits 24", out.display())).unwrap();
        assert_eq!(ints(&out), ints(&cd), "at its own rate the file plays untouched");
        let _ = std::fs::remove_dir_all(dir);
    }
}
