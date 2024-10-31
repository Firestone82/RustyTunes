CREATE TABLE IF NOT EXISTS notify_me (
    guild_id BIGINT NOT NULL,
    channel_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    message_id BIGINT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    notify_at DATETIME
);

CREATE TABLE IF NOT EXISTS guilds (
    guild_id TEXT NOT NULL PRIMARY KEY,
    volume INTEGER DEFAULT 1
)