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

    /// Run benchmark comparing Reflex vs frontier/small models or live Jev evaluation
    Benchmark(BenchmarkArgs),

    /// Run empirical 4-way evaluation on fresh datasets across sequential phases
    Experiment(commands::experiment::ExperimentArgs),

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
    /// Run shadow mode on an evaluation dataset or simulation
    Run {
        /// Path to an evaluation task JSON dataset
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
    /// Path to an evaluation task JSON dataset
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

    /// Stratify dataset into Train 50% / Val 25% / Held-Out Test 25%
    #[arg(long, default_value_t = true)]
    split: bool,

    #[arg(long, default_value = "reflex.db")]
    db: String,
}

#[derive(Args, Debug)]
struct ParetoArgs {
    /// Path to an evaluation task JSON dataset
    #[arg(short, long)]
    dataset: Option<String>,

    #[arg(long, default_value = "reflex.db")]
    db: String,
}

#[derive(Args, Debug)]
struct BenchmarkArgs {
    /// Provider to benchmark: 'jev' (Live API) or 'mock' (Synthetic)
    #[arg(short, long, default_value = "mock")]
    provider: String,

    /// Path to benchmark dataset JSON
    #[arg(short, long)]
    dataset: Option<String>,

    /// Maximum number of tasks to evaluate (0 = all)
    #[arg(long, default_value_t = 0)]
    tasks: usize,

    /// Formulation variant: 'a', 'b', 'frozen', or 'compare'
    #[arg(long, default_value = "a")]
    formulation: String,

    /// Custom verification threshold override
    #[arg(long)]
    threshold: Option<f64>,

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
                args.split,
                args.db,
            )?;
        }
        Commands::Pareto(args) => {
            commands::pareto::execute(args.dataset, args.db)?;
        }
        Commands::Benchmark(args) => {
            commands::benchmark::execute(
                args.provider,
                args.dataset,
                args.tasks,
                args.formulation,
                args.threshold,
                args.db,
            )
            .await?;
        }
        Commands::Experiment(args) => {
            commands::experiment::execute(args).await?;
        }
        Commands::Demo { sub } => match sub {
            DemoSubcommands::VerifierGate { db } => {
                commands::demo::execute_verifier_gate(db).await?;
            }
        },
    }

    Ok(())
}
