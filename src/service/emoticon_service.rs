use regex::Regex;
use serde::Deserialize;
use serenity::all::{EmojiId, ReactionType};
use std::collections::HashSet;
use std::path::Path;

/// Default location, relative to the working directory, of the emoticon
/// rules file. Override with the `EMOTICONS_CONFIG` environment variable.
/// The repo ships an `emoticons.json.example` next to it - copy that to
/// `emoticons.json` and edit. The real file is gitignored.
pub const DEFAULT_CONFIG_PATH: &str = "emoticons.json";

#[derive(Debug, thiserror::Error)]
pub enum EmoticonConfigError {
    #[error("Failed to read {path}: {source}")]
    Read {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("Invalid regex '{pattern}': {source}")]
    InvalidRegex {
        pattern: String,
        #[source]
        source: regex::Error,
    },

    #[error("Invalid emoji '{raw}': {reason}")]
    InvalidEmoji { raw: String, reason: String },
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    rules: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    pattern: String,
    /// Either a unicode emoji (`"👌"`) or a Discord custom-emoji mention
    /// (`"<:name:id>"` or `"<a:name:id>"` for animated). Custom emojis can
    /// come from any guild the bot shares — Discord lets the bot use any
    /// emoji it has access to as a reaction.
    emoji: String,
}

struct EmoticonRule {
    pattern: Regex,
    reaction: ReactionType,
}

pub struct EmoticonService {
    rules: Vec<EmoticonRule>,
}

impl EmoticonService {
    /// Build a service with no rules. Useful when the config file is
    /// absent — the feature simply becomes a no-op.
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Load rules from the configured JSON file. The path comes from the
    /// `EMOTICONS_CONFIG` environment variable or [`DEFAULT_CONFIG_PATH`].
    /// A missing file is not an error — it yields an empty service so the
    /// feature is opt-in.
    pub fn load_default() -> Result<Self, EmoticonConfigError> {
        let path = std::env::var("EMOTICONS_CONFIG").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        Self::load_from_path(&path)
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self, EmoticonConfigError> {
        let path = path.as_ref();

        if !path.exists() {
            tracing::warn!(
                "Emoticon config '{}' not found; emoticon reactions disabled",
                path.display()
            );
            return Ok(Self::empty());
        }

        let raw = std::fs::read_to_string(path).map_err(|source| EmoticonConfigError::Read {
            path: path.display().to_string(),
            source,
        })?;

        let config: RawConfig = serde_json::from_str(&raw).map_err(|source| EmoticonConfigError::Parse {
            path: path.display().to_string(),
            source,
        })?;

        let mut rules = Vec::with_capacity(config.rules.len());
        for raw_rule in config.rules {
            let pattern = Regex::new(&raw_rule.pattern).map_err(|source| EmoticonConfigError::InvalidRegex {
                pattern: raw_rule.pattern.clone(),
                source,
            })?;
            let reaction = parse_reaction(&raw_rule.emoji)?;
            rules.push(EmoticonRule { pattern, reaction });
        }

        tracing::info!(
            "Loaded {} emoticon rule(s) from {}",
            rules.len(),
            path.display()
        );
        Ok(Self { rules })
    }

    /// Return the reactions whose patterns match `content`. Each reaction
    /// is returned at most once, in rule order.
    pub fn detect_reactions(
        &self,
        content: &str,
    ) -> Vec<ReactionType> {
        let mut seen: HashSet<ReactionKey> = HashSet::new();
        let mut out: Vec<ReactionType> = Vec::new();

        for rule in &self.rules {
            if rule.pattern.is_match(content) {
                let key = ReactionKey::from(&rule.reaction);
                if seen.insert(key) {
                    out.push(rule.reaction.clone());
                }
            }
        }

        out
    }
}

#[derive(Hash, Eq, PartialEq)]
enum ReactionKey {
    Unicode(String),
    Custom(u64),
}

impl From<&ReactionType> for ReactionKey {
    fn from(r: &ReactionType) -> Self {
        match r {
            ReactionType::Unicode(s) => ReactionKey::Unicode(s.clone()),
            ReactionType::Custom { id, .. } => ReactionKey::Custom(id.get()),
            _ => ReactionKey::Unicode(String::new()),
        }
    }
}

/// Parse an emoji spec into a `ReactionType`.
///
/// Accepted forms:
/// - Unicode emoji literal: `"👌"`, `"❤️"`
/// - Discord custom-emoji mention: `"<:name:123456789>"`
/// - Animated custom-emoji mention: `"<a:name:123456789>"`
fn parse_reaction(raw: &str) -> Result<ReactionType, EmoticonConfigError> {
    let trimmed = raw.trim();

    if let Some(inner) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        let (animated, body) = match inner.strip_prefix("a:") {
            Some(rest) => (true, rest),
            None => (false, inner.strip_prefix(':').unwrap_or(inner)),
        };

        let (name, id_part) = body
            .rsplit_once(':')
            .ok_or_else(|| EmoticonConfigError::InvalidEmoji {
                raw: raw.to_string(),
                reason: "expected '<:name:id>' or '<a:name:id>'".to_string(),
            })?;

        let id = id_part
            .parse::<u64>()
            .map_err(|_| EmoticonConfigError::InvalidEmoji {
                raw: raw.to_string(),
                reason: format!("'{id_part}' is not a valid emoji id"),
            })?;

        return Ok(ReactionType::Custom {
            animated,
            id: EmojiId::new(id),
            name: if name.is_empty() { None } else { Some(name.to_string()) },
        });
    }

    if trimmed.is_empty() {
        return Err(EmoticonConfigError::InvalidEmoji {
            raw: raw.to_string(),
            reason: "empty emoji".to_string(),
        });
    }

    Ok(ReactionType::Unicode(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc_from_json(json: &str) -> EmoticonService {
        let config: RawConfig = serde_json::from_str(json).unwrap();
        let rules = config
            .rules
            .into_iter()
            .map(|r| EmoticonRule {
                pattern: Regex::new(&r.pattern).unwrap(),
                reaction: parse_reaction(&r.emoji).unwrap(),
            })
            .collect();
        EmoticonService { rules }
    }

    #[test]
    fn parses_unicode() {
        let r = parse_reaction("👌").unwrap();
        assert!(matches!(r, ReactionType::Unicode(ref s) if s == "👌"));
    }

    #[test]
    fn parses_custom_emoji() {
        let r = parse_reaction("<:ferris:123456789>").unwrap();
        match r {
            ReactionType::Custom { animated, id, name } => {
                assert!(!animated);
                assert_eq!(id.get(), 123456789);
                assert_eq!(name.as_deref(), Some("ferris"));
            }
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn parses_animated_emoji() {
        let r = parse_reaction("<a:dance:987654321>").unwrap();
        match r {
            ReactionType::Custom { animated, id, .. } => {
                assert!(animated);
                assert_eq!(id.get(), 987654321);
            }
            _ => panic!("expected Custom"),
        }
    }

    #[test]
    fn rejects_invalid_emoji() {
        assert!(parse_reaction("<:bad>").is_err());
        assert!(parse_reaction("<:name:notanid>").is_err());
        assert!(parse_reaction("").is_err());
    }

    #[test]
    fn detects_with_word_boundary() {
        let svc = svc_from_json(
            r#"{ "rules": [
                { "pattern": "(?i)\\bok\\b", "emoji": "👌" }
            ]}"#,
        );
        assert_eq!(svc.detect_reactions("ok").len(), 1);
        assert_eq!(svc.detect_reactions("look").len(), 0);
    }

    #[test]
    fn deduplicates_same_emoji() {
        let svc = svc_from_json(
            r#"{ "rules": [
                { "pattern": "(?i)\\bthanks\\b", "emoji": "🙏" },
                { "pattern": "(?i)\\bthank you\\b", "emoji": "🙏" }
            ]}"#,
        );
        assert_eq!(svc.detect_reactions("thanks, thank you").len(), 1);
    }

    #[test]
    fn missing_file_yields_empty_service() {
        let svc = EmoticonService::load_from_path("/does/not/exist.json").unwrap();
        assert!(svc.detect_reactions("anything").is_empty());
    }
}
