use reflex_telemetry::TelemetryStore;
use std::fs;
use std::path::Path;

const DEFAULT_CONFIG: &str = r#"# reflex-control configuration

[telemetry]
database_path = "reflex.db"

[provider]
default = "mock" # "mock" or "jev"
# jev_api_key = "env:JEV_API_KEY"

[verification]
accept_threshold = 0.90
escalate_threshold = 0.65

[risks]
critical_always_escalate = true
high_always_verify = true
"#;

pub fn execute() -> Result<(), Box<dyn std::error::Error>> {
    println!("Initializing reflex-control environment...");

    let config_path = Path::new("reflex.toml");
    if !config_path.exists() {
        fs::write(config_path, DEFAULT_CONFIG)?;
        println!("  Created config file: reflex.toml");
    } else {
        println!("  Existing config file found: reflex.toml");
    }

    let db_path = "reflex.db";
    let _store = TelemetryStore::open(db_path)?;
    println!("  Initialized telemetry database: {db_path}");

    println!("\nReflex Control initialized successfully!");
    println!("Try running:");
    println!("  reflex demo verifier-gate");
    println!("  reflex run --context 'Check if worker patch is safe' --risk low");

    Ok(())
}
