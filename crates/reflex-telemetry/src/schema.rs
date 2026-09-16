use rusqlite::Connection;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS decisions (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    provider TEXT NOT NULL,
    decision_type TEXT NOT NULL,
    context TEXT NOT NULL,
    task_id TEXT,
    selected TEXT NOT NULL,
    confidence REAL NOT NULL,
    probabilities_json TEXT NOT NULL,
    action TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    latency_ms INTEGER NOT NULL,
    cost_estimate REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS outcomes (
    decision_id TEXT PRIMARY KEY,
    result TEXT NOT NULL,
    source TEXT NOT NULL,
    verified_at TEXT NOT NULL,
    details TEXT,
    FOREIGN KEY(decision_id) REFERENCES decisions(id)
);

CREATE TABLE IF NOT EXISTS shadow_records (
    id TEXT PRIMARY KEY,
    task_id TEXT,
    timestamp TEXT NOT NULL,
    predicted_action TEXT NOT NULL,
    confidence REAL NOT NULL,
    actual_action TEXT NOT NULL,
    final_outcome TEXT,
    latency_ms INTEGER NOT NULL,
    cost_estimate REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS calibration_runs (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    dataset_size INTEGER NOT NULL,
    brier_score REAL NOT NULL,
    ece REAL NOT NULL,
    coverage REAL NOT NULL,
    recommended_threshold REAL NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_decisions_timestamp ON decisions(timestamp);
CREATE INDEX IF NOT EXISTS idx_decisions_task_id ON decisions(task_id);
CREATE INDEX IF NOT EXISTS idx_outcomes_result ON outcomes(result);
"#;

pub fn initialize_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
