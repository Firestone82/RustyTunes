//! Shared helpers for invoking `yt-dlp`. Centralises the JS-runtime flag and
//! the stderr-to-user-message extraction so every call site behaves the same.

use dotenv::var;

/// Extra args to pass to every `yt-dlp` invocation, derived from the
/// `YT_DLP_JS_RUNTIMES` env var. Empty when unset.
///
/// Modern YouTube extraction needs a JS runtime to deobfuscate player
/// signatures; yt-dlp only enables `deno` by default. If deno isn't installed
/// on the host, set this to e.g. `node:/usr/bin/node` so yt-dlp can still
/// extract full format lists instead of warning and degrading.
pub fn js_runtime_args() -> Vec<String> {
    match var("YT_DLP_JS_RUNTIMES") {
        Ok(value) if !value.trim().is_empty() => vec!["--js-runtimes".to_string(), value],
        _ => Vec::new(),
    }
}

/// Pull the most informative line out of a yt-dlp / songbird stderr blob for
/// surfacing to users. yt-dlp's noisy multiline output (WARNINGs, banner,
/// progress) is reduced to the trailing `ERROR:` line when present, falling
/// back to the last non-empty line, then to a generic message.
pub fn summarize_ytdlp_error(raw: &str) -> String {
    let cleaned: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if let Some(line) = cleaned.iter().rev().find(|l| l.starts_with("ERROR:")) {
        let after_prefix = line.trim_start_matches("ERROR:").trim();
        if let Some((_, rest)) = after_prefix.split_once(':') {
            let rest = rest.trim();
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
        return after_prefix.to_string();
    }

    cleaned
        .last()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "yt-dlp reported an unknown error".to_string())
}
