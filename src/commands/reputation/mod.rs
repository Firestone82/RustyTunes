use crate::bot::{Context, MusicBotError};

use time::{Duration, OffsetDateTime};

pub mod cmd_plus;
pub mod cmd_list;
pub mod cmd_minus;

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