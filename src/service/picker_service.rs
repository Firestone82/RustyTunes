use crate::bot::{Context, MusicBotError};
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::service::embed_service::SendEmbed;
use crate::service::interaction_service::DeferredInteractionStream;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponseFollowup};
use std::time::{Duration, Instant};

const PICKER_TIMEOUT: Duration = Duration::from_secs(60 * 2);

/// Outcome of a picker interaction.
pub enum PickerOutcome {
    Selected(usize),
    Cancelled,
    Expired,
}

pub async fn show_picker(
    ctx: Context<'_>,
    count: usize,
    id_prefix: &str,
    embed: CreateEmbed,
    not_author_message: &str,
) -> Result<PickerOutcome, MusicBotError> {
    let rows: Vec<CreateActionRow> = (0..count)
        .map(|i| {
            CreateButton::new(format!("{id_prefix}_{i}"))
                .label((i + 1).to_string())
                .style(ButtonStyle::Secondary)
        })
        .chain(std::iter::once(
            CreateButton::new(format!("{id_prefix}_cancel"))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ))
        .collect::<Vec<_>>()
        .chunks(5)
        .map(|chunk| CreateActionRow::Buttons(chunk.to_vec()))
        .collect();

    let message = ctx
        .send(
            poise::CreateReply::default()
                .embed(embed)
                .components(rows)
                .reply(true),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?
        .into_message()
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let cancel_id = format!("{id_prefix}_cancel");
    let select_prefix = format!("{id_prefix}_");

    // Each button click is deferred the moment it arrives, so a slow handler
    // here can't push the next user's click past Discord's 3-second window.
    let mut stream = DeferredInteractionStream::new(ctx.serenity_context(), message.id);

    let deadline = Instant::now() + PICKER_TIMEOUT;
    let interaction = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(PickerOutcome::Expired);
        }

        let Some(interaction) = stream.next_within(remaining).await else {
            message.delete(ctx.http()).await?;
            PlayerEmbed::SearchExpired
                .to_embed()
                .send_context(ctx, true, Some(15))
                .await?;
            return Ok(PickerOutcome::Expired);
        };

        if interaction.user.id != ctx.author().id {
            let _ = interaction
                .create_followup(
                    ctx.http(),
                    CreateInteractionResponseFollowup::new()
                        .content(not_author_message)
                        .ephemeral(true),
                )
                .await;
            continue;
        }

        break interaction;
    };

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

    Ok(PickerOutcome::Selected(index))
}
