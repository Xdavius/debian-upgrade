# Architecture (Draft)

## Vue d'ensemble

- `backend-cli`: moteur d'orchestration et d'actions système.
- `frontend-gui`: expérience utilisateur et suivi visuel.

## Flux principal

1. Déclenchement via notification interactive.
2. Lancement GUI.
3. Exécution des étapes backend et streaming d'événements.
4. Validation finale et demande de redémarrage.
5. Upgrade hors-ligne au reboot.

## Contrat d'événements (proposition)

```json
{"timestamp":"...","level":"info","step":"check-sources","state":"running","message":"..."}
```

## Points de sécurité

- Confirmation explicite avant actions destructives.
- Logs persistants pour audit.
- Gestion des erreurs avec arrêt sûr.
