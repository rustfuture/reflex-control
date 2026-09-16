use reflex_core::DecisionId;
use reflex_telemetry::TelemetryStore;

pub fn execute(decision_id_str: String, db_path: String) -> Result<(), Box<dyn std::error::Error>> {
    let store = TelemetryStore::open(&db_path)?;
    let id = DecisionId::new(&decision_id_str)?;

    let maybe_dec = store.get_decision(&id)?;
    let dec = match maybe_dec {
        Some(d) => d,
        None => {
            eprintln!("Decision not found: {decision_id_str}");
            return Ok(());
        }
    };

    let outcome = store.get_outcome(&id)?;

    println!("Decision");
    println!("──────────────────────────────────────────────");
    println!("ID:           {}", dec.id);
    println!("Timestamp:    {}", dec.timestamp.to_rfc3339());
    println!("Provider:     {}", dec.provider);
    println!("Type:         {}", dec.decision_type);
    println!("Context:      {}", dec.context);
    if let Some(ref tid) = dec.task_id {
        println!("Task ID:      {}", tid);
    }
    println!("Selected:     {}", dec.selected);
    println!("Confidence:   {:.4}", dec.confidence);
    println!("Risk Level:   {}", dec.risk_level);
    println!("Action:       {}", dec.action);
    println!("Latency:      {} ms", dec.latency_ms);
    println!("Cost:         ${:.6}", dec.cost_estimate);

    println!("\nOutcome");
    println!("──────────────────────────────────────────────");
    if let Some(out) = outcome {
        println!("Result:       {}", out.result);
        println!("Source:       {}", out.source);
        println!("Verified At:  {}", out.verified_at.to_rfc3339());
        if let Some(details) = out.details {
            println!("Details:      {}", details);
        }

        let is_accurate = match out.result {
            reflex_core::Outcome::Success => dec.action.is_accept(),
            reflex_core::Outcome::Failure => !dec.action.is_accept(),
            _ => false,
        };
        println!(
            "Evaluation:   {}",
            if is_accurate {
                "Correct"
            } else {
                "Incorrect (Mismatched Outcome)"
            }
        );
    } else {
        println!("Status:       Pending / Unverified");
    }

    Ok(())
}
