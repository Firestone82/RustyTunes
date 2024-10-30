use crate::bot::{Context, MusicBotError};
use crate::checks::channel_checks::check_author_in_same_voice_channel;
use crate::player::player::Player;
use crate::service::embed_service;
use serenity::all::CreateEmbed;
use tokio::sync::RwLockWriteGuard;

#[poise::command(
    prefix_command,
    check = "check_author_in_same_voice_channel",
    aliases("vol"),
)]
pub async fn volume(ctx: Context<'_>, mut volume: Option<f32>) -> Result<(), MusicBotError> {
    let mut player: RwLockWriteGuard<Player> = ctx.data().player.write().await;
    // TODO: Maybe split it into write lock and read lock?
    
    if let Some(vol) = volume {
        volume = vol.clamp(1.0, 1000.0).into();
    }

    if let Some(volume) = volume {
        println!("Setting volume to: {:?}", volume);
        
        player.set_volume(volume).await?;

        let embed: CreateEmbed = embed_service::create_volume_change_embed(volume);
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;

        // TODO: Question: Why do I need to have this into variable, then to use it in query?
        let guild_id: i64 = ctx.guild_id().unwrap().into();

        sqlx::query!(
            "UPDATE guilds SET volume = $1 WHERE guild_id = $2",
            volume, guild_id
        ).execute(&ctx.data().database).await.expect("TODO: panic message");
    } else {
        let embed: CreateEmbed = embed_service::create_volume_embed(player.volume * 100.0);
        let _ = embed_service::send_context_embed(ctx, embed, true, Some(30)).await?;
    }

    Ok(())
}
