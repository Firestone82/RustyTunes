use crate::bot::{Context, MusicBotError};
use crate::service::channel_service;

#[poise::command(
    prefix_command,
    check = "check_author_in_voice_channel",
)]
pub async fn join(ctx: Context<'_>) -> Result<(), MusicBotError> {
    channel_service::join_user_channel(ctx).await?;
    Ok(())
}

// pub fn join() -> poise::Command<MusicBotData, MusicBotError> {
//     async fn inner(ctx: Context<'_>) -> Result<(), MusicBotError> {
//         channel_service::join_user_channel(ctx).await?;
//         Ok(())
//     }
// 
//     poise::Command {
//         name: "join".parse().unwrap(),
//         description: Some("My command description".to_owned()),
//         help_text: Some("My command help text".to_owned()),
//         prefix_action: Some(|ctx| Box::pin(async move {
//             if ctx.author.voice_channel(ctx.guild_id).await.is_none() {
//                 Err(FrameworkError::CheckFailed("You must be in a voice channel to use this command".to_owned()))
//             } else {
//                 Ok(())
//             }
//         })),
//         ..poise::Command::default()
//     }
// }