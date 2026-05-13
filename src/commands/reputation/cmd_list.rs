use crate::bot::{Context, MusicBotError};
use crate::commands::reputation::Rep;
use crate::embeds::rep_embed::ReputationEmbed;
use crate::service::embed_service::SendEmbed;
use serenity::all::User;

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

    //TODO paging
    //
    // let reply_handle = ctx.send(
    //     poise::CreateReply::default()
    //         .embed(PlayerEmbed::Search(&tracks).to_embed())
    //         .components(rows)
    //         .reply(true)
    // ).await
    //     .map_err(|error| MusicBotError::InternalError(error.to_string()))?;
    //
    // let message: Message = reply_handle.into_message().await
    //     .map_err(|error| MusicBotError::InternalError(error.to_string()))?;
    //
    // let interaction = message
    //     .await_component_interaction(ctx.serenity_context().shard.clone())
    //     .timeout(Duration::from_secs(60 * 2));

    Ok(())
}