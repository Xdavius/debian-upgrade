use std::thread;
use std::time::Duration;
use std::{fs, path::Path};
use std::io::Write;
use std::process::{Command, Stdio};

use upgrade_core::{
    check_new_major_release, run_command, AppContext, Command as CoreCommand, Event as CoreEvent,
    LogLevel, StepState,
};

slint::include_modules!();

struct UiEvent {
    level: String,
    step: String,
    state: String,
    message: String,
}

#[derive(Clone, Copy)]
struct RunMode {
    debug: bool,
}

fn parse_run_mode() -> RunMode {
    let debug = std::env::args().any(|arg| arg == "--debug");
    RunMode { debug }
}

fn run_with_pkexec(script: &str) -> anyhow::Result<()> {
    let status = Command::new("pkexec")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .status()
        .map_err(|e| anyhow::anyhow!("pkexec indisponible ou echec lancement: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("pkexec a echoue: {status}"))
    }
}

fn run_with_zenity_fallback(script: &str) -> anyhow::Result<()> {
    let output = Command::new("zenity")
        .arg("--password")
        .arg("--title=Authentification requise")
        .output()
        .map_err(|e| anyhow::anyhow!("zenity indisponible: {e}"))?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("zenity annule ou en echec"));
    }

    let mut password = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("mot de passe zenity invalide UTF-8: {e}"))?;
    while password.ends_with('\n') || password.ends_with('\r') {
        password.pop();
    }

    let mut child = Command::new("sudo")
        .arg("-S")
        .arg("/bin/sh")
        .arg("-c")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("sudo fallback impossible a lancer: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(format!("{password}\n").as_bytes());
    }
    let status = child
        .wait()
        .map_err(|e| anyhow::anyhow!("sudo fallback echec attente: {e}"))?;

    // Effacement explicite du champ mot de passe en memoire.
    password.replace_range(.., &"\0".repeat(password.len()));
    password.clear();

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("sudo fallback a echoue: {status}"))
    }
}

fn setup_offline_upgrade_and_reboot() -> anyhow::Result<()> {
    let setup_script = r#"set -euo pipefail

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
ln -snf /usr/lib/systemd/system/debian-upgrade-offline.service /etc/systemd/system/system-update.target.wants/debian-upgrade-offline.service
ln -snf /var/lib/system-update /system-update
systemctl daemon-reload
"#;
    if let Err(pk_err) = run_with_pkexec(setup_script) {
        run_with_zenity_fallback(&setup_script)
            .map_err(|zen_err| anyhow::anyhow!("echec elevation privilegies (pkexec puis zenity): {pk_err}; {zen_err}"))?;
    }

    let reboot_script = "systemctl reboot";
    if let Err(pk_err) = run_with_pkexec(reboot_script) {
        run_with_zenity_fallback(reboot_script)
            .map_err(|zen_err| anyhow::anyhow!("echec reboot privilegie (pkexec puis zenity): {pk_err}; {zen_err}"))?;
    }
    Ok(())
}

fn append_log(app: &AppWindow, line: &str) {
    let current = app.get_logs_text().to_string();
    let updated = if current.is_empty() {
        line.to_string()
    } else {
        format!("{current}\n{line}")
    };
    let end_offset = updated.chars().count() as i32;
    app.set_logs_text(updated.into());
    app.invoke_scroll_logs_to_end(end_offset);
}

fn level_to_str(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Debug => "debug",
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
        LogLevel::Success => "success",
    }
}

fn state_to_str(state: StepState) -> &'static str {
    match state {
        StepState::Pending => "pending",
        StepState::Running => "running",
        StepState::Done => "done",
        StepState::Failed => "failed",
    }
}

fn core_to_ui_event(event: CoreEvent) -> UiEvent {
    UiEvent {
        level: level_to_str(event.level).to_string(),
        step: event.step.to_string(),
        state: state_to_str(event.state).to_string(),
        message: event.message,
    }
}

fn apply_ui_event(app: &AppWindow, evt: UiEvent) {
    append_log(app, &format!("[{}] {} ({}, {})", evt.level, evt.message, evt.step, evt.state));
}

fn detect_third_party_candidates() -> Vec<String> {
    let dir = Path::new("/etc/apt/sources.list.d");
    let mut files = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return files,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(ext) => ext,
            None => continue,
        };
        if ext == "list" || ext == "sources" {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "debian.sources" {
                    continue;
                }
                files.push(name.to_string());
            }
        }
    }

    files.sort();
    files
}

fn main() -> Result<(), slint::PlatformError> {
    let mode = parse_run_mode();
    let app = AppWindow::new()?;
    let window = app.window();
    // Centered startup position (desktop standard 1920x1080 baseline).
    // Slint 1.5 does not expose monitor geometry directly in this API surface.
    let size = window.size();
    let x = ((1920 - size.width as i32) / 2).max(0);
    let y = ((1080 - size.height as i32) / 2).max(0);
    window.set_position(slint::PhysicalPosition::new(x, y));

    let third_party = detect_third_party_candidates();
    app.set_third_party_1_visible(false);
    app.set_third_party_2_visible(false);
    app.set_third_party_3_visible(false);
    app.set_third_party_1_enabled(false);
    app.set_third_party_2_enabled(false);
    app.set_third_party_3_enabled(false);

    if let Some(name) = third_party.first() {
        app.set_third_party_1_name(name.as_str().into());
        app.set_third_party_1_visible(true);
    }
    if let Some(name) = third_party.get(1) {
        app.set_third_party_2_name(name.as_str().into());
        app.set_third_party_2_visible(true);
    }
    if let Some(name) = third_party.get(2) {
        app.set_third_party_3_name(name.as_str().into());
        app.set_third_party_3_visible(true);
    }

    if third_party.len() > 3 {
        append_log(
            &app,
            &format!(
                "[info] {} depots detectes (.list/.sources), affichage limite aux 3 premiers.",
                third_party.len()
            ),
        );
    } else if third_party.is_empty() {
        append_log(&app, "[info] Aucun depot tiers detecte dans /etc/apt/sources.list.d.");
    }
    if mode.debug {
        append_log(&app, "[debug] Mode debug actif.");
    } else {
        append_log(&app, "[info] Mode normal actif.");
    }

    {
        let weak = app.as_weak();
        app.on_go_next(move || {
            if let Some(app) = weak.upgrade() {
                let p = app.get_current_page();
                if p < 6 {
                    app.set_current_page(p + 1);
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_go_prev(move || {
            if let Some(app) = weak.upgrade() {
                let p = app.get_current_page();
                if p > 1 {
                    app.set_current_page(p - 1);
                }
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_check_new_release(move || {
            let ui = weak.clone();
            let mode = mode;

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_header_status("Verification version Debian en ligne...".into());
                        append_log(&app, "[info] Verification de la nouvelle version majeure Debian...");
                    }
                }
            });

            thread::spawn(move || {
                let ctx = AppContext {
                    dry_run: true,
                    debug: mode.debug,
                };

                let mut publish = |evt: CoreEvent| {
                    let ui_evt = core_to_ui_event(evt);
                    let ui_clone = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui_clone.upgrade() {
                            apply_ui_event(&app, ui_evt);
                        }
                    });
                    Ok(())
                };

                let result = check_new_major_release(ctx, &mut publish);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ui.upgrade() {
                        match result {
                            Ok(info) => {
                                if info.update_available {
                                    append_log(
                                        &app,
                                        &format!(
                                            "[success] Mise a niveau disponible: {} -> {} ({})",
                                            info.current_major, info.stable_major, info.stable_codename
                                        ),
                                    );
                                    app.set_header_status("Nouvelle version detectee".into());
                                    app.set_current_page(2);
                                } else if mode.debug {
                                    append_log(
                                        &app,
                                        "[warn] Aucune nouvelle version detectee, mais mode debug actif: poursuite autorisee.",
                                    );
                                    app.set_header_status("Mode debug: poursuite test".into());
                                    app.set_current_page(2);
                                } else {
                                    append_log(
                                        &app,
                                        &format!(
                                            "[warn] Pas de nouvelle version: stable actuelle {} ({})",
                                            info.stable_major, info.stable_codename
                                        ),
                                    );
                                    app.set_header_status("Aucune nouvelle version".into());
                                    app.set_current_page(6);
                                }
                            }
                            Err(err) => {
                                append_log(&app, &format!("[error] Echec verification en ligne: {err}"));
                                app.set_header_status("Erreur verification version".into());
                                app.set_current_page(6);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_validate_sources(move || {
            if let Some(app) = weak.upgrade() {
                app.set_header_status("Validation sources APT".into());

                let mut enabled = Vec::new();
                if app.get_third_party_1_visible() && app.get_third_party_1_enabled() {
                    enabled.push(app.get_third_party_1_name().to_string());
                }
                if app.get_third_party_2_visible() && app.get_third_party_2_enabled() {
                    enabled.push(app.get_third_party_2_name().to_string());
                }
                if app.get_third_party_3_visible() && app.get_third_party_3_enabled() {
                    enabled.push(app.get_third_party_3_name().to_string());
                }

                if enabled.is_empty() {
                    append_log(&app, "[info] Aucun depot tiers re-active. Tous les depots tiers restent desactives.");
                } else {
                    append_log(&app, &format!("[warn] Depots tiers re-actives manuellement: {}", enabled.join(", ")));
                }

                app.set_current_page(3);
                app.set_header_status("Sources validees".into());
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_run_download_step(move || {
            let ui = weak.clone();
            let mode = mode;

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_header_status("Telechargement des paquets en cours".into());
                        append_log(&app, "[info] Demarrage preparation paquets via upgrade-core...");
                    }
                }
            });

            thread::spawn(move || {
                let ctx = AppContext {
                    dry_run: true,
                    debug: mode.debug,
                };

                let mut publish = |evt: CoreEvent| {
                    let ui_evt = core_to_ui_event(evt);
                    let ui_clone = ui.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui_clone.upgrade() {
                            apply_ui_event(&app, ui_evt);
                        }
                    });
                    Ok(())
                };

                let result = run_command(ctx, CoreCommand::PreparePackages, &mut publish);
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ui.upgrade() {
                        match result {
                            Ok(()) => {
                                append_log(&app, "[success] Preparation des paquets terminee.");
                                app.set_header_status("Preparation paquets terminee".into());
                                app.set_current_page(4);
                            }
                            Err(err) => {
                                append_log(&app, &format!("[error] Echec preparation paquets: {err}"));
                                app.set_header_status("Erreur preparation paquets".into());
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_run_dry_run_upgrade(move || {
            let ui = weak.clone();

            let _ = slint::invoke_from_event_loop({
                let ui = ui.clone();
                move || {
                    if let Some(app) = ui.upgrade() {
                        app.set_header_status("Test dry-run upgrade en cours".into());
                        append_log(&app, "[info] Simulation du test de mise a niveau...");
                        append_log(&app, "[debug] Commande cible future: apt-get -s dist-upgrade");
                    }
                }
            });

            thread::spawn(move || {
                thread::sleep(Duration::from_millis(900));

                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(app) = ui.upgrade() {
                        append_log(&app, "[success] Dry-run upgrade valide: aucune erreur bloquante detectee.");
                        app.set_header_status("Dry-run valide".into());
                        app.set_current_page(5);
                    }
                });
            });
        });
    }

    {
        let weak = app.as_weak();
        app.on_request_reboot(move || {
            if let Some(app) = weak.upgrade() {
                if mode.debug {
                    app.set_header_status("Pret au redemarrage (debug)".into());
                    append_log(&app, "[warn] Redemarrage demande (debug): aucune action systeme reelle executee.");
                    append_log(&app, "[info] Execution cible: mode non interactif avec options par defaut (DEBIAN_FRONTEND=noninteractive).");
                    return;
                }

                app.set_header_status("Armemement upgrade hors-ligne...".into());
                append_log(&app, "[info] Configuration du mode upgrade hors-ligne (system-update.target + script non interactif)...");

                let ui = app.as_weak();
                thread::spawn(move || {
                    let result = setup_offline_upgrade_and_reboot();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(app) = ui.upgrade() {
                            match result {
                                Ok(()) => {
                                    append_log(&app, "[success] Upgrade hors-ligne arme. Redemarrage en cours...");
                                }
                                Err(err) => {
                                    app.set_header_status("Erreur armer/reboot".into());
                                    append_log(&app, &format!("[error] {err}"));
                                }
                            }
                        }
                    });
                });
            }
        });
    }

    {
        let weak = app.as_weak();
        app.on_close_app(move || {
            if let Some(app) = weak.upgrade() {
                let _ = app.window().hide();
            }
        });
    }

    app.run()
}
