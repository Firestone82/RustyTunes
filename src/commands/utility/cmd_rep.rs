use crate::bot::{Context, MusicBotError};
use crate::embeds::rep_embed::{RepEmbed, ReputationEmbed};
use crate::service::embed_service::SendEmbed;
use serenity::all::User;
use sqlx::types::time::OffsetDateTime;

/// Give positive reputation to a user with a reason.
#[poise::command(prefix_command, slash_command, aliases("+rep", "plusrep"))]
pub async fn add_rep(
    ctx: Context<'_>,
    #[description = "User to give reputation to"] user: User,
    #[rest]
    #[description = "Reason for giving reputation"]
    reason: String,
) -> Result<(), MusicBotError> {
    if user.id == ctx.author().id {
        ReputationEmbed::SelfError
            .to_embed()
            .send_context(ctx, false, None)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(());
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();

    sqlx::query!(
        "
INSERT INTO reputation_logs (giver_id, receiver_id, rep_value, reason)
VALUES (?, ?, ?, ?)
",
        giver_id,
        receiver_id,
        1,
        reason
    )
        .execute(&*ctx.data().database_pool)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    ReputationEmbed::PlusRep(&RepEmbed {
        giver_id,
        receiver_id,
        reason,
    })
        .to_embed()
        .send_context(ctx, false, None)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(())
}

/// Give negative reputation to a user with a reason.
#[poise::command(prefix_command, slash_command, aliases("-rep", "minusrep"))]
pub async fn remove_rep(
    ctx: Context<'_>,
    #[description = "User to remove reputation from"] user: User,
    #[rest]
    #[description = "Reason for removing reputation"]
    reason: String,
) -> Result<(), MusicBotError> {
    if user.id == ctx.author().id {
        ctx.say("You cannot remove reputation from yourself.")
            .await?;
        return Ok(());
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();

    sqlx::query!(
        "
INSERT INTO reputation_logs (giver_id, receiver_id, rep_value, reason)
VALUES (?, ?, ?, ?)
",
        giver_id,
        receiver_id,
        -1,
        reason,
    )
        .execute(&*ctx.data().database_pool)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    ReputationEmbed::MinusRep(&RepEmbed {
        giver_id,
        receiver_id,
        reason,
    })
        .to_embed()
        .send_context(ctx, false, None)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(())
}

/// List the reputation of a user, including history.
#[poise::command(prefix_command, slash_command, aliases("reps", "repinfo"))]
pub async fn list_rep(
    ctx: Context<'_>,
    #[description = "User to check reputation for (optional, defaults to yourself)"] user: Option<
        User,
    >,
) -> Result<(), MusicBotError> {
    // Pokud není zadán uživatel, použijeme toho, kdo příkaz zavolal
    let target_user = user.as_ref().unwrap_or_else(|| ctx.author());
    let target_id = target_user.id.to_string();

    let total_rep: i64 = sqlx::query_scalar!(
        "
SELECT COALESCE(SUM(rep_value), 0) FROM reputation_logs WHERE receiver_id == ?
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

    ReputationEmbed::List(&logs, &target_id, &total_rep)
        .to_embed()
        .send_context(ctx, false, None)
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct Rep {
    pub id: i64,
    pub giver_id: String,
    pub receiver_id: String,
    pub rep_value: i64,
    pub reason: String,
    pub created_at: OffsetDateTime,
}
