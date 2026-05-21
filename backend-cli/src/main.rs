use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::io::Write;
use upgrade_core::{emit_bootstrap, run_command, AppContext, Command as CoreCommand, DeferPeriod as CoreDeferPeriod, Event};

#[derive(Parser, Debug)]
#[command(
    name = "debian-upgrade-cli",
    bin_name = "debian-upgrade-cli",
    about = "Assistant CLI pour préparation de mise à niveau majeure Debian"
)]
struct Cli {
    #[arg(long, global = true, help = "Simule les actions sans modifier le système")]
    dry_run: bool,

    #[arg(long, global = true, help = "Active des logs de debug plus verbeux")]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    CheckNewRelease,
    CheckSources,
    DisableThirdParty,
    PreparePackages,
    DryRunUpgrade,
    ScheduleOfflineUpgrade,
    RunAll,
    Defer {
        #[arg(value_enum)]
        period: DeferPeriod,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DeferPeriod {
    Day,
    Week,
    Month,
}

impl From<DeferPeriod> for CoreDeferPeriod {
    // Convertit la période CLI vers le type partagé du coeur backend.
    fn from(value: DeferPeriod) -> Self {
        match value {
            DeferPeriod::Day => Self::Day,
            DeferPeriod::Week => Self::Week,
            DeferPeriod::Month => Self::Month,
        }
    }
}

// Emet un événement JSON ligne par ligne sur stdout pour la GUI.
fn emit_json_stdout(event: Event) -> Result<()> {
    println!("{}", serde_json::to_string(&event)?);
    std::io::stdout().flush()?;
    Ok(())
}

// Point d'entrée CLI: parse les options et délègue l'exécution à upgrade-core.
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();
    let ctx = AppContext {
        dry_run: cli.dry_run,
        debug: cli.debug,
    };

    let mut sink = |evt: Event| emit_json_stdout(evt);
    emit_bootstrap(ctx, &mut sink)?;

    match cli.command {
        Some(Commands::CheckNewRelease) => run_command(ctx, CoreCommand::CheckNewRelease, &mut sink),
        Some(Commands::CheckSources) => run_command(ctx, CoreCommand::CheckSources, &mut sink),
        Some(Commands::DisableThirdParty) => run_command(ctx, CoreCommand::DisableThirdParty, &mut sink),
        Some(Commands::PreparePackages) => run_command(ctx, CoreCommand::PreparePackages, &mut sink),
        Some(Commands::DryRunUpgrade) => run_command(ctx, CoreCommand::DryRunUpgrade, &mut sink),
        Some(Commands::ScheduleOfflineUpgrade) => run_command(ctx, CoreCommand::ScheduleOfflineUpgrade, &mut sink),
        Some(Commands::RunAll) => run_command(ctx, CoreCommand::RunAll, &mut sink),
        Some(Commands::Defer { period }) => run_command(
            ctx,
            CoreCommand::Defer {
                period: period.into(),
            },
            &mut sink,
        ),
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}
