PRAGMA foreign_keys=ON;
BEGIN;
CREATE TABLE movements (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, modality TEXT NOT NULL, muscle_groups TEXT, equipment TEXT, default_unit TEXT);
CREATE TABLE sessions (id INTEGER PRIMARY KEY, started_at DATETIME NOT NULL, ended_at DATETIME, kind TEXT NOT NULL, title TEXT, location TEXT, felt INTEGER, rpe INTEGER, notes TEXT, source TEXT NOT NULL DEFAULT 'manual');
CREATE TABLE session_items (id INTEGER PRIMARY KEY, session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE, position INTEGER NOT NULL, movement_id INTEGER NOT NULL REFERENCES movements(id), structure TEXT, notes TEXT);
CREATE TABLE efforts (id INTEGER PRIMARY KEY, session_item_id INTEGER NOT NULL REFERENCES session_items(id) ON DELETE CASCADE, position INTEGER NOT NULL, reps INTEGER, weight_kg REAL, duration_s REAL, distance_m REAL, elevation_m REAL, rest_s REAL, avg_hr INTEGER, max_hr INTEGER, rpe INTEGER, is_warmup INTEGER NOT NULL DEFAULT 0, notes TEXT);
CREATE TABLE body_metrics (id INTEGER PRIMARY KEY, date DATETIME NOT NULL, metric TEXT NOT NULL, value REAL NOT NULL, unit TEXT NOT NULL, source TEXT NOT NULL DEFAULT 'manual');
CREATE TABLE preferences (id INTEGER PRIMARY KEY, key TEXT NOT NULL, value TEXT NOT NULL, confidence REAL NOT NULL, source TEXT NOT NULL, evidence TEXT, active INTEGER NOT NULL DEFAULT 1, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP);
CREATE TABLE workout_plans (id INTEGER PRIMARY KEY, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, for_date DATE, focus TEXT, plan_json TEXT NOT NULL, rationale TEXT, status TEXT NOT NULL DEFAULT 'proposed', rating INTEGER, feedback TEXT);
CREATE TABLE model_calls (id INTEGER PRIMARY KEY, at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, purpose TEXT NOT NULL, model TEXT NOT NULL, prompt_tokens INTEGER, completion_tokens INTEGER, cached_tokens INTEGER, ok INTEGER NOT NULL, error TEXT);
CREATE TABLE batch_buffer (id INTEGER PRIMARY KEY, chat_id INTEGER NOT NULL, message_id INTEGER NOT NULL, text TEXT NOT NULL, sent_at DATETIME NOT NULL, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(chat_id, message_id));
CREATE TABLE batch_state (chat_id INTEGER PRIMARY KEY, opened_at DATETIME NOT NULL, auto_flush_at DATETIME NOT NULL);
CREATE TABLE external_activities (id INTEGER PRIMARY KEY, source TEXT NOT NULL, external_id TEXT NOT NULL, session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL, payload TEXT NOT NULL, imported_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, UNIQUE(source, external_id));
CREATE TABLE effort_splits (id INTEGER PRIMARY KEY, session_item_id INTEGER NOT NULL REFERENCES session_items(id) ON DELETE CASCADE, position INTEGER NOT NULL, distance_m REAL, duration_s REAL, avg_hr INTEGER);
CREATE INDEX effort_splits_item_position ON effort_splits(session_item_id, position);
CREATE INDEX sessions_started_at ON sessions(started_at);
CREATE INDEX body_metrics_metric_date ON body_metrics(metric, date);
CREATE TABLE processed_updates (update_id INTEGER PRIMARY KEY);
ALTER TABLE preferences ADD COLUMN reviewed_at DATETIME;
CREATE TABLE reflection_runs (
    date DATE PRIMARY KEY,
    completed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    proposals_created INTEGER NOT NULL
);
CREATE TABLE hr_samples (id INTEGER PRIMARY KEY, session_item_id INTEGER NOT NULL REFERENCES session_items(id) ON DELETE CASCADE, at DATETIME NOT NULL, bpm INTEGER NOT NULL);
CREATE INDEX hr_samples_item_at ON hr_samples(session_item_id, at);
PRAGMA user_version=5;
COMMIT;
VACUUM;
