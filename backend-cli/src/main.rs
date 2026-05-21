use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use std::io::BufRead;
use std::io::Write;
use std::process::Command as ProcessCommand;
use upgrade_core::{emit_bootstrap, run_command, AppContext, Command as CoreCommand, DeferPeriod as CoreDeferPeriod, Event};

const AGENT_DONE_PREFIX: &str = "__AGENT_DONE__";

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
    Agent,
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

// Parse une commande texte simple pour le mode agent.
fn parse_agent_command(line: &str) -> Option<CoreCommand> {
    match line.trim() {
        "check-new-release" => Some(CoreCommand::CheckNewRelease),
        "check-sources" => Some(CoreCommand::CheckSources),
        "disable-third-party" => Some(CoreCommand::DisableThirdParty),
        "prepare-packages" => Some(CoreCommand::PreparePackages),
        "dry-run-upgrade" => Some(CoreCommand::DryRunUpgrade),
        "schedule-offline-upgrade" => Some(CoreCommand::ScheduleOfflineUpgrade),
        "run-all" => Some(CoreCommand::RunAll),
        _ => None,
    }
}

// Exécute l'armement offline + reboot directement depuis le contexte root agent.
fn run_agent_arm_and_reboot() -> Result<()> {
    let script = r#"set -euo pipefail
if [ ! -x /usr/local/lib/debian-upgrade/offline-upgrade.sh ]; then
  echo "Script manquant: /usr/local/lib/debian-upgrade/offline-upgrade.sh"
  exit 1
fi
if [ ! -f /usr/lib/systemd/system/debian-upgrade-offline.service ]; then
  echo "Service manquant: /usr/lib/systemd/system/debian-upgrade-offline.service"
  exit 1
fi
install -d -m 0755 /var/lib/system-update
install -d -m 0755 /etc/systemd/system/system-update.target.wants
ln -snf /usr/lib/systemd/system/debian-upgrade-offline.service \
  /etc/systemd/system/system-update.target.wants/debian-upgrade-offline.service
ln -snf /var/lib/system-update /system-update
systemctl daemon-reload
systemctl reboot
"#;
    let status = ProcessCommand::new("/bin/bash").arg("-lc").arg(script).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("arm-and-reboot echec: {status}"))
    }
}

// Boucle agent: lit stdin ligne par ligne, exécute et stream les événements JSON.
fn run_agent_loop(ctx: AppContext, sink: &mut dyn FnMut(Event) -> Result<()>) -> Result<()> {
    let stdin = std::io::stdin();
    let mut reader = std::io::BufReader::new(stdin.lock());
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let cmd = line.trim();
        if cmd.is_empty() {
            continue;
        }
        if cmd == "quit" || cmd == "exit" {
            break;
        }
        let result = if cmd == "arm-and-reboot" {
            run_agent_arm_and_reboot()
        } else if let Some(core) = parse_agent_command(cmd) {
            run_command(ctx, core, sink)
        } else if let Some(period) = cmd.strip_prefix("defer ") {
            let period = match period {
                "day" => Some(CoreDeferPeriod::Day),
                "week" => Some(CoreDeferPeriod::Week),
                "month" => Some(CoreDeferPeriod::Month),
                _ => None,
            };
            if let Some(period) = period {
                run_command(ctx, CoreCommand::Defer { period }, sink)
            } else {
                Err(anyhow::anyhow!("commande agent inconnue: {cmd}"))
            }
        } else {
            Err(anyhow::anyhow!("commande agent inconnue: {cmd}"))
        };

        match result {
            Ok(()) => {
                println!("{AGENT_DONE_PREFIX}|ok|{cmd}");
                std::io::stdout().flush()?;
            }
            Err(err) => {
                // On garde l'agent vivant et on renvoie un marqueur d'echec explicite.
                println!("{}", serde_json::to_string(&Event {
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    level: upgrade_core::LogLevel::Error,
                    step: "agent",
                    state: upgrade_core::StepState::Failed,
                    message: format!("Echec commande agent: {err}"),
                })?);
                println!("{AGENT_DONE_PREFIX}|err|{cmd}");
                std::io::stdout().flush()?;
            }
        }
    }
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
        Some(Commands::Agent) => run_agent_loop(ctx, &mut sink),
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    }
}
