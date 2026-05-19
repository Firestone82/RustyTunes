use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::LeaderboardEntry;
use crate::embeds::reputation::rep_embed::ReputationEmbed;
use crate::service::interaction_service::DeferredInteractionStream;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateInteractionResponseFollowup, EditMessage};
use std::time::Duration;

const ITEMS_PER_PAGE: usize = 10;
const SUMMARY_TOP: usize = 5;
const SUMMARY_BOTTOM: usize = 5;

/// Show the reputation leaderboard, ranked by total rep.
#[poise::command(prefix_command, slash_command, aliases("leaderboard", "reptop", "toprep"))]
pub async fn rep_leaderboard(ctx: Context<'_>) -> Result<(), MusicBotError> {
    let entries = sqlx::query_as!(
        LeaderboardEntry,
        r#"
        SELECT
            receiver_id AS "receiver_id!: String",
            COALESCE(SUM(rep_value), 0) AS "total_rep!: i64",
            COUNT(*) AS "log_count!: i64"
        FROM reputation_logs
        GROUP BY receiver_id
        ORDER BY SUM(rep_value) DESC, COUNT(*) DESC, receiver_id ASC
        "#
    )
    .fetch_all(&*ctx.data().database_pool)
    .await
    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let detail_pages = entries.len().div_ceil(ITEMS_PER_PAGE);
    let total_pages = (1 + detail_pages).max(1);
    let mut current_page = 0;

    let mut message = ctx
        .send(
            poise::CreateReply::default()
                .embed(render_page(&entries, current_page))
                .components(get_nav_components(current_page, total_pages))
                .reply(true),
        )
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?
        .into_message()
        .await
        .map_err(|error| MusicBotError::InternalError(error.to_string()))?;

    let mut stream = DeferredInteractionStream::new(ctx.serenity_context(), message.id);

    while let Some(interaction) = stream.next_within(Duration::from_mins(2)).await {
        if interaction.user.id != ctx.author().id {
            interaction
                .create_followup(
                    ctx.http(),
                    CreateInteractionResponseFollowup::new()
                        .content("Only the person who ran this command can navigate the leaderboard.")
                        .ephemeral(true),
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

        message
            .edit(
                ctx.serenity_context(),
                EditMessage::new()
                    .embed(render_page(&entries, current_page))
                    .components(get_nav_components(current_page, total_pages)),
            )
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
    }

    message
        .edit(
            ctx.serenity_context(),
            EditMessage::new().components(Vec::new()),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(())
}

fn render_page(
    entries: &[LeaderboardEntry],
    page: usize,
) -> CreateEmbed {
    if page == 0 {
        let total = entries.len();
        let top_len = SUMMARY_TOP.min(total);
        let top = &entries[..top_len];

        let remaining = total - top_len;
        let bottom_len = SUMMARY_BOTTOM.min(remaining);
        let bottom_start = total - bottom_len;
        let bottom = &entries[bottom_start..];
        let middle_count = remaining - bottom_len;

        ReputationEmbed::LeaderboardSummary {
            top,
            middle_count,
            bottom,
            bottom_start_rank: bottom_start,
            total_entries: total,
        }
        .to_embed()
    } else {
        let detail_page = page - 1;
        let start = detail_page * ITEMS_PER_PAGE;
        let end = (start + ITEMS_PER_PAGE).min(entries.len());
        let slice = if start < entries.len() { &entries[start..end] } else { &[][..] };

        ReputationEmbed::LeaderboardPage {
            entries: slice,
            start_rank: start,
            total_entries: entries.len(),
        }
        .to_embed()
    }
}

fn get_nav_components(
    page: usize,
    total_pages: usize,
) -> Vec<CreateActionRow> {
    let prev_btn = CreateButton::new("page_prev")
        .label("⬅️ Previous")
        .style(ButtonStyle::Primary)
        .disabled(page == 0);

    let indicator = CreateButton::new("page_indicator")
        .label(format!("{}/{}", page + 1, total_pages))
        .style(ButtonStyle::Secondary)
        .disabled(true);

    let next_btn = CreateButton::new("page_next")
        .label("Next ➡️")
        .style(ButtonStyle::Primary)
        .disabled(page + 1 >= total_pages);

    vec![CreateActionRow::Buttons(vec![prev_btn, indicator, next_btn])]
}
