CREATE TABLE IF NOT EXISTS notify_me (
    userId TEXT NOT NULL,
    createdAt DATETIME DEFAULT CURRENT_TIMESTAMP,
    notifyAt DATETIME,
    messageId TEXT
);
