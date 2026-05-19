use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::process_rep;
use crate::embeds::reputation::rep_embed::{RepEmbed, ReputationEmbed};
use crate::service::rep_interaction_service::{rep_action_buttons, watch_rep_actions, RepActionContext};
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
    let Some(result) = process_rep(ctx, user.clone(), reason.clone(), -1).await? else {
        return Ok(());
    };

    let embed = ReputationEmbed::MinusRep(&RepEmbed {
        giver_id: ctx.author(),
        receiver_id: &user,
        reason: reason.clone(),
        overall_rep: result.overall_rep,
        edited: false,
    })
    .to_embed();

    let message = ctx
        .send(
            poise::CreateReply::default()
                .embed(embed)
                .components(rep_action_buttons(result.rep_id)),
        )
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?
        .into_message()
        .await
        .map_err(|e| MusicBotError::InternalError(e.to_string()))?;

    watch_rep_actions(
        ctx,
        message,
        RepActionContext {
            rep_id: result.rep_id,
            giver: ctx.author(),
            receiver: &user,
            rep_value: -1,
            reason,
            overall_rep: result.overall_rep,
        },
    )
    .await?;

    Ok(())
}
