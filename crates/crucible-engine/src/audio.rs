//! Audio files and the offline mix: import, place, waveform peaks, render, export.
//!
//! Rules from `docs/04-audio-quality.md` this file keeps:
//! - **Unity is exact.** 0 dB and centre pan multiply by exactly 1.0, so a lone clip at unity
//!   exports the same samples it imported (the null test).
//! - **No silent resampling.** A file at another sample rate is refused with E5017.
//! - **No hidden processing.** Dither only when the user exports 16-bit with it on (the default
//!   there, shown in the reply); no limiter, no normalising. Overs are clipped in fixed-point files
//!   and reported (W5020), never squashed.
//! - **One render path.** `render` mixes one block and gets each clip's samples through a
//!   callback: from files during export, from a disk thread's buffers during playback (M2).
//!
//! Memory: files are streamed in blocks, never loaded whole. A 10-minute stereo file would be
//! 230 MB as floats; a 20-track song several gigabytes.
//!
//! Pan law (D-030, proposed): balance. Centre passes both channels unchanged; turning toward one
//! side fades the other with a cosine, reaching silence at the hard side. Nothing is boosted.
//!
//! Imported files are copied into the project's `audio` folder, so a project folder is
//! self-contained and opens the same on every machine. Placing a file on a track (`clip.place`)
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
        args: &[req("file", Kind::Text, "path to a WAV, MP3 or FLAC file")],
        help: "Bring a file into the project's audio folder: WAV copied unchanged, MP3 and FLAC decoded once to float WAV. Then clip.place.",
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
pub(crate) struct Reader {
    r: hound::WavReader<BufReader<File>>,
    path: PathBuf,
    stereo: bool,
    /// `None` for float files; otherwise the factor that maps the integers to -1..1.
    scale: Option<f32>,
    pos: u64,
}

impl Reader {
    pub(crate) fn open(path: &Path) -> Result<Reader, DawError> {
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
        Ok(Reader { r, path: path.to_path_buf(), stereo: s.channels == 2, scale, pos: 0 })
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

/// Decode an MP3 or FLAC file into a 32-bit float WAV at `out`, streaming. Returns its rate,
/// channels and frames. A damaged frame is skipped, as players do.
fn decode_to_wav(path: &Path, out: &Path) -> Result<(u32, u16, u64), DawError> {
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
    let mut w: Option<(hound::WavWriter<std::io::BufWriter<File>>, u32, u16)> = None;
    let (mut buf, mut samples) = (Vec::<f32>::new(), 0u64);
    let write_err = |e: hound::Error| unreadable(out, e);
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
        if w.is_none() {
            let n = b.spec().channels().count() as u16;
            if !(1..=2).contains(&n) {
                return Err(unreadable(path, format!("{n} channels; Crucible reads mono and stereo files")));
            }
            let spec = hound::WavSpec { channels: n, sample_rate: b.spec().rate(), bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
            w = Some((hound::WavWriter::create(out, spec).map_err(write_err)?, b.spec().rate(), n));
        }
        buf.resize(b.samples_interleaved(), 0.0);
        b.copy_to_slice_interleaved(&mut buf);
        let (writer, _, _) = w.as_mut().expect("created above");
        for v in &buf {
            writer.write_sample(*v).map_err(write_err)?;
        }
        samples += buf.len() as u64;
    }
    let Some((writer, rate, ch)) = w else { return Err(unreadable(path, "no audio in the file")) };
    writer.finalize().map_err(write_err)?;
    Ok((rate, ch, samples / u64::from(ch)))
}

fn audio_dir(e: &Engine) -> Result<PathBuf, DawError> {
    e.folder
        .as_ref()
        .map(|f| f.join(AUDIO_DIR))
        .ok_or_else(|| DawError::new(Code::ProjectNotSaved, "the project has no folder yet"))
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

fn audio_import(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let src = PathBuf::from(a.need_text("file")?);
    let dir = audio_dir(e)?;
    if !src.is_file() {
        return Err(unreadable(&src, "no such file"));
    }
    let ext = src.extension().and_then(|x| x.to_str()).unwrap_or("").to_ascii_lowercase();
    let stem = src.file_stem().and_then(|n| n.to_str()).unwrap_or("audio").to_string();
    std::fs::create_dir_all(&dir).map_err(|err| unreadable(&dir, err))?;
    let mismatch = |rate: u32| {
        DawError::new(Code::SampleRateMismatch, format!("{} is {rate} Hz; the project is {} Hz", src.display(), e.project.sample_rate))
    };
    // WAV is copied byte for byte. Anything else (MP3, FLAC) is decoded once, here, into a
    // 32-bit float WAV, so the mix only ever reads WAV and nothing is decoded twice. Either way
    // the new file is written under a temporary name first, so a crash never leaves a half file
    // under the real one.
    let tmp = dir.join(format!("{stem}.import.part"));
    let (wanted, i) = if ext == "wav" || ext == "wave" {
        let i = info(&src)?;
        if i.rate != e.project.sample_rate {
            return Err(mismatch(i.rate));
        }
        (format!("{stem}.{ext}"), i)
    } else {
        let decoded = decode_to_wav(&src, &tmp);
        let (rate, channels, frames) = decoded.inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })?;
        if rate != e.project.sample_rate {
            let _ = std::fs::remove_file(&tmp);
            return Err(mismatch(rate));
        }
        (format!("{stem}.wav"), Info { frames, rate, channels, kind: format!("{} decoded to 32-bit float", ext.to_uppercase()) })
    };
    let content = if tmp.exists() { tmp.clone() } else { src.clone() };
    let (name, existed) = free_name(&dir, &wanted, &content);
    if existed {
        let _ = std::fs::remove_file(&tmp);
    } else {
        if !tmp.exists() {
            std::fs::copy(&src, &tmp).map_err(|err| unreadable(&tmp, err))?;
        }
        std::fs::rename(&tmp, dir.join(&name)).map_err(|err| unreadable(&dir.join(&name), err))?;
    }
    // Waveform peaks now, while the file is fresh in the disk cache, so drawing it never waits.
    let placed = dir.join(&name);
    if i.channels <= 2
        && let Ok(p) = cached_peaks(&placed)
    {
        e.peaks.insert(placed, Arc::new(p));
    }
    let secs = i.frames as f64 / f64::from(i.rate);
    let what = format!("{}, {} Hz, {}, {secs:.2} s", i.kind, i.rate, if i.channels == 1 { "mono" } else { "stereo" });
    let text = if existed { format!("Already in the project as '{name}' ({what})") } else { format!("Imported '{name}' ({what})") };
    Ok(Reply::new(text, json!({ "source": name, "frames": i.frames, "rate": i.rate, "channels": i.channels, "format": i.kind, "seconds": secs })))
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
    let i = info(&audio_dir(e)?.join(&source))?;
    if i.rate != e.project.sample_rate {
        return Err(DawError::new(Code::SampleRateMismatch, format!("{source} is {} Hz; the project is {} Hz", i.rate, e.project.sample_rate)));
    }
    if i.channels > 2 {
        return Err(DawError::new(Code::ClipEditRefused, format!("{source} has {} channels: it is an impulse response for IR Reverb (fx.file), not a clip", i.channels)));
    }
    let start = super::clips::ticks(a.need_number("at")?)?;
    let source_length = (i.frames as f64 / e.project.frames_per_tick()).ceil().max(1.0) as i64;
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
        format!("Placed clip #{id} '{}' at beat {} ({:.2} s)", c.name, super::clips::beats(start), i.frames as f64 / f64::from(i.rate)),
        json!({ "id": id, "created": [id], "track": track, "start": super::clips::beats(start), "length": super::clips::beats(source_length) }),
    ))
}

fn audio_peaks(e: &mut Engine, a: &Args) -> Result<Reply, DawError> {
    let source = a.need_text("source")?.to_string();
    let path = audio_dir(e)?.join(&source);
    let peaks = match e.peaks.get(&path) {
        Some(p) => p.clone(),
        None => {
            let p = Arc::new(cached_peaks(&path)?);
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
    let frames = u64::from(Reader::open(path)?.r.duration());
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
    let mut r = Reader::open(path)?;
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
    path: PathBuf,
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
    pub(crate) fn path(&self) -> &Path {
        &self.path
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
}

/// Everything needed to mix a project, built once (it allocates and reads impulse responses),
/// then run block by block by `render` without allocating.
pub(crate) struct Mixer {
    pub voices: Vec<Voice>,
    strips: Vec<Strip>,
    block: usize,
}

/// Build the mixer. Muted tracks are left out; if any track is soloed, only soloed tracks play.
/// MIDI clips are silent until instruments exist (M3); placeholder audio clips have no file. Each
/// file's header is checked here, so a missing or changed file stops an export before it starts.
///
/// `held` is for playback: the effects the running mixer holds, by id, with the settings and
/// tempo they were built from. An effect found there unchanged is not built again; a stand-in
/// takes its place until `Mixer::inherit` swaps the running one in, sound and all.
pub(crate) fn mixer(p: &Project, folder: Option<&Path>, block: usize, held: Option<&Held>) -> Result<Mixer, DawError> {
    let any_solo = p.tracks.iter().any(|t| t.solo);
    let fpt = p.frames_per_tick();
    let rate = f64::from(p.sample_rate);
    let mut checked = std::collections::HashMap::<PathBuf, u64>::new();
    let (mut voices, mut strips) = (Vec::new(), Vec::new());
    for t in p.tracks.iter().filter(|t| !t.mute && (!any_solo || t.solo)) {
        let first = voices.len();
        for c in p.clips.iter().filter(|c| c.track == t.id && c.has_audio_file()) {
            let Content::Audio { source, frames, .. } = &c.content else { continue };
            let folder = folder.ok_or_else(|| DawError::new(Code::ProjectNotSaved, "the project has audio but no folder"))?;
            let path = folder.join(AUDIO_DIR).join(source);
            let file_frames = match checked.get(&path) {
                Some(n) => *n,
                None => {
                    let i = info(&path)?;
                    if i.rate != p.sample_rate {
                        return Err(DawError::new(Code::SampleRateMismatch, format!("{} is {} Hz; the project is {} Hz", path.display(), i.rate, p.sample_rate)));
                    }
                    checked.insert(path.clone(), i.frames);
                    i.frames
                }
            }
            .min(*frames);
            let from = (c.offset as f64 * fpt).round() as u64;
            let len = ((c.length as f64 * fpt).round() as u64).min(file_frames.saturating_sub(from));
            voices.push(Voice { path, at: (c.start as f64 * fpt).round() as u64, from, len, gain: gain(c.gain_db) });
        }
        if voices.len() == first {
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
        let g = gain(t.gain_db) * gain(p.master_gain_db);
        strips.push(Strip { track: t.id, voices: first..voices.len(), effects, ids, peak: (0.0, 0.0), out: (g * l, g * r), buf: (vec![0.0; block], vec![0.0; block]), tail });
    }
    Ok(Mixer { voices, strips, block })
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
                    *stand_in = false;
                } else {
                    s.effects[k].adopt(&mut *o.effects[j]);
                }
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
        self.strips.iter().map(|s| self.voices[s.voices.clone()].iter().map(|v| v.at + v.len).max().unwrap_or(0) + s.tail).max().unwrap_or(0)
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
    for s in &mut m.strips {
        let (tl, tr) = (&mut s.buf.0[..n], &mut s.buf.1[..n]);
        tl.fill(0.0);
        tr.fill(0.0);
        for i in s.voices.clone() {
            let v = &m.voices[i];
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
    let mut m = mixer(p, e.folder.as_deref(), BLOCK, None)?;
    let start = (super::clips::ticks(a.number("start").unwrap_or(0.0))?.max(0) as f64 * fpt).round() as u64;
    let end = match a.number("end") {
        Some(b) => (super::clips::ticks(b)? as f64 * fpt).round() as u64,
        None => m.end_frame(),
    };
    if end <= start {
        return Err(DawError::new(
            Code::ExportFailed,
            if a.number("end").is_none() { "nothing to export: no audible audio clips (MIDI is silent until instruments exist)".to_string() } else { "the end is not after the start".to_string() },
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
    let spans: Vec<(u64, PathBuf)> = m.voices.iter().map(|v| (v.at + v.len, v.path.clone())).collect();
    let mut noise = Xorshift(0x9E37_79B9_7F4A_7C15);
    let (mut peak, mut overs) = (0.0f32, 0u64);
    let mut pos = start;
    let t0 = std::time::Instant::now();
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
    if let Err(err) = result {
        drop(w);
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    w.finalize().map_err(|err| fail(err.to_string()))?;
    std::fs::rename(&tmp, &file).map_err(|err| fail(err.to_string()))?;
    let frames = end - start;
    let secs = frames as f64 / f64::from(p.sample_rate);
    let tail_s = if a.number("end").is_none() { m.end_frame().saturating_sub(m.clips_end()) as f64 / f64::from(p.sample_rate) } else { 0.0 };
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

    #[test]
    fn refusals_are_coded() {
        let mut e = Engine::new();
        let inp = temp("refuse");
        let f441 = inp.join("cd.wav");
        wav(&f441, 44_100, 2, "16", &[0.0; 200]);
        run(&mut e, "track.add Gtr").unwrap();
        assert_eq!(run(&mut e, &format!("audio.import \"{}\"", f441.display())).unwrap_err().code, Code::ProjectNotSaved);

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
        let mut m = mixer(e.project(), Some(&dir), 512, None).unwrap();
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
}
