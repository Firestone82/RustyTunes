CREATE TABLE IF NOT EXISTS notify_me (
    user_id TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    notify_at DATETIME,
    message_id TEXT
);

CREATE TABLE IF NOT EXISTS guilds (
    guild_id TEXT NOT NULL PRIMARY KEY,
    volume INTEGER DEFAULT 1
)