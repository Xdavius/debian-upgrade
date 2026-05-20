# frontend-gui

Composant GUI (Rust + Slint) du projet.

## Rôle

- Assistant multi-écrans (suivant).
- Affichage de progression des étapes.
- Écran de logs en temps réel.
- Écran final avec redémarrage pour déclencher l'upgrade hors-ligne.
- Respect du thème clair/sombre système.

## Écrans prévus (MVP)

- `welcome`
- `progress_logs`
- `final_reboot`

## Notes

La GUI interagira avec le backend via un protocole d'événements à définir (JSON lines proposé).
Selon la version locale de Rust, Slint peut nécessiter une toolchain plus récente pour compiler.
