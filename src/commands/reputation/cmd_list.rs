use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::Rep;
use crate::embeds::reputation::rep_embed::ReputationEmbed;
use serenity::all::{ButtonStyle, ComponentInteractionCollector, CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage, User};
use serenity::futures::StreamExt;
use std::time::Duration;

/// List the reputation of a user, including history.
#[poise::command(prefix_command, slash_command, aliases("reps", "repinfo"))]
pub async fn list_rep(ctx: Context<'_>, #[description = "User to check reputation for (optional, defaults to yourself)"] user: Option<User>) -> Result<(), MusicBotError> {
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let target_id = target_user.id.to_string();

    let total_rep: i64 = sqlx::query_scalar!(
        "
        SELECT COALESCE(SUM(rep_value), 0)
        FROM reputation_logs
        WHERE receiver_id == ?
        ",
        target_id
    )
    .fetch_one(&*ctx.data().database_pool)
    .await
    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let logs = sqlx::query_as!(
        Rep,
        "
        SELECT id, giver_id, receiver_id, rep_value, reason, created_at
        FROM reputation_logs
        WHERE receiver_id == ?
        ORDER BY created_at DESC
        ",
        target_id
    )
    .fetch_all(&*ctx.data().database_pool)
    .await
    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let items_per_page = 5;
    let total_pages = logs.len().div_ceil(items_per_page).max(1);
    let mut current_page = 0;

    let get_page_slice = |page: usize| -> &[Rep] {
        let start = page * items_per_page;
        let end = (start + items_per_page).min(logs.len());
        &logs[start..end]
    };

    let mut message = ctx
        .send(
            poise::CreateReply::default()
                .embed(ReputationEmbed::List(get_page_slice(current_page), &target_id, total_rep, logs.len()).to_embed())
                .components(get_nav_components(current_page, total_pages))
                .reply(true),
        )
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?
        .into_message()
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    loop {
        match tokio::time::timeout(
            Duration::from_mins(2),
            ComponentInteractionCollector::new(ctx.serenity_context()).message_id(message.id).stream().next(),
        )
        .await
        {
            Ok(Some(interaction)) => {
                if interaction.user.id != ctx.author().id {
                    interaction
                        .create_response(
                            ctx.http(),
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Only the person who ran this command can select a track.")
                                    .ephemeral(true),
                            ),
                        )
                        .await
                        .ok();
                    continue;
                }

                match interaction.data.custom_id.as_str() {
                    "page_next" => {
                        if current_page + 1 < total_pages {
                            current_page += 1;
                        }
                    }
                    "page_prev" => {
                        current_page = current_page.saturating_sub(1);
                    }
                    _ => continue,
                }

                interaction
                    .create_response(
                        ctx.serenity_context(),
                        CreateInteractionResponse::UpdateMessage(
                            CreateInteractionResponseMessage::new()
                                .embed(ReputationEmbed::List(get_page_slice(current_page), &target_id, total_rep, logs.len()).to_embed())
                                .components(get_nav_components(current_page, total_pages)),
                        ),
                    )
                    .await
                    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    message
        .edit(ctx.serenity_context(), serenity::all::EditMessage::new().components(Vec::new()))
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(())
}

fn get_nav_components(page: usize, total_pages: usize) -> Vec<CreateActionRow> {
    let prev_btn = CreateButton::new("page_prev").label("⬅️ Previews").style(ButtonStyle::Primary).disabled(page == 0);

    let indicator = CreateButton::new("page_indicator")
        .label(format!("{}/{}", page + 1, total_pages))
        .style(ButtonStyle::Secondary)
        .disabled(true);

    let next_btn = CreateButton::new("page_next").label("Next ➡️").style(ButtonStyle::Primary).disabled(page + 1 >= total_pages);

    vec![CreateActionRow::Buttons(vec![prev_btn, indicator, next_btn])]
}
