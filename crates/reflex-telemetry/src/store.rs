use crate::error::TelemetryError;
use crate::models::{DecisionRecord, OutcomeRecord, ShadowRecord};
use crate::schema::initialize_schema;
use chrono::{DateTime, Utc};
use reflex_core::{DecisionId, Outcome, OutcomeSource, ReflexAction, RiskLevel};
use rusqlite::{params, Connection};
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct TelemetryStore {
    conn: Arc<Mutex<Connection>>,
}

impl TelemetryStore {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, TelemetryError> {
        let conn = Connection::open(path)?;
        initialize_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_in_memory() -> Result<Self, TelemetryError> {
        let conn = Connection::open_in_memory()?;
        initialize_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn record_decision(&self, rec: &DecisionRecord) -> Result<(), TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let action_str = serde_json::to_string(&rec.action)?;
        let risk_str = rec.risk_level.to_string();

        conn.execute(
            r#"INSERT OR REPLACE INTO decisions (
                id, timestamp, provider, decision_type, context, task_id,
                selected, confidence, probabilities_json, action, risk_level,
                latency_ms, cost_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)"#,
            params![
                rec.id.as_str(),
                rec.timestamp.to_rfc3339(),
                rec.provider,
                rec.decision_type,
                rec.context,
                rec.task_id,
                rec.selected,
                rec.confidence,
                rec.probabilities_json,
                action_str,
                risk_str,
                rec.latency_ms as i64,
                rec.cost_estimate,
            ],
        )?;

        Ok(())
    }

    pub fn record_outcome(&self, rec: &OutcomeRecord) -> Result<(), TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let result_str = serde_json::to_string(&rec.result)?;
        let source_str = serde_json::to_string(&rec.source)?;

        conn.execute(
            r#"INSERT OR REPLACE INTO outcomes (
                decision_id, result, source, verified_at, details
            ) VALUES (?1, ?2, ?3, ?4, ?5)"#,
            params![
                rec.decision_id.as_str(),
                result_str,
                source_str,
                rec.verified_at.to_rfc3339(),
                rec.details,
            ],
        )?;

        Ok(())
    }

    pub fn record_shadow(&self, rec: &ShadowRecord) -> Result<(), TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let predicted_str = serde_json::to_string(&rec.predicted_action)?;
        let final_outcome_str = rec
            .final_outcome
            .as_ref()
            .map(|o| serde_json::to_string(o).unwrap_or_default());
        let ci_outcome_str = rec
            .ci_outcome
            .as_ref()
            .map(|o| serde_json::to_string(o).unwrap_or_default())
            .or_else(|| final_outcome_str.clone());

        conn.execute(
            r#"INSERT OR REPLACE INTO shadow_records (
                id, task_id, timestamp, predicted_action, confidence,
                actual_action, verifier_result, ci_outcome, final_outcome, latency_ms, cost_estimate
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
            params![
                rec.id,
                rec.task_id,
                rec.timestamp.to_rfc3339(),
                predicted_str,
                rec.confidence,
                rec.actual_action,
                rec.verifier_result,
                ci_outcome_str,
                final_outcome_str,
                rec.latency_ms as i64,
                rec.cost_estimate,
            ],
        )?;

        Ok(())
    }

    pub fn get_decision(&self, id: &DecisionId) -> Result<Option<DecisionRecord>, TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let mut stmt = conn.prepare(
            r#"SELECT id, timestamp, provider, decision_type, context, task_id,
                      selected, confidence, probabilities_json, action, risk_level,
                      latency_ms, cost_estimate
               FROM decisions WHERE id = ?1"#,
        )?;

        let mut rows = stmt.query(params![id.as_str()])?;
        if let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let ts_str: String = row.get(1)?;
            let action_str: String = row.get(9)?;
            let risk_str: String = row.get(10)?;
            let latency_i64: i64 = row.get(11)?;

            let record = DecisionRecord {
                id: DecisionId::new(id_str).unwrap(),
                timestamp: DateTime::parse_from_rfc3339(&ts_str)
                    .unwrap()
                    .with_timezone(&Utc),
                provider: row.get(2)?,
                decision_type: row.get(3)?,
                context: row.get(4)?,
                task_id: row.get(5)?,
                selected: row.get(6)?,
                confidence: row.get(7)?,
                probabilities_json: row.get(8)?,
                action: serde_json::from_str(&action_str).unwrap_or(ReflexAction::Accept),
                risk_level: RiskLevel::from_str(&risk_str).unwrap_or_default(),
                latency_ms: latency_i64 as u64,
                cost_estimate: row.get(12)?,
            };
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    pub fn get_outcome(
        &self,
        decision_id: &DecisionId,
    ) -> Result<Option<OutcomeRecord>, TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let mut stmt = conn.prepare(
            r#"SELECT decision_id, result, source, verified_at, details
               FROM outcomes WHERE decision_id = ?1"#,
        )?;

        let mut rows = stmt.query(params![decision_id.as_str()])?;
        if let Some(row) = rows.next()? {
            let id_str: String = row.get(0)?;
            let result_str: String = row.get(1)?;
            let source_str: String = row.get(2)?;
            let ts_str: String = row.get(3)?;

            let record = OutcomeRecord {
                decision_id: DecisionId::new(id_str).unwrap(),
                result: serde_json::from_str(&result_str).unwrap_or(Outcome::Unknown),
                source: serde_json::from_str(&source_str).unwrap_or(OutcomeSource::Runtime),
                verified_at: DateTime::parse_from_rfc3339(&ts_str)
                    .unwrap()
                    .with_timezone(&Utc),
                details: row.get(4)?,
            };
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    pub fn list_decisions(&self, limit: usize) -> Result<Vec<DecisionRecord>, TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let mut stmt = conn.prepare(
            r#"SELECT id, timestamp, provider, decision_type, context, task_id,
                      selected, confidence, probabilities_json, action, risk_level,
                      latency_ms, cost_estimate
               FROM decisions ORDER BY timestamp DESC LIMIT ?1"#,
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            let id_str: String = row.get(0)?;
            let ts_str: String = row.get(1)?;
            let action_str: String = row.get(9)?;
            let risk_str: String = row.get(10)?;
            let latency_i64: i64 = row.get(11)?;

            Ok(DecisionRecord {
                id: DecisionId::new(id_str).unwrap(),
                timestamp: DateTime::parse_from_rfc3339(&ts_str)
                    .unwrap()
                    .with_timezone(&Utc),
                provider: row.get(2)?,
                decision_type: row.get(3)?,
                context: row.get(4)?,
                task_id: row.get(5)?,
                selected: row.get(6)?,
                confidence: row.get(7)?,
                probabilities_json: row.get(8)?,
                action: serde_json::from_str(&action_str).unwrap_or(ReflexAction::Accept),
                risk_level: RiskLevel::from_str(&risk_str).unwrap_or_default(),
                latency_ms: latency_i64 as u64,
                cost_estimate: row.get(12)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn list_paired_decisions(
        &self,
    ) -> Result<Vec<(DecisionRecord, Option<OutcomeRecord>)>, TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let mut stmt = conn.prepare(
            r#"SELECT d.id, d.timestamp, d.provider, d.decision_type, d.context, d.task_id,
                      d.selected, d.confidence, d.probabilities_json, d.action, d.risk_level,
                      d.latency_ms, d.cost_estimate,
                      o.result, o.source, o.verified_at, o.details
               FROM decisions d
               LEFT JOIN outcomes o ON d.id = o.decision_id
               ORDER BY d.timestamp ASC"#,
        )?;

        let rows = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let ts_str: String = row.get(1)?;
            let action_str: String = row.get(9)?;
            let risk_str: String = row.get(10)?;
            let latency_i64: i64 = row.get(11)?;

            let dec = DecisionRecord {
                id: DecisionId::new(&id_str).unwrap(),
                timestamp: DateTime::parse_from_rfc3339(&ts_str)
                    .unwrap()
                    .with_timezone(&Utc),
                provider: row.get(2)?,
                decision_type: row.get(3)?,
                context: row.get(4)?,
                task_id: row.get(5)?,
                selected: row.get(6)?,
                confidence: row.get(7)?,
                probabilities_json: row.get(8)?,
                action: serde_json::from_str(&action_str).unwrap_or(ReflexAction::Accept),
                risk_level: RiskLevel::from_str(&risk_str).unwrap_or_default(),
                latency_ms: latency_i64 as u64,
                cost_estimate: row.get(12)?,
            };

            let out_res: Option<String> = row.get(13)?;
            let outcome = match out_res {
                Some(res_str) => {
                    let src_str: String = row.get(14)?;
                    let vts_str: String = row.get(15)?;
                    let details: Option<String> = row.get(16)?;
                    Some(OutcomeRecord {
                        decision_id: DecisionId::new(id_str).unwrap(),
                        result: serde_json::from_str(&res_str).unwrap_or(Outcome::Unknown),
                        source: serde_json::from_str(&src_str).unwrap_or(OutcomeSource::Runtime),
                        verified_at: DateTime::parse_from_rfc3339(&vts_str)
                            .unwrap()
                            .with_timezone(&Utc),
                        details,
                    })
                }
                None => None,
            };

            Ok((dec, outcome))
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn list_shadow_records(&self, limit: usize) -> Result<Vec<ShadowRecord>, TelemetryError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| TelemetryError::LockError(e.to_string()))?;
        let mut stmt = conn.prepare(
            r#"SELECT id, task_id, timestamp, predicted_action, confidence,
                      actual_action, verifier_result, ci_outcome, final_outcome, latency_ms, cost_estimate
               FROM shadow_records ORDER BY timestamp DESC LIMIT ?1"#,
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            let id: String = row.get(0)?;
            let task_id: Option<String> = row.get(1)?;
            let ts_str: String = row.get(2)?;
            let pred_str: String = row.get(3)?;
            let conf: f64 = row.get(4)?;
            let act: String = row.get(5)?;
            let verifier_result: Option<String> = row.get(6)?;
            let ci_str: Option<String> = row.get(7)?;
            let out_str: Option<String> = row.get(8)?;
            let latency_i64: i64 = row.get(9)?;
            let cost: f64 = row.get(10)?;

            let ci_outcome: Option<Outcome> = ci_str.and_then(|s| serde_json::from_str(&s).ok());
            let final_outcome = out_str
                .and_then(|s| serde_json::from_str(&s).ok())
                .or(ci_outcome);

            Ok(ShadowRecord {
                id,
                task_id,
                timestamp: DateTime::parse_from_rfc3339(&ts_str)
                    .unwrap()
                    .with_timezone(&Utc),
                predicted_action: serde_json::from_str(&pred_str).unwrap_or(ReflexAction::Accept),
                confidence: conf,
                actual_action: act,
                verifier_result,
                ci_outcome,
                final_outcome,
                latency_ms: latency_i64 as u64,
                cost_estimate: cost,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_telemetry_roundtrip() {
        let store = TelemetryStore::open_in_memory().unwrap();
        let id = DecisionId::generate();

        let dec = DecisionRecord {
            id: id.clone(),
            timestamp: Utc::now(),
            provider: "mock".to_string(),
            decision_type: "probability".to_string(),
            context: "verify test".to_string(),
            task_id: Some("task-1".to_string()),
            selected: "true".to_string(),
            confidence: 0.94,
            probabilities_json: "[]".to_string(),
            action: ReflexAction::Accept,
            risk_level: RiskLevel::Low,
            latency_ms: 8,
            cost_estimate: 0.0001,
        };

        store.record_decision(&dec).unwrap();

        let fetched = store
            .get_decision(&id)
            .unwrap()
            .expect("should find decision");
        assert_eq!(fetched.confidence, 0.94);
        assert_eq!(fetched.selected, "true");

        let out = OutcomeRecord {
            decision_id: id.clone(),
            result: Outcome::Success,
            source: OutcomeSource::Tests,
            verified_at: Utc::now(),
            details: Some("All tests passed".to_string()),
        };
        store.record_outcome(&out).unwrap();

        let fetched_out = store
            .get_outcome(&id)
            .unwrap()
            .expect("should find outcome");
        assert_eq!(fetched_out.result, Outcome::Success);

        let pairs = store.list_paired_decisions().unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].1.as_ref().unwrap().result, Outcome::Success);
    }
}
