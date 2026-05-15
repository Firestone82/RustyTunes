use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::process_rep;
use crate::embeds::reputation::rep_embed::{RepEmbed, ReputationEmbed};
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
    if let Some(overall_rep) = process_rep(ctx, user.clone(), reason.clone(), 1).await? {
        ReputationEmbed::PlusRep(&RepEmbed {
            giver_id: ctx.author(),
            receiver_id: &user,
            reason,
            overall_rep,
        })
            .to_embed()
            .send_context(ctx, false, None)
            .await
            .map_err(|e| MusicBotError::InternalError(e.to_string()))?;
    }

    Ok(())
}
