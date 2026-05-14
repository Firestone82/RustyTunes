use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::spam_protection;
use crate::embeds::rep_embed::{RepEmbed, ReputationEmbed};
use crate::service::embed_service::SendEmbed;
use serenity::all::User;

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
        ReputationEmbed::SelfError
            .to_embed()
            .send_context(ctx, true, None)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(());
    }

    let giver_id = ctx.author().id.to_string();
    let receiver_id = user.id.to_string();

    if spam_protection(ctx, receiver_id.clone()).await? {
        ReputationEmbed::SpamError
            .to_embed()
            .send_context(ctx, true, None)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
        return Ok(());
    }

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