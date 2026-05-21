# Debian Major Upgrade Assistant

Outil de préparation et d'accompagnement pour les mises à niveau majeures Debian.

Le projet est composé de deux briques:

- Un backend CLI qui vérifie les prérequis, prépare la machine, et orchestre la mise à niveau.
- Une interface GUI (Rust + Slint) qui guide l'utilisateur étape par étape.

## Objectif

Rendre la montée de version majeure Debian plus fiable, plus lisible et plus sûre, tout en gardant une expérience utilisateur simple.

## Structure initiale

- `context.md`: document de référence partagé (vision, architecture, roadmap, suivi).
- `backend-cli/`: binaire CLI backend (adaptateur d'exécution).
- `upgrade-core/`: librairie cœur (logique métier, étapes, événements).
- `frontend-gui/`: interface graphique Slint.
- `docs/`: documents d'architecture, procédures et décisions.
- `build.sh`: script de build/test local générant un bundle test dans `buildtest/`.

## Démarrage rapide

```bash
cargo run -p backend-cli --bin debian-upgrade-backend -- --dry-run --debug run-all
cargo run -p backend-cli --bin debian-upgrade-backend -- --dry-run defer week
./build.sh
```

## Dry-run GUI + CLI

1. Compiler le backend:

```bash
cargo build -p backend-cli --bin debian-upgrade-backend
```

2. Lancer la GUI:

```bash
cargo run -p frontend-gui --bin debian-upgrade
```

3. Dans la GUI, cliquer sur `Dry-run integre` (recommande).
4. Option de secours: `Dry-run process`.


## GUI mode debug

- Mode normal (par defaut):

```bash
cargo run -p frontend-gui --bin debian-upgrade
```

- Mode debug (bypass test):

```bash
cargo run -p frontend-gui --bin debian-upgrade -- --debug
```

## Notification automatique (service systemd root)

Le package installe un timer `systemd` (system-wide, execute en root) qui verifie periodiquement la disponibilite d'une nouvelle version majeure Debian.

- Service: `debian-upgrade-notify.service`
- Timer: `debian-upgrade-notify.timer`
- Script: `/usr/local/lib/debian-upgrade/check-upgrade-notify.sh`

Comportement:

1. Verifie internet et lance `debian-upgrade-backend check-new-release`.
2. Si une nouvelle majeure est detectee, envoie une notification interactive aux sessions graphiques actives:
   - utilisateur `root`,
   - utilisateurs membres du groupe `sudo`.
3. Notification:
   - app name `Debian-Upgrade`,
   - icone `system-software-update`,
   - urgence `critical`,
   - expiration `0` (persistante tant qu'aucune action n'est prise).
4. Actions directement dans la notification:
   - lancer la GUI,
   - reporter 1 jour / 1 semaine / 1 mois.
5. Le report est memorise par utilisateur cible dans `/var/lib/debian-upgrade/notify/`.

Ancien comportement retire:

- plus de popup `zenity`,
- plus de timer `systemd --user`.
