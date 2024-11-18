use crate::bot::{Context, MusicBotError};

/**
* Help command
*/
#[poise::command(
    prefix_command, slash_command,
    track_edits,
    aliases("h"),
)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), MusicBotError> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration {
            extra_text_at_bottom: "RustyTunes (Rusty) created by Pavel Mikula as VŠB-TUO project. :)",
            ..Default::default()
        },
    ).await?;

    Ok(())
}
