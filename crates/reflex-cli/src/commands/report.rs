use reflex_core::{Outcome, ReflexAction};
use reflex_telemetry::TelemetryStore;
use std::collections::HashMap;

#[derive(Default)]
struct OutcomeSummary {
    linked: usize,
    resolved: usize,
    success: usize,
    failure: usize,
    partial: usize,
    unknown: usize,
    false_accepts: usize,
    resolved_autonomous_passes: usize,
}

impl OutcomeSummary {
    fn record(&mut self, action: &ReflexAction, outcome: Option<Outcome>) {
        let Some(outcome) = outcome else { return };
        self.linked += 1;
        match outcome {
            Outcome::Success => self.success += 1,
            Outcome::Failure => {
                self.failure += 1;
                if action.is_autonomous_pass() {
                    self.false_accepts += 1;
                }
            }
            Outcome::Partial => self.partial += 1,
            Outcome::Unknown => self.unknown += 1,
        }
        if outcome.is_resolved() {
            self.resolved += 1;
            if action.is_autonomous_pass() {
                self.resolved_autonomous_passes += 1;
            }
        }
    }
}

pub fn execute(db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let pairs = store.list_paired_decisions()?;

    if pairs.is_empty() {
        println!("No telemetry decisions found in {db_path}");
        println!("Run some decisions first with: reflex run or reflex demo verifier-gate");
        return Ok(());
    }

    let total = pairs.len();
    let mut actions_count = HashMap::new();
    let mut providers_count = HashMap::new();
    let mut outcome_summary = OutcomeSummary::default();
    let mut total_latency = 0u64;
    let mut total_cost = 0.0f64;

    for (dec, out) in &pairs {
        *actions_count
            .entry(dec.action.to_string())
            .or_insert(0usize) += 1;
        *providers_count
            .entry(dec.provider.clone())
            .or_insert(0usize) += 1;
        total_latency += dec.latency_ms;
        total_cost += dec.cost_estimate;

        outcome_summary.record(&dec.action, out.as_ref().map(|record| record.result));
    }

    let avg_latency = total_latency as f64 / total as f64;
    let baseline_frontier_cost = total as f64 * 0.02; // Frontier call baseline (~$0.02)
    let cost_reduction = if baseline_frontier_cost > 0.0 {
        ((baseline_frontier_cost - total_cost) / baseline_frontier_cost) * 100.0
    } else {
        0.0
    };

    println!("================ Reflex Control Telemetry Report ================");
    println!("Total Decisions:         {total}");
    println!("Average Decision Latency: {avg_latency:.1} ms");
    println!("Total Estimated Cost:    ${total_cost:.4}");
    println!("Baseline Frontier Cost:  ${baseline_frontier_cost:.4}");
    println!("Projected Cost Savings:  {:.1}%", cost_reduction.max(0.0));

    println!("\n--- Actions Breakdown ---");
    for (action, cnt) in &actions_count {
        let pct = (*cnt as f64 / total as f64) * 100.0;
        println!("  {action:14} : {cnt:5} ({pct:5.1}%)");
    }

    println!("\n--- Provider Breakdown ---");
    for (prov, cnt) in &providers_count {
        let pct = (*cnt as f64 / total as f64) * 100.0;
        println!("  {prov:14} : {cnt:5} ({pct:5.1}%)");
    }

    println!("\n--- Outcome Verification ---");
    println!("Outcome records linked:  {}", outcome_summary.linked);
    println!("  Resolved (Success/Failure): {}", outcome_summary.resolved);
    println!("    Success:             {}", outcome_summary.success);
    println!("    Failure:             {}", outcome_summary.failure);
    println!("  Partial:               {}", outcome_summary.partial);
    println!("  Unknown:               {}", outcome_summary.unknown);
    println!(
        "  Missing:               {}",
        total - outcome_summary.linked
    );
    if outcome_summary.resolved > 0 {
        println!("  False Accepts:         {}", outcome_summary.false_accepts);
        let far = if outcome_summary.resolved_autonomous_passes > 0 {
            format!(
                "{:.2}% ({}/{})",
                outcome_summary.false_accepts as f64
                    / outcome_summary.resolved_autonomous_passes as f64
                    * 100.0,
                outcome_summary.false_accepts,
                outcome_summary.resolved_autonomous_passes
            )
        } else {
            format!(
                "N/A (no resolved autonomous passes; false accepts = {})",
                outcome_summary.false_accepts
            )
        };
        println!("  False Accept Rate:     {far}");
    } else {
        println!("  False Accept Rate:     N/A (no resolved outcomes)");
        println!("  Outcomes are recorded via testing, CI, or verifiers.");
    }
    println!("=================================================================");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn false_accept_rate_cohort_excludes_partial_unknown_and_missing_outcomes() {
        let mut summary = OutcomeSummary::default();
        summary.record(&ReflexAction::Accept, Some(Outcome::Failure));
        summary.record(&ReflexAction::Accept, Some(Outcome::Success));
        summary.record(&ReflexAction::Accept, Some(Outcome::Partial));
        summary.record(&ReflexAction::Accept, Some(Outcome::Unknown));
        summary.record(&ReflexAction::Accept, None);

        assert_eq!(summary.linked, 4);
        assert_eq!(summary.resolved, 2);
        assert_eq!(summary.false_accepts, 1);
        assert_eq!(summary.resolved_autonomous_passes, 2);
    }
}
