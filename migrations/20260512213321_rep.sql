CREATE TABLE IF NOT EXISTS reputation_logs
(
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    giver_id    TEXT    NOT NULL,
    receiver_id TEXT    NOT NULL,
    rep_value   INTEGER NOT NULL,
    reason      TEXT    NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_receiver ON reputation_logs (receiver_id);
CREATE INDEX idx_giver_time ON reputation_logs (giver_id, created_at);