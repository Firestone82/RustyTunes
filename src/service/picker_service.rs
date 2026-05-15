use crate::bot::{Context, MusicBotError};
use crate::embeds::music::player_embed::PlayerEmbed;
use crate::service::embed_service::SendEmbed;
// Nutné pro .next() na streamu
use serenity::all::{ButtonStyle, ComponentInteractionCollector, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage};
use serenity::futures::StreamExt;
use std::time::Duration;

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
    // gather all rows
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

    // stream for handling user input
    let mut collector = ComponentInteractionCollector::new(ctx.serenity_context().clone())
        .message_id(message.id)
        .stream();

    // main waiting loop for some interaction
    let interaction_result = tokio::time::timeout(PICKER_TIMEOUT, async {
        while let Some(interaction) = collector.next().await {
            // check for other users
            if interaction.user.id != ctx.author().id {
                let _ = interaction
                    .create_response(
                        ctx.http(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(not_author_message)
                                .ephemeral(true),
                        ),
                    )
                    .await;
                continue;
            }
            return interaction;
        }
        unreachable!() // user has only one interaction
    })
    .await;

    let Ok(interaction) = interaction_result else {
        message.delete(ctx.http()).await?;
        PlayerEmbed::SearchExpired
            .to_embed()
            .send_context(ctx, true, Some(15))
            .await?;
        return Ok(PickerOutcome::Expired);
    };

    // acknowledge and delete
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

    Ok(PickerOutcome::Selected(index))
}
