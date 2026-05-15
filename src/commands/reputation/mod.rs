use crate::bot::{Context, MusicBotError};
use serenity::all::User;

use crate::embeds::rep_embed::ReputationEmbed;
use crate::service::embed_service::SendEmbed;
use time::{Duration, OffsetDateTime};

pub mod cmd_list;
pub mod cmd_minus;
pub mod cmd_plus;

#[derive(sqlx::FromRow)]
pub struct Rep {
    pub id: i64,
    pub giver_id: String,
    pub receiver_id: String,
    pub rep_value: i64,
    pub reason: String,
    pub created_at: OffsetDateTime,
}

/// Detects if giver spams
/// If yes it returns true otherwise false
async fn spam_protection(ctx: Context<'_>, receiver_id: String) -> Result<bool, MusicBotError> {
    let giver_id = ctx.author().id.to_string();
    let last_insert = sqlx::query_scalar!(
        "
SELECT created_at
FROM reputation_logs
WHERE giver_id == ? AND receiver_id == ?
ORDER BY created_at DESC
LIMIT 1
         ",
        giver_id,
        receiver_id
    )
        .fetch_optional(&*ctx.data().database_pool)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    let now = OffsetDateTime::now_utc();

    if let Some(last_insert) = last_insert {
        let elapsed = now - last_insert;
        if elapsed < Duration::minutes(10) {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn apply_rep_db(
    pool: &sqlx::Pool<sqlx::Sqlite>,
    giver_id: &str,
    receiver_id: &str,
    rep_value: i64,
    reason: &str,
) -> Result<i64, sqlx::Error> {
    sqlx::query!(
        "
INSERT INTO reputation_logs (giver_id, receiver_id, rep_value, reason)
VALUES (?, ?, ?, ?)
",
        giver_id,
        receiver_id,
        rep_value,
        reason,
    )
        .execute(pool)
        .await?;

    let overall_rep: i64 = sqlx::query_scalar!(
        "
SELECT COALESCE(SUM(rep_value), 0)
FROM reputation_logs
WHERE receiver_id == ?
",
        receiver_id,
    )
        .fetch_one(pool)
        .await?;

    Ok(overall_rep)
}

/// process reputation
async fn process_rep(
    ctx: Context<'_>,
    user: User,
    reason: String,
    rep_value: i64,
) -> Result<Option<i64>, MusicBotError> {
    // self check
    if user.id == ctx.author().id {
        ReputationEmbed::SelfError
            .to_embed()
            .send_context(ctx, true, 60u64.into())
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(None);
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();

    // spam check
    if spam_protection(ctx, receiver_id.clone()).await? {
        ReputationEmbed::SpamError
            .to_embed()
            .send_context(ctx, true, None)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(None);
    }

    let rep = apply_rep_db(&ctx.data().database_pool, &giver_id, &receiver_id, rep_value, &reason)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(Some(rep))
}
