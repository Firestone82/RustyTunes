//! Shared helpers for assembling yt-dlp invocations.
//!
//! Hosts where yt-dlp's pure-Python JS interpreter struggles can install a
//! native runtime (Deno, Node, …) and point yt-dlp at it via `--js-runtimes`.
//! See <https://github.com/yt-dlp/yt-dlp/wiki/EJS#deno>. When the
//! `YT_DLP_JS_RUNTIMES` env var is set we forward its value to every yt-dlp
//! call so spawn, probe, playlist enumeration, and the streaming `YoutubeDl`
//! input all stay in sync.

const JS_RUNTIMES_ENV: &str = "YT_DLP_JS_RUNTIMES";

/// Extra CLI args to append to any yt-dlp invocation. Empty when no
/// configuration applies.
pub fn extra_args() -> Vec<String> {
    let mut args = Vec::new();
    if let Ok(runtimes) = std::env::var(JS_RUNTIMES_ENV) {
        let trimmed = runtimes.trim();
        if !trimmed.is_empty() {
            args.push("--js-runtimes".to_string());
            args.push(trimmed.to_string());
        }
    }
    args
}
