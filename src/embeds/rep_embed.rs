use crate::commands::utility::cmd_rep::Rep;
use serenity::all::{CreateEmbed, CreateEmbedFooter};

pub enum ReputationEmbed<'a> {
    SelfError,
    SpamError,
    PlusRep(&'a RepEmbed),
    MinusRep(&'a RepEmbed),
    List(&'a [Rep], &'a str, &'a i64),
    NotFound,
}

pub struct RepEmbed {
    pub giver_id: String,
    pub receiver_id: String,
    pub reason: String,
}

impl ReputationEmbed<'_> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            ReputationEmbed::PlusRep(rep) => CreateEmbed::new()
                .color(serenity::all::Color::DARK_GREEN)
                .title("✅  Reputation given")
                .description(format!(
                    "Gave +1 reputation from <@{}> to <@{}> \nfor: {}",
                    rep.giver_id, rep.receiver_id, rep.reason
                )),

            ReputationEmbed::MinusRep(rep) => CreateEmbed::new()
                .color(serenity::all::Color::DARK_RED)
                .title("❌  Reputation taken")
                .description(format!(
                    "Took -1 reputation from <@{}> to <@{}> \nfor: {}",
                    rep.giver_id, rep.receiver_id, rep.reason
                )),

            ReputationEmbed::List(reps, for_user, calculated_rep) => {
                let mut embed = CreateEmbed::new()
                    .color(serenity::all::Color::DARK_BLUE)
                    .title("📜  Reputation logs")
                    .description(format!(
                        "Here are the reputation logs for user:<@{}>",
                        for_user
                    ));

                match reps.is_empty() {
                    false => {
                        for rep in reps.iter() {
                            embed = embed.field(
                                format!("Reputation {}", if rep.rep_value == 1 { "added ✅" } else { "removed ❌" }),
                                format!("from <@{}> \nreason: {} ({})", rep.giver_id, rep.reason, rep.created_at.date()),
                                false,
                            );
                        }
                        embed = embed.footer(CreateEmbedFooter::new(format!(
                            "Total reputation logs: {}\nTotal reputation: {}",
                            reps.len(),
                            calculated_rep
                        )));
                    }
                    true => {
                        embed = embed.description(format!(
                            "No reputation logs found for user:<@{}>",
                            for_user
                        ));
                    }
                }
                embed
            }
            ReputationEmbed::NotFound => CreateEmbed::new()
                .color(serenity::all::Color::DARK_RED)
                .title("🚫  Not found")
                .description("No reputation logs found for the specified user."),

            ReputationEmbed::SpamError => CreateEmbed::new()
                .color(serenity::all::Color::DARK_RED)
                .title("🚫  Too fast")
                .description("You are giving/taking reputation too fast. Please wait a bit before trying again."),

            ReputationEmbed::SelfError => CreateEmbed::new()
                .color(serenity::all::Color::DARK_RED)
                .title("🚫  Invalid target")
                .description("You cannot give reputation to yourself."),
        }
    }
}
