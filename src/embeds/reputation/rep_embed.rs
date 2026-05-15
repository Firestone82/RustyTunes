use crate::commands::reputation::Rep;
use serenity::all::{CreateEmbed, CreateEmbedFooter, User};

pub enum ReputationEmbed<'a> {
    SelfError,
    SpamError,
    PlusRep(&'a RepEmbed<'a>),
    MinusRep(&'a RepEmbed<'a>),
    List(&'a [Rep], &'a str, i64, usize),
    NotFound,
}

pub struct RepEmbed<'a> {
    pub giver_id: &'a User,
    pub receiver_id: &'a User,
    pub reason: String,
    pub overall_rep: i64,
}

impl ReputationEmbed<'_> {
    pub fn to_embed(&self) -> CreateEmbed {
        match self {
            ReputationEmbed::PlusRep(rep) => CreateEmbed::new()
                .color(serenity::all::Color::DARK_GREEN)
                .title("✅  Reputation Increased")
                .field("Given by", format!("{}", rep.giver_id), true)
                .field("ㅤ", "-->", true)
                .field("Target", format!("{}", rep.receiver_id), true)
                .field("Amount", "+1 💚", true)
                .field(" ", " ", true)
                .field("Current rep", format!("`{}`", rep.overall_rep), true)
                .field("Reason", format!(">>> {}", rep.reason), false),
            ReputationEmbed::MinusRep(rep) => CreateEmbed::new()
                .color(serenity::all::Color::DARK_RED)
                .title("❌  Reputation Decreased")
                .field("Given by", format!("{}", rep.giver_id), true)
                .field("ㅤ", "-->", true)
                .field("Target", format!("{}", rep.receiver_id), true)
                .field("Amount", "-1 💔", true)
                .field(" ", " ", true)
                .field("Current rep", format!("`{}`", rep.overall_rep), true)
                .field("Reason", format!(">>> {}", rep.reason), false),
            ReputationEmbed::List(reps, for_user, calculated_rep, rep_count) => {
                let mut embed = CreateEmbed::new()
                    .color(serenity::all::Color::DARK_BLUE)
                    .title("📜  Reputation logs")
                    .description(format!("**👤 User:** <@{}>", for_user));

                match reps.is_empty() {
                    false => {
                        for rep in reps.iter() {
                            embed = embed
                                .field("Amount", if rep.rep_value == 1 { "+1 💚" } else { "-1 💔" }, true)
                                .field("Given by", format!("<@{}>", rep.giver_id), true)
                                .field("Date", format!("`{}`", rep.created_at.date()), true)
                                .field("Reason", format!(">>> {}", rep.reason), false);
                        }
                        embed = embed.footer(CreateEmbedFooter::new(format!("📊 Overall rep: {} | 📑 Logs: {}", calculated_rep, rep_count,)));
                    }
                    true => {
                        embed = embed.footer(CreateEmbedFooter::new("This user has no reputation logs yet."));
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
