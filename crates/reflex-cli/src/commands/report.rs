use reflex_telemetry::TelemetryStore;
use std::collections::HashMap;

pub fn execute(db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let pairs = store.list_paired_decisions()?;

    if pairs.is_empty() {
        println!("No telemetry decisions found in {}", db_path);
        println!("Run some decisions first with: reflex run or reflex demo verifier-gate");
        return Ok(());
    }

    let total = pairs.len();
    let mut actions_count = HashMap::new();
    let mut providers_count = HashMap::new();
    let mut verified_count = 0;
    let mut success_count = 0;
    let mut failure_count = 0;
    let mut false_accepts = 0;
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

        if let Some(o) = out {
            verified_count += 1;
            if o.result.is_success() {
                success_count += 1;
            } else if o.result.is_failure() {
                failure_count += 1;
                if dec.action.is_accept() {
                    false_accepts += 1;
                }
            }
        }
    }

    let avg_latency = total_latency as f64 / total as f64;
    let baseline_frontier_cost = total as f64 * 0.02; // Frontier call baseline (~$0.02)
    let cost_reduction = if baseline_frontier_cost > 0.0 {
        ((baseline_frontier_cost - total_cost) / baseline_frontier_cost) * 100.0
    } else {
        0.0
    };

    println!("================ Reflex Control Telemetry Report ================");
    println!("Total Decisions:         {}", total);
    println!("Average Decision Latency: {:.1} ms", avg_latency);
    println!("Total Estimated Cost:    ${:.4}", total_cost);
    println!("Baseline Frontier Cost:  ${:.4}", baseline_frontier_cost);
    println!("Projected Cost Savings:  {:.1}%", cost_reduction.max(0.0));

    println!("\n--- Actions Breakdown ---");
    for (action, cnt) in &actions_count {
        let pct = (*cnt as f64 / total as f64) * 100.0;
        println!("  {:14} : {:5} ({:5.1}%)", action, cnt, pct);
    }

    println!("\n--- Provider Breakdown ---");
    for (prov, cnt) in &providers_count {
        let pct = (*cnt as f64 / total as f64) * 100.0;
        println!("  {:14} : {:5} ({:5.1}%)", prov, cnt, pct);
    }

    println!("\n--- Outcome Verification ---");
    println!("Total Verified:          {}", verified_count);
    if verified_count > 0 {
        println!("  Success:               {}", success_count);
        println!("  Failure:               {}", failure_count);
        println!("  False Accepts:         {}", false_accepts);
        let far = (false_accepts as f64 / verified_count as f64) * 100.0;
        println!("  False Accept Rate:     {:.2}%", far);
    } else {
        println!("  No outcomes linked yet. Outcomes are recorded via testing, CI, or verifiers.");
    }
    println!("=================================================================");

    Ok(())
}
