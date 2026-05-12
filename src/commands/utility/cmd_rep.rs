use crate::bot::{Context, MusicBotError};
use serenity::all::{Mentionable, User};
use sqlx::types::time::OffsetDateTime;

/// Manage reputation: add, remove.
#[poise::command(
    prefix_command,
    slash_command,
    subcommands("add", "remove", "list"),
    subcommand_required,
)]
pub async fn rep(_ctx: Context<'_>) -> Result<(), MusicBotError> {
    Ok(())
}

/// Give positive reputation to a user with a reason.
#[poise::command(prefix_command, slash_command, aliases("+rep","plusrep"))]
pub async fn add(
    ctx: Context<'_>,
    #[description = "User to give reputation to"] user: User,
    #[rest]
    #[description = "Reason for giving reputation"]
    reason: String,
) -> Result<(), MusicBotError> {
    if user.id == ctx.author().id {
        ctx.say("You cannot give reputation to yourself.").await?;
        return Ok(());
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();
    let rep_value = 1;
    let created_at = OffsetDateTime::now_utc();

    sqlx::query!(
        "
INSERT INTO reputation_logs (giver_id, receiver_id, rep_value, reason, created_at)
VALUES (?, ?, ?, ?, ?)
",
        giver_id,
        receiver_id,
        rep_value,
        reason,
        created_at
    )
    .execute(&*ctx.data().database_pool)
    .await
    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    ctx.say(format!("Gave +1 reputation to {} for: {}", user.mention(), reason)).await?;

    Ok(())
}

/// Give negative reputation to a user with a reason.
#[poise::command(prefix_command, slash_command,aliases("-rep","minusrep"))]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "User to remove reputation from"] user: User,
    #[rest]
    #[description = "Reason for removing reputation"]
    reason: String,
) -> Result<(), MusicBotError> {
    if user.id == ctx.author().id {
        ctx.say("You cannot remove reputation from yourself.").await?;
        return Ok(());
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();
    let rep_value = -1;
    let created_at = OffsetDateTime::now_utc();

    sqlx::query!(
        "
INSERT INTO reputation_logs (giver_id, receiver_id, rep_value, reason, created_at)
VALUES (?, ?, ?, ?, ?)
",
        giver_id,
        receiver_id,
        rep_value,
        reason,
        created_at
    )
    .execute(&*ctx.data().database_pool)
    .await
    .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    ctx.say(format!("Removed -1 reputation from {} for: {}", user.mention(), reason)).await?;

    Ok(())
}

/// Zobrazí celkovou reputaci a posledních 5 záznamů uživatele.
#[poise::command(prefix_command, slash_command, aliases("repstats", "info"))]
pub async fn list(
    ctx: Context<'_>,
    #[description = "User to check reputation for (optional, defaults to yourself)"]
    user: Option<User>,
) -> Result<(), MusicBotError> {
    // Pokud není zadán uživatel, použijeme toho, kdo příkaz zavolal
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let target_id = target_user.id.to_string();

    // 1. Zjištění celkové reputace (SUM všech rep_value)
    let total_rep: i64 = sqlx::query_scalar!(
        "SELECT COALESCE(SUM(rep_value), 0) FROM reputation_logs WHERE receiver_id == ?",
        target_id
    )
        .fetch_one(&*ctx.data().database_pool)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;


    let logs = sqlx::query_as!(
        Rep,
        "SELECT id, giver_id, receiver_id, rep_value, reason, created_at
         FROM reputation_logs
         WHERE receiver_id == ?
         ORDER BY created_at DESC",
        target_id
    )
        .fetch_all(&*ctx.data().database_pool)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    // TODO embed
    let mut response = format!("## Reputation of user {}\n", target_user.name);
    response.push_str(&format!("**Overall Score:** {}\n\n", total_rep));

    if logs.is_empty() {
        response.push_str("So far no rep.");
    } else {
        response.push_str("**Overall history:**\n");
        for log in logs {
            let emoji = if log.rep_value > 0 { "➕" } else { "➖" };
        response.push_str(&format!("{} from <@{}>: *{}* ({})\n", emoji, log.giver_id, log.reason, log.created_at.date()));
        }
    }

    ctx.say(response).await?;

    Ok(())
}


pub struct Rep {
    pub id: i64,
    pub giver_id: String,
    pub receiver_id: String,
    pub rep_value: i64,
    pub reason: String,
    pub created_at: OffsetDateTime,
}
