-- Migration to create active_events table for runtime game events
CREATE TABLE IF NOT EXISTS active_events (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    starts_at INTEGER NOT NULL,
    ends_at INTEGER NOT NULL,
    config_json TEXT NOT NULL
);
