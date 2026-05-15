use crate::bot::{Context, MusicBotError};
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::service::embed_service::SendEmbed;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage, Message, UserId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PICKER_TIMEOUT: Duration = Duration::from_secs(60 * 2);
const OTHER_USER_COOLDOWN: Duration = Duration::from_secs(5);

/// Outcome of a picker interaction.
pub enum PickerOutcome {
    Selected(usize),
    Cancelled,
    Expired,
}

/// Render numbered buttons (1..=`count`) plus a Cancel button under `embed`
/// and wait for the original author's selection. Returns the picked index,
/// or a cancellation/timeout sentinel.
///
/// `id_prefix` namespaces the button custom IDs so multiple pickers can run
/// concurrently on the same message without clashing.
/// `not_author_message` is shown to anyone else who clicks a button.
pub async fn show_picker(ctx: Context<'_>, count: usize, id_prefix: &str, embed: CreateEmbed, not_author_message: &str) -> Result<PickerOutcome, MusicBotError> {
    let mut buttons: Vec<CreateButton> = (0..count)
        .map(|i| CreateButton::new(format!("{id_prefix}_{i}")).label((i + 1).to_string()).style(ButtonStyle::Secondary))
        .collect();
    buttons.push(CreateButton::new(format!("{id_prefix}_cancel")).label("✖ Cancel").style(ButtonStyle::Danger));

    let row_count = buttons.len().div_ceil(5);
    let per_row = buttons.len().div_ceil(row_count.max(1));
    let rows: Vec<CreateActionRow> = buttons.chunks(per_row.max(1)).map(|chunk| CreateActionRow::Buttons(chunk.to_vec())).collect();

    let reply_handle = ctx
        .send(poise::CreateReply::default().embed(embed).components(rows).reply(true))
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let message: Message = reply_handle.into_message().await.map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let deadline = Instant::now() + PICKER_TIMEOUT;
    let mut cooldowns: HashMap<UserId, Instant> = HashMap::new();
    let cancel_id = format!("{id_prefix}_cancel");
    let select_prefix = format!("{id_prefix}_");

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired.to_embed().send_context(ctx, true, Some(15)).await?;
            return Ok(PickerOutcome::Expired);
        }

        let interaction = message.await_component_interaction(ctx.serenity_context().shard.clone()).timeout(remaining).await;

        let Some(interaction) = interaction else {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired.to_embed().send_context(ctx, true, Some(15)).await?;
            return Ok(PickerOutcome::Expired);
        };

        if interaction.user.id != ctx.author().id {
            let now = Instant::now();
            let on_cooldown = cooldowns.get(&interaction.user.id).map(|&last| now.duration_since(last) < OTHER_USER_COOLDOWN).unwrap_or(false);
            if on_cooldown {
                interaction.defer(ctx.http()).await.ok();
            } else {
                cooldowns.insert(interaction.user.id, now);
                interaction
                    .create_response(
                        ctx.http(),
                        CreateInteractionResponse::Message(CreateInteractionResponseMessage::new().content(not_author_message).ephemeral(true)),
                    )
                    .await
                    .ok();
            }
            continue;
        }

        interaction.defer(ctx.http()).await?;
        message.delete(ctx.http()).await?;

        if interaction.data.custom_id == cancel_id {
            return Ok(PickerOutcome::Cancelled);
        }

        let index: usize = interaction
            .data
            .custom_id
            .strip_prefix(&select_prefix)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| MusicBotError::InternalError("Bad picker id".into()))?;

        return Ok(PickerOutcome::Selected(index));
    }
}
