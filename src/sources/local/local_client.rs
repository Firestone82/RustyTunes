use std::path::{Path, PathBuf};

const DOWNLOADS_DIR: &str = "downloads";
const ALLOWED_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "ogg", "m4a", "opus"];

pub fn downloads_dir() -> PathBuf {
    PathBuf::from(DOWNLOADS_DIR)
}

pub async fn ensure_downloads_dir() -> std::io::Result<PathBuf> {
    let dir = downloads_dir();
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Strip path separators and other risky characters so a remote-supplied
/// filename can't escape the downloads directory.
pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "download.mp3".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Pick a filename from the URL's path, falling back to a default.
pub fn filename_from_url(url: &str) -> String {
    let stripped = url.split('?').next().unwrap_or(url);
    let candidate = stripped.rsplit('/').next().unwrap_or("");
    let candidate = sanitize_filename(candidate);

    if has_audio_extension(&candidate) {
        candidate
    } else {
        format!("{candidate}.mp3").trim_start_matches('.').to_string()
    }
}

pub fn has_audio_extension(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    ALLOWED_EXTENSIONS.iter().any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Avoid clobbering an existing file by appending " (n)" before the extension.
pub async fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !path_exists(&candidate).await {
        return candidate;
    }

    let (stem, ext) = match filename.rfind('.') {
        Some(idx) if idx > 0 => (&filename[..idx], &filename[idx..]),
        _ => (filename, ""),
    };

    for n in 1..1000 {
        let next = dir.join(format!("{stem} ({n}){ext}"));
        if !path_exists(&next).await {
            return next;
        }
    }
    candidate
}

async fn path_exists(p: &Path) -> bool {
    tokio::fs::metadata(p).await.is_ok()
}

pub async fn list_local_files() -> std::io::Result<Vec<PathBuf>> {
    let dir = downloads_dir();
    let mut files: Vec<PathBuf> = Vec::new();

    let mut read_dir = match tokio::fs::read_dir(&dir).await {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(e),
    };

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        let is_file = entry.file_type().await.map(|t| t.is_file()).unwrap_or(false);
        if !is_file {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if has_audio_extension(name) {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

pub fn track_title(path: &Path) -> String {
    path.file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()).unwrap_or_else(|| "Unknown".to_string())
}

/// Substring match (case-insensitive) against the file stem. Returns matches
/// in alphabetical order. Used by `local play` / `local remove` so users can
/// pick a track by name without typing the full filename.
pub async fn search_local(query: &str) -> std::io::Result<Vec<PathBuf>> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return list_local_files().await;
    }

    let all = list_local_files().await?;
    let mut matches: Vec<PathBuf> = all.into_iter().filter(|p| track_title(p).to_ascii_lowercase().contains(&needle)).collect();

    matches.sort();
    Ok(matches)
}

pub async fn delete_local(path: &Path) -> std::io::Result<()> {
    tokio::fs::remove_file(path).await
}
