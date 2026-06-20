use crate::commands::reputation::{LeaderboardEntry, Rep};
use serenity::all::{CreateEmbed, CreateEmbedFooter, User};

pub enum ReputationEmbed<'a> {
    SelfError,
    SpamError,
    PlusRep(&'a RepEmbed<'a>),
    MinusRep(&'a RepEmbed<'a>),
    List(&'a [Rep], &'a str, i64, usize),
    LeaderboardSummary {
        top: &'a [LeaderboardEntry],
        middle_count: usize,
        bottom: &'a [LeaderboardEntry],
        bottom_start_rank: usize,
        total_entries: usize,
    },
    LeaderboardPage {
        entries: &'a [LeaderboardEntry],
        start_rank: usize,
        total_entries: usize,
    },
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
                                .field(
                                    "Amount",
                                    if rep.rep_value == 1 { "+1 💚" } else { "-1 💔" },
                                    true,
                                )
                                .field("Given by", format!("<@{}>", rep.giver_id), true)
                                .field("Date", format!("`{}`", rep.created_at.date()), true)
                                .field("Reason", format!(">>> {}", rep.reason), false);
                        }
                        embed = embed.footer(CreateEmbedFooter::new(format!(
                            "📊 Overall rep: {} | 📑 Logs: {}",
                            calculated_rep, rep_count,
                        )));
                    }
                    true => {
                        embed = embed.footer(CreateEmbedFooter::new(
                            "This user has no reputation logs yet.",
                        ));
                    }
                }
                embed
            }
            ReputationEmbed::LeaderboardSummary {
                top,
                middle_count,
                bottom,
                bottom_start_rank,
                total_entries,
            } => {
                let mut embed = CreateEmbed::new()
                    .color(serenity::all::Color::DARK_GOLD)
                    .title("🏆  Reputation leaderboard");

                if *total_entries == 0 {
                    embed = embed
                        .description("No reputation has been given out yet.")
                        .footer(CreateEmbedFooter::new("Be the first to !+rep someone."));
                } else {
                    let mut sections: Vec<String> = Vec::new();

                    if !top.is_empty() {
                        sections.push(render_entries(top, 0));
                    }

                    if *middle_count > 0 {
                        sections.push(format!(
                            "⋯ *{} more {} in between* ⋯",
                            middle_count,
                            if *middle_count == 1 { "user" } else { "users" },
                        ));
                    }

                    if !bottom.is_empty() {
                        sections.push(render_entries(bottom, *bottom_start_rank));
                    }

                    embed = embed
                        .description(sections.join("\n\n"))
                        .footer(CreateEmbedFooter::new(format!(
                            "👥 Ranked users: {}",
                            total_entries
                        )));
                }
                embed
            }
            ReputationEmbed::LeaderboardPage { entries, start_rank, total_entries } => {
                let mut embed = CreateEmbed::new()
                    .color(serenity::all::Color::DARK_GOLD)
                    .title("🏆  Reputation leaderboard");

                if entries.is_empty() {
                    embed = embed.description("No reputation has been given out yet.");
                } else {
                    embed = embed
                        .description(render_entries(entries, *start_rank))
                        .footer(CreateEmbedFooter::new(format!(
                            "👥 Ranked users: {}",
                            total_entries
                        )));
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

fn rank_prefix(rank: usize) -> String {
    match rank {
        1 => "🥇".to_string(),
        2 => "🥈".to_string(),
        3 => "🥉".to_string(),
        _ => format!("**#{}**", rank),
    }
}

fn render_entries(
    entries: &[LeaderboardEntry],
    start_rank_zero_based: usize,
) -> String {
    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            let rank = start_rank_zero_based + idx + 1;
            format!(
                "{} <@{}> — `{:+}` ({} {})",
                rank_prefix(rank),
                entry.receiver_id,
                entry.total_rep,
                entry.log_count,
                if entry.log_count == 1 { "log" } else { "logs" },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
