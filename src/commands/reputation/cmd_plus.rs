use crate::bot::{Context, MusicBotError};
use crate::embeds::rep_embed::{RepEmbed, ReputationEmbed};
use crate::service::embed_service::SendEmbed;
use serenity::all::User;

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



