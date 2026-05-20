# backend-cli

Composant CLI du projet.

## Rôle

- Vérifications de pré-upgrade.
- Préparation des étapes techniques.
- Journalisation structurée pour la GUI.
- Planification du report de notification.
- Préparation de la phase d'upgrade hors-ligne.

## Commandes disponibles

- `check-sources`
- `disable-third-party`
- `prepare-packages`
- `schedule-offline-upgrade`
- `run-all`
- `defer {day|week|month}`

## Modes de travail

- `--dry-run`: simule toutes les actions système.
- `--debug`: ajoute des événements de debug JSON sur les actions prévues.

Exemple:

```bash
cargo run -p backend-cli -- --dry-run --debug run-all
```
