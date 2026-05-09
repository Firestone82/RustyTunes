CREATE TABLE notify_me_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    message_id INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    notify_at DATETIME,
    note TEXT DEFAULT NULL
);

INSERT INTO notify_me_new (id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note)
SELECT id, guild_id, channel_id, user_id, message_id, created_at, notify_at, note FROM notify_me;

DROP TABLE notify_me;
ALTER TABLE notify_me_new RENAME TO notify_me;
