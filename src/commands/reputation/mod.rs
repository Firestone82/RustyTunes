use time::OffsetDateTime;

pub mod cmd_plus;
pub mod cmd_list;
pub mod cmd_minus;

#[derive(sqlx::FromRow)]
pub struct Rep {
    pub id: i64,
    pub giver_id: String,
    pub receiver_id: String,
    pub rep_value: i64,
    pub reason: String,
    pub created_at: OffsetDateTime,
}
