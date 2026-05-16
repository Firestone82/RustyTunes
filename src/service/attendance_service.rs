use crate::embeds::activity::attendees_embed::{AttendeeRow, AttendeesEmbed};
use serenity::all::{ChannelId, CreateEmbed, GuildId, Mentionable, UserId};
use serenity::prelude::Context as SerenityContext;
use std::collections::HashSet;

/// Build the attendee list for an active gather/break: current voice-channel
/// members plus anyone added via `/expect`, minus anyone removed via
/// `/forget`. The bot itself is always excluded.
pub fn attendee_rows(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    extra_expected: &HashSet<UserId>,
    forgotten: &HashSet<UserId>,
) -> Vec<AttendeeRow> {
    let bot_id = serenity_ctx.cache.current_user().id;

    let voice_ids: HashSet<UserId> = serenity_ctx
        .cache
        .guild(guild_id)
        .as_ref()
        .map(|g| {
            g.voice_states
                .values()
                .filter(|vs| vs.channel_id == Some(voice_channel_id) && vs.user_id != bot_id)
                .map(|vs| vs.user_id)
                .collect()
        })
        .unwrap_or_default();

    let mut all: HashSet<UserId> = voice_ids.clone();
    for id in extra_expected {
        all.insert(*id);
    }
    for id in forgotten {
        all.remove(id);
    }

    all.into_iter()
        .map(|id| AttendeeRow {
            mention: id.mention().to_string(),
            in_voice: voice_ids.contains(&id),
        })
        .collect()
}

pub fn attendees_embed(
    serenity_ctx: &SerenityContext,
    guild_id: GuildId,
    voice_channel_id: ChannelId,
    extra_expected: &HashSet<UserId>,
    forgotten: &HashSet<UserId>,
) -> CreateEmbed {
    let rows = attendee_rows(
        serenity_ctx,
        guild_id,
        voice_channel_id,
        extra_expected,
        forgotten,
    );
    AttendeesEmbed { rows: &rows }.to_embed()
}
