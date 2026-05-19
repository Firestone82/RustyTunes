use crate::bot::{Context, MusicBotError};
use crate::embeds::reputation::rep_embed::{RepEmbed, ReputationEmbed};
use serenity::all::{
    ActionRowComponent, ButtonStyle, ComponentInteractionCollector, CreateActionRow, CreateButton, CreateEmbed, CreateInputText, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateModal, EditMessage, InputTextStyle, Message, ModalInteraction, ModalInteractionCollector, User,
};
use serenity::futures::StreamExt;
use std::time::{Duration, Instant};

const REP_ACTION_WINDOW: Duration = Duration::from_secs(30);
const REP_MODAL_TIMEOUT: Duration = Duration::from_secs(120);
const REP_REASON_MAX_LEN: u16 = 500;
const REP_REASON_INPUT_ID: &str = "rep_reason";

pub struct RepActionContext<'a> {
    pub rep_id: i64,
    pub giver: &'a User,
    pub receiver: &'a User,
    pub rep_value: i64,
    pub reason: String,
    pub overall_rep: i64,
}

/// Build the Edit / Undo action row shown under a freshly-given rep message.
pub fn rep_action_buttons(rep_id: i64) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(format!("rep_edit_{rep_id}"))
            .label("✏️ Edit")
            .style(ButtonStyle::Primary),
        CreateButton::new(format!("rep_undo_{rep_id}"))
            .label("↩️ Undo")
            .style(ButtonStyle::Danger),
    ])]
}

/// Watch a rep message for `REP_ACTION_WINDOW` and handle Edit / Undo clicks
/// from the giver. After the window closes, the buttons are stripped.
pub async fn watch_rep_actions(
    ctx: Context<'_>,
    mut message: Message,
    mut rep: RepActionContext<'_>,
) -> Result<(), MusicBotError> {
    let edit_id = format!("rep_edit_{}", rep.rep_id);
    let undo_id = format!("rep_undo_{}", rep.rep_id);
    let modal_id = format!("rep_edit_modal_{}", rep.rep_id);

    let deadline = Instant::now() + REP_ACTION_WINDOW;

    let mut collector = Box::pin(
        ComponentInteractionCollector::new(ctx.serenity_context())
            .message_id(message.id)
            .stream(),
    );

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }

        let Ok(Some(interaction)) = tokio::time::timeout(remaining, collector.next()).await else {
            break;
        };

        if interaction.user.id != rep.giver.id {
            let _ = interaction
                .create_response(
                    ctx.http(),
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Only the person who gave this reputation can edit or undo it.")
                            .ephemeral(true),
                    ),
                )
                .await;
            continue;
        }

        if interaction.data.custom_id == edit_id {
            let modal = CreateModal::new(&modal_id, "Edit reputation reason").components(vec![CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Paragraph, "Reason", REP_REASON_INPUT_ID)
                    .value(&rep.reason)
                    .required(true)
                    .max_length(REP_REASON_MAX_LEN),
            )]);

            if let Err(error) = interaction
                .create_response(ctx.http(), CreateInteractionResponse::Modal(modal))
                .await
            {
                tracing::debug!("Failed to open rep edit modal: {:?}", error);
                continue;
            }

            let filter_modal_id = modal_id.clone();
            let giver_id = rep.giver.id;
            let modal_interaction = ModalInteractionCollector::new(ctx.serenity_context())
                .filter(move |mi| mi.data.custom_id == filter_modal_id && mi.user.id == giver_id)
                .timeout(REP_MODAL_TIMEOUT)
                .await;

            let Some(modal_interaction) = modal_interaction else {
                continue;
            };

            let new_reason = extract_modal_reason(&modal_interaction).unwrap_or_default();
            let trimmed = new_reason.trim();
            if trimmed.is_empty() {
                let _ = modal_interaction
                    .create_response(
                        ctx.http(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Reason cannot be empty.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                continue;
            }

            if let Err(error) = sqlx::query!(
                "UPDATE reputation_logs SET reason = ? WHERE id = ?",
                trimmed,
                rep.rep_id
            )
            .execute(&*ctx.data().database_pool)
            .await
            {
                tracing::warn!("Failed to update rep reason: {:?}", error);
                let _ = modal_interaction
                    .create_response(
                        ctx.http(),
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content("Failed to update reputation reason.")
                                .ephemeral(true),
                        ),
                    )
                    .await;
                continue;
            }

            rep.reason = trimmed.to_string();

            let _ = modal_interaction
                .create_response(
                    ctx.http(),
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new()
                            .embed(render_rep_embed(&rep, true))
                            .components(rep_action_buttons(rep.rep_id)),
                    ),
                )
                .await;
        } else if interaction.data.custom_id == undo_id {
            let _ = interaction.defer(ctx.http()).await;

            if let Err(error) = sqlx::query!("DELETE FROM reputation_logs WHERE id = ?", rep.rep_id)
                .execute(&*ctx.data().database_pool)
                .await
            {
                tracing::warn!("Failed to delete rep: {:?}", error);
                continue;
            }

            let _ = message.delete(ctx.http()).await;
            return Ok(());
        }
    }

    let _ = message
        .edit(
            ctx.serenity_context(),
            EditMessage::new().components(Vec::new()),
        )
        .await;

    Ok(())
}

fn extract_modal_reason(modal: &ModalInteraction) -> Option<String> {
    for row in &modal.data.components {
        for component in &row.components {
            if let ActionRowComponent::InputText(input) = component {
                if input.custom_id == REP_REASON_INPUT_ID {
                    return input.value.clone();
                }
            }
        }
    }
    None
}

fn render_rep_embed(
    rep: &RepActionContext<'_>,
    edited: bool,
) -> CreateEmbed {
    let rep_embed = RepEmbed {
        giver_id: rep.giver,
        receiver_id: rep.receiver,
        reason: rep.reason.clone(),
        overall_rep: rep.overall_rep,
        edited,
    };
    if rep.rep_value > 0 {
        ReputationEmbed::PlusRep(&rep_embed).to_embed()
    } else {
        ReputationEmbed::MinusRep(&rep_embed).to_embed()
    }
}
