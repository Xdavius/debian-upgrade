# Contexte Projet - Debian Major Upgrade Assistant

## 1. Vision

Créer une solution de mise à niveau majeure Debian assistée, composée:

- d'un outil backend en CLI (orchestration technique et vérifications),
- d'une interface frontend graphique (Rust/Slint) pour guider l'utilisateur.

Le but est de réduire les erreurs de préparation, améliorer la visibilité des étapes, et rendre l'expérience plus rassurante.

## 2. Objectifs Produit

- Vérifier automatiquement l'environnement avant upgrade majeure.
- Préparer le système de manière contrôlée et reproductible.
- Informer l'utilisateur via une notification interactive:
  - lancer immédiatement l'interface de mise à niveau,
  - reporter la notification de 1 jour, 1 semaine ou 1 mois.
- Exécuter une stratégie de mise à niveau en plusieurs phases, dont une phase finale hors-ligne au redémarrage.

## 3. Parcours Utilisateur

1. Une notification informe qu'une montée de version majeure est disponible.
2. L'utilisateur choisit:
   - démarrer maintenant,
   - reporter de 1 jour / 1 semaine / 1 mois.
3. Au lancement de la GUI, un assistant multi-écrans présente:
   - prérequis,
   - étapes en cours,
   - logs d'avancement.
4. Une fois la préparation terminée, la GUI propose un bouton de redémarrage.
5. Le redémarrage déclenche la phase de mise à niveau hors-ligne.

## 4. Pipeline Technique Cible

Étapes de préparation avant upgrade finale:

1. Vérification de la normalisation des sources APT.
2. Désactivation des sources tierces.
3. Vidage du cache APT puis téléchargement des paquets.
4. Bascule vers une mise à niveau de type hors-ligne.

## 5. Architecture Prévisionnelle

Principe directeur:

- La GUI est l'interface utilisateur principale du projet.
- La CLI est le moteur backend d'exécution et d'orchestration, appelé par la GUI.

### Backend CLI

Responsabilités:

- Détecter l'état de la machine et les prérequis.
- Exposer des sous-commandes pour chaque étape.
- Produire des logs structurés exploitables par la GUI.
- Gérer la planification du report de notification.
- Préparer le déclenchement de l'upgrade hors-ligne au reboot.

### Frontend GUI (Rust + Slint)

Responsabilités:

- Assistant en plusieurs écrans ("suivant").
- Écran de logs temps réel.
- Écran final avec action de redémarrage.
- Respect du thème système clair/sombre.
- Interface volontairement sobre et lisible.

### Contrat CLI <-> GUI (à figer)

- Format d'événements: JSON lines (proposition initiale).
- Niveaux de logs: `info`, `warn`, `error`, `success`.
- États d'étape: `pending`, `running`, `done`, `failed`.

## 6. Principes UX/UI

- Design sobre, stable, sans surcharge visuelle.
- Cohérence desktop Linux.
- Bonne lisibilité pour les actions critiques.
- Le thème suit automatiquement le mode clair/sombre système.

## 7. Contraintes et Risques

- Opérations sensibles (sources APT, cache, upgrade système).
- Risque de coupure/interruption pendant préparation.
- Besoin de reprise et de traçabilité des opérations.
- Différences de configuration selon installations Debian.

## 8. Stratégie MVP

### MVP v0.1

- CLI capable d'exécuter séquentiellement les 4 étapes de préparation.
- GUI capable d'afficher:
  - un écran d'introduction,
  - un écran de progression + logs,
  - un écran final avec redémarrage.
- Planification simple des reports de notification.

### Hors MVP (ensuite)

- Reprise automatique après incident.
- Rapports de diagnostic enrichis.
- Modes "simulation" et "audit only".

## 9. Roadmap Initiale

1. Initialiser workspace Rust (CLI + GUI).
2. Définir le protocole d'échange CLI/GUI.
3. Implémenter les vérifications APT (lecture seule).
4. Implémenter actions de préparation (avec garde-fous).
5. Connecter GUI aux événements backend.
6. Implémenter bouton final "redémarrer pour upgrader".

## 10. Décisions Ouvertes

- Mécanisme exact de notification interactive (systemd + desktop notifications ?).
- Outil de scheduling du report (timer systemd recommandé).
- Implémentation précise de l'upgrade hors-ligne selon version Debian cible.
- Politique de rollback / reprise en cas d'échec.

## 11. Politique d'Élévation de Privilèges

- Toute élévation de privilèges devra être effectuée en priorité via `pkexec`.
- En cas d'indisponibilité/échec de `pkexec`, un fallback via `zenity` sera utilisé.
- En mode `zenity`, le champ de saisie du mot de passe devra être vidé immédiatement après usage, et ne jamais être conservé en mémoire plus longtemps que nécessaire.
- Les logs applicatifs ne doivent jamais contenir le mot de passe ni ses dérivés.

## 12. Journal de Suivi

## 12. Règle de Suivi

- Le suivi dans `context.md` doit être mis à jour systématiquement à chaque évolution significative du projet (technique, produit, architecture, tests, build, debug).
- Chaque session de travail doit laisser une trace explicite dans le journal de suivi avec la date et les changements réalisés.

### 2026-05-20

- Création du référentiel de contexte projet.
- Cadrage du périmètre fonctionnel et technique MVP.
- Préparation de la structure initiale du dépôt.
- Initialisation du workspace Rust (`backend-cli`, `frontend-gui`).
- Ajout d'un premier squelette CLI (sous-commandes MVP + événements JSON).
- Ajout d'un stub GUI Slint pour démarrage des écrans.
- Mise en place du mode `dry-run` et `debug` pour la CLI backend.
- Ajout d'une logique de tests backend (`cargo test -p backend-cli`).
- Ajout du script racine `build.sh` générant `buildtest/` compilé et testable.
- Mise en place du comportement CLI: affichage automatique de l'aide si aucune commande n'est fournie.
- Décision de gouvernance: le suivi `context.md` est obligatoire et continu.
- Démarrage de la GUI Slint avec une base visuelle sobre de style GTK: en-tête, étapes, logs et boutons d'action.
- Résolution du blocage de compilation GUI sans upgrade Rust:
  - réduction des features Slint (`default-features = false`),
  - activation `compat-1-2`, `backend-winit`, `renderer-software`,
  - verrouillage du graphe de dépendances sur des versions compatibles Rust 1.85 (`url 2.4.1`, `idna 0.4.0` via lockfile).
- Branchement GUI <-> CLI dry-run: la GUI peut lancer `backend-cli --dry-run --debug run-all`, lire les evenements JSON et mettre a jour les logs/etats en direct.
- Politique d'élévation ajoutée: priorité `pkexec`, fallback `zenity`, avec vidage du champ mot de passe.
- Script `build.sh` mis a jour avec priorite GUI: check/build `frontend-gui`, packaging GUI+CLI dans `buildtest/`, ajout de `run-gui.sh`.
- Clarification produit: la GUI est définie comme interface utilisateur principale; la CLI reste le moteur backend.
- Nommage des executables ajuste:
  - GUI principale: `debian-upgrade`
  - moteur backend: `debian-upgrade-backend`
- Correction du lancement backend depuis la GUI: resolution robuste du binaire selon l'emplacement d'execution (`target/` et `buildtest/bin`).
- Extraction du coeur backend dans la crate `upgrade-core` (lib): commandes, contexte, generation d'evenements et orchestration dry-run.
- `backend-cli` devient un adaptateur Clap + emission JSON vers stdout, sans logique metier dupliquee.
- GUI branchee directement sur `upgrade-core` (mode integre) pour un dry-run robuste sans sous-processus.
- Le mode process backend est conserve en fallback dans la GUI (`debian-upgrade-backend`).
- Refonte GUI en mode pages (1 a 5): accueil, sources APT/depots tiers, preparation paquets, test dry-run upgrade, finalisation redemarrage.
- Page 2: depots tiers desactives par defaut avec cases a cocher de reactivation manuelle.
- Page 3: clic sur Suivant declenche la preparation paquets (upgrade-core), puis passage auto page 4.
- Page 4: test dry-run upgrade (simulation guidee), puis passage auto page 5 en cas de succes.
- Page 5: message explicite sur l'execution non interactive et choix par defaut pour eviter les blocages APT.
- Ajustement UX GUI: affichage des logs limite a 10 lignes max (rotation FIFO).
- Ajustement layout pages: zone centrale re-equilibree et contenu des pages force en pleine zone pour eviter les elements trop bas (notamment page 2 depots tiers).
- Refactor GUI Slint: separation de l'UI dans `frontend-gui/ui/app.slint`.
- Ajout de `frontend-gui/build.rs` + `slint-build` et migration de `main.rs` vers `slint::include_modules!()`.
- Page 2 GUI corrigee: detection reelle des depots tiers dans `/etc/apt/sources.list.d` avec support `.list` et `.sources`.
- Les noms de depots affiches ne sont plus statiques; ils viennent du systeme (limite actuelle: 3 premiers affiches).
- Regle detection depots tiers: `debian.sources` est ignore, car considere comme source officielle Debian apres normalisation.
- Logs GUI ajustes pour suivre la derniere ligne: les nouveaux logs sont inseres en tete (affichage des plus recents en haut), avec limite de 10 lignes.
- Ajout verification en ligne de la nouvelle version majeure Debian via https://www.debian.org/releases/index.en.html (stable version + codename cible).
- Le workflow s'arrete si aucune nouvelle version majeure n'est disponible, avec affichage d'une page GUI dediee (page 6).
- Protection contre le bug `debian.sources`: la logique considere cette source comme officielle et evite les manipulations tiers.
- Objectif de normalisation des sources renforce: remplacement de tous les codenames actuels par le codename de la nouvelle majeure cible.
- Correction bug verification en ligne: abandon du parsing HTML fragile; utilisation des fichiers officiels `https://deb.debian.org/debian/dists/stable/Release` et `.../testing/Release` pour extraire version/codename.
- Zone logs GUI migree vers `TextEdit` read-only: selection/copie possible et scroll manuel.
- Limite des 10 lignes retiree: historique de logs complet conserve dans l'UI.
- Logs GUI: auto-scroll ajoute vers la derniere ligne via `TextEdit.set-selection-offsets(end,end)` apres chaque ajout de log.
- Mode debug etendu: si aucune nouvelle version majeure n'est disponible, le workflow peut continuer pour tests (backend + GUI), avec logs explicites de bypass.
- GUI: mode normal actif par defaut; option `--debug` ajoutee pour activer explicitement le mode debug (bypass de test).
- Chaine build/test corrigee: `buildtest/run-gui.sh` propage maintenant les arguments (`$@`), ce qui permet `./buildtest/run-gui.sh --debug`.
- Documentation buildtest mise a jour avec l'usage du mode debug GUI.
- Adaptation non interactive integree dans `upgrade-core`:
  - Variables env: `DEBIAN_FRONTEND=noninteractive`, `DEBIAN_PRIORITY=critical`, `APT_LISTCHANGES_FRONTEND=none`.
  - Options APT/dpkg: `-y`, `--force-confdef`, `--force-confold`, `Always-Include-Phased-Updates=true`.
  - Dry-run: affichage explicite des commandes qui seraient executees.
  - Mode reel: commande post-reboot non interactive journalisee pour la mise a niveau hors-ligne.
- Mode normal rendu fonctionnel sur action `Redemarrer` GUI:
  - Preparation reelle de l'upgrade hors-ligne via script `/usr/local/lib/debian-upgrade/offline-upgrade.sh`.
  - Bascule vers le flux systemd offline update standard: creation du marqueur `/system-update -> /var/lib/system-update`.
  - Creation/activation d'un service one-shot `debian-upgrade-offline.service` dans `system-update.target.wants` (au lieu d'un demarrage en mode normal).
  - Le script hors-ligne supprime le marqueur tres tot pour eviter les boucles de boot, puis execute `apt-get update` + `apt-get dist-upgrade` en mode non interactif.
  - Elevation privilegies prioritaire via `pkexec`, fallback `zenity` + `sudo -S` (avec effacement du mot de passe en memoire).
  - Redemarrage systeme declenche apres armement.
- Mode debug conserve en simulation (pas d'action systeme destructive).
- Demarrage du chantier packaging:
  - Ajout d'un `pacscript` initial: `packaging/pacstall/debian-upgrade-deb.pacscript`.
  - Le package installe la GUI (`/usr/bin/debian-upgrade`) et le backend (`/usr/libexec/debian-upgrade-backend`).
  - Les fichiers systeme lies a l'upgrade hors-ligne sont packages explicitement (`/usr/local/lib/debian-upgrade/offline-upgrade.sh` et `/usr/lib/systemd/system/debian-upgrade-offline.service`) pour garantir leur suppression a la desinstallation.
  - Ajout d'un launcher desktop (`/usr/share/applications/debian-upgrade.desktop`).
  - Refactor packaging: les fichiers installes ne sont plus en heredoc dans le pacscript.
  - Assets versionnes ajoutes dans le depot:
    - `packaging/assets/bin/offline-upgrade.sh`
    - `packaging/assets/systemd/debian-upgrade-offline.service`
    - `packaging/assets/desktop/debian-upgrade.desktop`
  - Le pacscript installe maintenant ces fichiers directement via `install -Dm...` pour faciliter maintenance, revue et suppression propre.
- GUI runtime alignee packaging:
  - `frontend-gui` ne genere plus le script offline ni le service systemd a l'execution.
  - En mode normal, la GUI verifie la presence des fichiers packages:
    - `/usr/local/lib/debian-upgrade/offline-upgrade.sh`
    - `/usr/lib/systemd/system/debian-upgrade-offline.service`
  - La GUI arme uniquement le mode offline (lien `/system-update`, lien dans `system-update.target.wants`, `systemctl daemon-reload`) puis declenche le reboot.
  - Objectif: aucun fichier systeme critique cree "ad hoc" hors packaging.
- Optimisation build release:
  - Ajout d'un profil `[profile.release]` au workspace dans `Cargo.toml`.
  - Parametres actifs: `opt-level=3`, `lto=thin`, `codegen-units=1`, `strip=symbols`, `debug=false`, `incremental=false`.
  - Impact attendu: binaires release plus optimises/perf et plus petits pour le packaging.
