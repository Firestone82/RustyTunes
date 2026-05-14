use crate::bot::{Context, MusicBotData, MusicBotError};
use crate::service::embed_service::SendEmbed;
use serenity::all::{
    Color, CreateEmbed, EditMember, GuildId, Member, Mentionable, PartialGuild, User,
};

#[derive(Debug, poise::Modal)]
#[name = "Rename"]
struct RenameModal {
    #[name = "New nickname"]
    #[placeholder = "leave empty to reset to original name"]
    #[max_length = 32]
    new_name: Option<String>,
}

/// Set a user's nickname. Caller's top role must be at-or-above the target's.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn rename(
    ctx: Context<'_>,
    user: User,
    #[rest] new_name: Option<String>,
) -> Result<(), MusicBotError> {
    do_rename(ctx, user, new_name).await
}

/// Rename a user via right-click → Apps → Rename. Opens a modal for the new nickname.
#[poise::command(context_menu_command = "Rename", guild_only)]
pub async fn rename_context(
    ctx: poise::ApplicationContext<'_, MusicBotData, MusicBotError>,
    user: User,
) -> Result<(), MusicBotError> {
    let data = match poise::Modal::execute(ctx).await? {
        Some(d) => d,
        None => return Ok(()),
    };
    let RenameModal { new_name } = data;

    do_rename(poise::Context::Application(ctx), user, new_name).await
}

async fn do_rename(
    ctx: Context<'_>,
    user: User,
    new_name: Option<String>,
) -> Result<(), MusicBotError> {
    let guild_id: GuildId = ctx.guild_id().ok_or_else(|| {
        MusicBotError::InternalError("Rename is only available in guilds".to_string())
    })?;

    let guild: PartialGuild = ctx
        .http()
        .get_guild(guild_id)
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to fetch guild: {e}")))?;

    let actor: Member = guild
        .member(ctx.http(), ctx.author().id)
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to fetch your member: {e}")))?;
    let target: Member = guild
        .member(ctx.http(), user.id)
        .await
        .map_err(|e| MusicBotError::InternalError(format!("Failed to fetch target member: {e}")))?;

    let actor_top = highest_role_position(&guild, &actor);
    let target_top = highest_role_position(&guild, &target);

    let is_owner = guild.owner_id == ctx.author().id;
    let is_self = target.user.id == ctx.author().id;

    if !is_owner && !is_self && actor_top < target_top {
        CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("🚫  Insufficient role")
            .description(format!(
                "You can't rename {} — their top role outranks yours.",
                target.mention()
            ))
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let trimmed = new_name.as_deref().map(str::trim).unwrap_or("");
    let edit = if trimmed.is_empty() {
        EditMember::new().nickname("")
    } else if trimmed.chars().count() > 32 {
        CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("🚫  Nickname too long")
            .description("Discord nicknames are limited to 32 characters.")
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    } else {
        EditMember::new().nickname(trimmed)
    };

    let previous = target.nick.clone().unwrap_or_else(|| {
        target
            .user
            .global_name
            .clone()
            .unwrap_or_else(|| target.user.name.clone())
    });

    if let Err(e) = guild_id.edit_member(ctx.http(), target.user.id, edit).await {
        tracing::error!("Failed to rename {}: {:?}", target.user.id, e);
        CreateEmbed::new()
            .color(Color::DARK_RED)
            .title("🚫  Rename failed")
            .description(format!(
                "Discord rejected the rename: `{e}`.\nThe bot's role probably sits below {}'s top role.",
                target.mention()
            ))
            .send_context(ctx, true, Some(30))
            .await?;
        return Ok(());
    }

    let next = if trimmed.is_empty() {
        target
            .user
            .global_name
            .clone()
            .unwrap_or(target.user.name.clone())
    } else {
        trimmed.to_string()
    };

    CreateEmbed::new()
        .color(Color::DARK_BLUE)
        .title("✏️  Renamed")
        .description(format!("`{}` → `{}`", previous, next))
        .field("Target:", target.mention().to_string(), true)
        .send_context(ctx, true, None)
        .await?;

    Ok(())
}

fn highest_role_position(guild: &PartialGuild, member: &Member) -> u16 {
    member
        .roles
        .iter()
        .filter_map(|role_id| guild.roles.get(role_id))
        .map(|role| role.position)
        .max()
        .unwrap_or(0)
}
