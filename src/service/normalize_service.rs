//! Per-track loudness normalization based on EBU R128 (LUFS) measurement.
//!
//! Concept: run `ffmpeg`'s `loudnorm` filter in measurement mode once per
//! cached file to learn its integrated loudness, then derive a static gain
//! offset that brings it close to `TARGET_LUFS`. The gain is applied as a
//! multiplier on top of the user-set volume at playback time. This is a
//! ReplayGain-style approach: the dynamic range within a song is preserved
//! (quiet passages stay quiet), but the overall perceived loudness across
//! songs is evened out.
//!
//! Results are persisted as a `<stem>.gain` sidecar file next to the audio,
//! so the (slow) ffmpeg measurement only runs once per cached track. An
//! in-process cache layered on top avoids reparsing the sidecar on every play.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use tokio::process::Command;

/// EBU R128 target integrated loudness. -16 LUFS sits between Spotify (-14)
/// and Apple Music (-16) and leaves a bit of headroom before peak limiting.
const TARGET_LUFS: f32 = -16.0;

/// Clamp range for the derived gain (in dB). Caps amplification to avoid
/// blasting silence-y tracks, and keeps loud tracks from being pulled down
/// hard enough to feel obviously crushed.
const MAX_GAIN_DB: f32 = 6.0;
const MIN_GAIN_DB: f32 = -12.0;

/// Extension used for the per-file gain sidecar.
pub const SIDECAR_EXT: &str = "gain";

/// In-memory cache of measured gains, keyed by absolute path string. We
/// keep this even with the sidecar on disk because measuring back-to-back
/// reads of the same file would otherwise hit the filesystem each time.
static GAIN_CACHE: OnceLock<Mutex<HashMap<String, f32>>> = OnceLock::new();

fn cache_handle() -> &'static Mutex<HashMap<String, f32>> {
    GAIN_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str) -> Option<f32> {
    cache_handle().lock().ok()?.get(key).copied()
}

fn cache_set(key: String, gain_db: f32) {
    if let Ok(mut guard) = cache_handle().lock() {
        guard.insert(key, gain_db);
    }
}

/// Convert a dB gain offset to an amplitude multiplier suitable for
/// `track_handle.set_volume`. Amplitude doubles per +6 dB.
pub fn gain_to_multiplier(gain_db: f32) -> f32 {
    10f32.powf(gain_db / 20.0)
}

/// Best-effort lookup of the normalization multiplier for `path`. Returns
/// 1.0 (no change) if the file can't be analyzed or ffmpeg isn't available.
pub async fn multiplier_for(path: &Path) -> f32 {
    match gain_db_for(path).await {
        Some(db) => gain_to_multiplier(db),
        None => 1.0,
    }
}

/// Measure-or-recall the gain offset (in dB) for `path`. Tries the memory
/// cache, then the on-disk sidecar, then falls back to running ffmpeg. A
/// successful measurement is written through to both layers.
pub async fn gain_db_for(path: &Path) -> Option<f32> {
    let key = path.to_string_lossy().to_string();

    if let Some(g) = cache_get(&key) {
        return Some(g);
    }

    if let Some(g) = read_sidecar(path).await {
        cache_set(key.clone(), g);
        return Some(g);
    }

    let measured = measure_with_ffmpeg(path).await?;
    let gain_db = (TARGET_LUFS - measured).clamp(MIN_GAIN_DB, MAX_GAIN_DB);

    if let Err(e) = write_sidecar(path, gain_db).await {
        tracing::debug!("Failed to write gain sidecar for {}: {e}", path.display());
    }
    cache_set(key, gain_db);
    Some(gain_db)
}

/// Run loudness measurement in the background so playback isn't blocked.
/// Used right after a cache write so the next play has the gain ready.
pub fn spawn_precompute(path: PathBuf) {
    tokio::spawn(async move {
        match gain_db_for(&path).await {
            Some(g) => tracing::debug!(
                "Loudness gain for {}: {:+.2} dB",
                path.display(),
                g
            ),
            None => tracing::debug!(
                "Skipped loudness analysis for {} (ffmpeg unavailable or parse failed)",
                path.display()
            ),
        }
    });
}

fn sidecar_path(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let parent = path.parent()?;
    Some(parent.join(format!("{stem}.{SIDECAR_EXT}")))
}

async fn read_sidecar(path: &Path) -> Option<f32> {
    let sidecar = sidecar_path(path)?;
    let contents = tokio::fs::read_to_string(&sidecar).await.ok()?;
    contents.trim().parse::<f32>().ok()
}

async fn write_sidecar(path: &Path, gain_db: f32) -> std::io::Result<()> {
    let sidecar = sidecar_path(path).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no sidecar path for input")
    })?;
    tokio::fs::write(sidecar, format!("{gain_db:.2}")).await
}

/// Invoke `ffmpeg -af loudnorm=print_format=json` on `path` and pull the
/// `input_i` (integrated loudness, in LUFS) field out of its stderr JSON.
/// Returns `None` if ffmpeg isn't installed or its output can't be parsed.
async fn measure_with_ffmpeg(path: &Path) -> Option<f32> {
    let output = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostats",
            "-nostdin",
            "-i",
        ])
        .arg(path)
        .args([
            "-af",
            "loudnorm=I=-16:print_format=json",
            "-f",
            "null",
            "-",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| {
            tracing::debug!("ffmpeg launch failed for {}: {e}", path.display());
            e
        })
        .ok()?;

    if !output.status.success() {
        tracing::debug!(
            "ffmpeg loudnorm failed for {} (status {})",
            path.display(),
            output.status
        );
        return None;
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_input_i(&stderr)
}

/// Find `"input_i" : "<float>"` in ffmpeg's JSON report. Hand-rolled instead
/// of full JSON parsing because the block is embedded in a stream of other
/// log lines and the surrounding noise breaks strict parsers.
fn parse_input_i(stderr: &str) -> Option<f32> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("\"input_i\"") else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix(':') else {
            continue;
        };
        let value = rest.trim().trim_end_matches(',').trim().trim_matches('"');
        return value.parse::<f32>().ok();
    }
    None
}
