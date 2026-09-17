mod commands;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "reflex",
    author,
    version,
    about = "A calibrated System-1 control plane for AI agents and swarms"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize reflex-control configuration and database
    Init,

    /// Execute a decision evaluation through provider and policy
    Run(RunArgs),

    /// Shadow mode commands
    Shadow {
        #[command(subcommand)]
        sub: ShadowSubcommands,
    },

    /// Inspect a specific decision and its outcome
    Inspect {
        /// Decision ID to inspect
        id: String,

        #[arg(long, default_value = "reflex.db")]
        db: String,
    },

    /// Generate aggregate telemetry and economics report
    Report {
        #[arg(long, default_value = "reflex.db")]
        db: String,
    },

    /// Calculate calibration metrics and optimize policy thresholds
    Calibrate(CalibrateArgs),

    /// Generate Cost vs Risk Pareto Frontier analysis
    Pareto(ParetoArgs),

    /// Run benchmark comparing Reflex vs frontier/small models
    Benchmark {
        #[arg(long, default_value_t = 1000)]
        tasks: usize,
    },

    /// Run interactive demos
    Demo {
        #[command(subcommand)]
        sub: DemoSubcommands,
    },
}

#[derive(Args, Debug)]
struct RunArgs {
    #[arg(short, long, default_value = "mock")]
    provider: String,

    #[arg(short, long)]
    context: String,

    #[arg(long)]
    task_id: Option<String>,

    #[arg(short, long, default_value = "low")]
    risk: String,

    #[arg(long, value_delimiter = ',')]
    options: Option<Vec<String>>,

    #[arg(long, default_value = "reflex.db")]
    db: String,
}

#[derive(Subcommand, Debug)]
enum ShadowSubcommands {
    /// Run shadow mode on verified agent task dataset or simulation
    Run {
        /// Path to verified agent tasks JSON dataset
        #[arg(short, long)]
        dataset: Option<String>,

        #[arg(long, default_value_t = 120)]
        count: usize,

        #[arg(long, default_value = "reflex.db")]
        db: String,
    },

    /// Generate shadow mode agreement and performance report
    Report {
        #[arg(long, default_value = "reflex.db")]
        db: String,
    },
}

#[derive(Args, Debug)]
struct CalibrateArgs {
    /// Path to verified agent tasks JSON dataset
    #[arg(short, long)]
    dataset: Option<String>,

    #[arg(long, default_value_t = 0.01)]
    max_false_accept: f64,

    #[arg(long, default_value_t = 0.01)]
    max_false_negative: f64,

    #[arg(long, default_value_t = 0.50)]
    min_coverage: f64,

    #[arg(long, default_value_t = 0.90)]
    threshold: f64,

    #[arg(long, default_value = "reflex.db")]
    db: String,
}

#[derive(Args, Debug)]
struct ParetoArgs {
    /// Path to verified agent tasks JSON dataset
    #[arg(short, long)]
    dataset: Option<String>,

    #[arg(long, default_value = "reflex.db")]
    db: String,
}

#[derive(Subcommand, Debug)]
enum DemoSubcommands {
    /// Run the primary Verifier Gate demo
    VerifierGate {
        #[arg(long, default_value = "reflex.db")]
        db: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            commands::init::execute()?;
        }
        Commands::Run(args) => {
            commands::run::execute(
                args.provider,
                args.context,
                args.task_id,
                args.risk,
                args.options,
                args.db,
            )
            .await?;
        }
        Commands::Shadow { sub } => match sub {
            ShadowSubcommands::Run { dataset, count, db } => {
                commands::shadow::run_shadow(dataset, count, db).await?;
            }
            ShadowSubcommands::Report { db } => {
                commands::shadow::report(db)?;
            }
        },
        Commands::Inspect { id, db } => {
            commands::inspect::execute(id, db)?;
        }
        Commands::Report { db } => {
            commands::report::execute(db)?;
        }
        Commands::Calibrate(args) => {
            commands::calibrate::execute(
                args.dataset,
                args.max_false_accept,
                args.max_false_negative,
                args.min_coverage,
                args.threshold,
                args.db,
            )?;
        }
        Commands::Pareto(args) => {
            commands::pareto::execute(args.dataset, args.db)?;
        }
        Commands::Benchmark { tasks } => {
            commands::benchmark::execute(tasks)?;
        }
        Commands::Demo { sub } => match sub {
            DemoSubcommands::VerifierGate { db } => {
                commands::demo::execute_verifier_gate(db).await?;
            }
        },
    }

    Ok(())
}
