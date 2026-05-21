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
- Branchement execution page 2 GUI:
  - Le bouton `Suivant` de la page 2 appelle maintenant `upgrade-core` en sequence:
    - `CheckSources`
    - `DisableThirdParty`
  - En mode normal: execution reelle (`dry_run=false`).
  - En mode `--debug`: execution simulee (`dry_run=true`) pour tests.
  - Si execution normale echoue par manque de droits (`/etc/apt/...`), fallback automatique avec elevation privilegiee:
    - appel du backend CLI via `pkexec`,
    - fallback `zenity + sudo` si `pkexec` indisponible,
    - reinjection des logs JSON backend dans l'UI.
- Branchement execution page 3 GUI (preparation paquets):
  - En mode normal: `PreparePackages` execute reellement (`dry_run=false`) pour lancer le nettoyage cache APT + telechargement paquets.
  - En mode `--debug`: `PreparePackages` reste en simulation (`dry_run=true`).
  - En cas d'echec par permissions, fallback automatique avec elevation privilegiee via backend CLI (`prepare-packages`) + reinjection des logs JSON.
- Branchement execution page 4 GUI (test dry-run upgrade):
  - La page 4 n'est plus une temporisation fictive: elle appelle `upgrade-core` via `DryRunUpgrade`.
  - En mode normal: execution reelle de `apt-get -s dist-upgrade` (non interactif).
  - En mode `--debug`: simulation.
  - En cas d'echec par permissions, fallback automatique avec elevation privilegiee via backend CLI (`dry-run-upgrade`).
- Journal logs / UX temps reel:
  - Les sorties des commandes APT sont maintenant remontees ligne par ligne (streaming) dans `upgrade-core` au lieu d'attendre la fin de commande.
  - Le chemin privilegie GUI (pkexec/zenity+sudo) relaie les logs backend en streaming vers l'UI sur toutes les pages branchees (`check-sources`/`disable-third-party`, `prepare-packages`, `dry-run-upgrade`).
  - Auto-scroll logs ajuste: il suit la fin tant que la zone logs n'a pas le focus; pendant selection/copie, l'auto-scroll est suspendu pour eviter les sauts.
  - Correctif anti-crash selection logs: pendant que la zone logs a le focus (selection active), les nouvelles lignes ne sont plus injectees en direct dans `TextEdit`; elles sont tamponnees puis re-appliquees apres perte de focus.
  - Lissage UI unifie: toutes les pages utilisent maintenant une meme file d'events + flush cadence (20ms) en rendu ligne-par-ligne (et non par gros paquets), pour conserver un vrai effet stream tout en gardant la fluidite.
  - Correctif blocage page 4 (chemin privilegie): suppression du risque de deadlock de pipes `stderr` dans les fonctions streaming `pkexec`/`sudo` (stderr redirige vers `null`, attente via `wait()`).
  - Optimisation supplementaire anti-lag page 4: les lignes de logs d'un batch sont maintenant concatenees et poussees en une seule mise a jour `set_logs_text` (au lieu d'une reecriture par ligne), reduisant fortement le cout UI sur gros volumes.
  - Correctif streaming page 4: le parseur backend traite maintenant `\\r` ET `\\n` comme separateurs de lignes (certaines sorties `apt-get -s` utilisent surtout des retours chariot), ce qui evite l'effet "rien puis bloc final".
  - Correctif buffering IPC backend->GUI: `backend-cli` force un `stdout.flush()` apres chaque event JSON pour eviter la retention en buffer sur les executions via pipe (pkexec/sudo), en particulier visible sur la page 4.
  - Correctif fluidite global logs: passage a un ring buffer borne (1200 lignes max) cote GUI pour eviter la croissance non bornee et les reecritures de texte gigantesques qui pouvaient figer la fenetre en page 4.
  - Nouvelle tentative operationnelle demandee: clean des logs au lancement de la page 4 + throttle d'affichage global (`40ms`, `8` lignes max par flush) pour limiter la pression UI tout en conservant un flux progressif.
  - Ajustement suivant demande utilisateur: suppression du `clear logs` automatique en page 4.
  - Migration composant logs: abandon de `TextEdit` au profit d'un affichage `ScrollView + Text` pour limiter les instabilites liees a l'edition/selection sous gros flux.
- Ajustements layout recents:
  - Hauteur carte logs augmentee (`120px` -> `180px`) pour afficher davantage de lignes.
  - Hauteur fenetre principale augmentee (`620px` -> `700px`) pour garder les boutons visibles.
- Upgrade-core execution reelle:
  - `DisableThirdParty`: desactivation effective des fichiers tiers `.list/.sources` (hors `debian.sources`) via renommage `*.disabled-by-debian-upgrade` en mode normal.
  - `PreparePackages`: execution effective de `apt-get clean`, `apt-get update`, puis `apt-get --download-only dist-upgrade` en mode non interactif.
  - Nouvelle commande `DryRunUpgrade`: execution de `apt-get -s dist-upgrade` en mode non interactif.
- Optimisation build release:
  - Ajout d'un profil `[profile.release]` au workspace dans `Cargo.toml`.
  - Parametres actifs: `opt-level=3`, `lto=thin`, `codegen-units=1`, `strip=symbols`, `debug=false`, `incremental=false`.
  - Impact attendu: binaires release plus optimises/perf et plus petits pour le packaging.
- Compatibilite toolchain Debian 12:
  - Edition Rust du workspace ramenee de `2024` a `2021` dans `Cargo.toml`.
  - Objectif: permettre build/check/test avec Cargo ancien de Debian 12 sans upgrade forcée.
  - Validation effectuee: `build.sh` complet passe (check, tests backend, build release GUI+CLI, bundle `buildtest`).
  - Compatibilite lockfile: `Cargo.lock` ajuste en `version = 3` (au lieu de `4`) pour prise en charge par le Cargo Debian 12.
  - Pin de dependance transitive pour Cargo ancien: `home` forcee a `0.5.11` via lockfile (`cargo update -p home --precise 0.5.11`) car `0.5.12` exige edition 2024.
  - Pin additionnel: `clru` forcee a `0.6.2` via lockfile (`cargo update -p clru --precise 0.6.2`) car `0.6.3` exige edition 2024.
  - Pin additionnel: `linked_hash_set` forcee a `0.1.5` via lockfile (`cargo update -p linked_hash_set --precise 0.1.5`) car `0.1.6` exige edition 2024.
  - Pin additionnel: `smithay-clipboard` forcee a `0.7.2` via lockfile (`cargo update -p smithay-clipboard --precise 0.7.2`) car `0.7.3` exige edition 2024.
  - Pin additionnel: `indexmap` forcee a `2.13.0` via lockfile (`cargo update -p indexmap --precise 2.13.0`) car `2.14.0` exige edition 2024.
  - Stabilisation TOML ecosystem: `proc-macro-crate` forcee a `3.2.0` (au lieu de `3.5.0`) pour retirer `toml_datetime 1.x`/`toml_edit 0.25.x` incompatibles Cargo Debian 12; retour sur `toml_datetime 0.6.11` + `toml_edit 0.22.27`.
  - Pin additionnel rustc 1.63: `wayland-protocols` forcee a `0.31.0` (au lieu de `0.31.2`) pour contourner la contrainte MSRV >= 1.65.
  - Pin additionnel rustc 1.63: `webpki-roots` forcee a `1.0.6` (au lieu de `1.0.7`) pour contourner la contrainte MSRV >= 1.64.
  - Pin additionnel rustc 1.63: `winnow` forcee a `0.7.13` (au lieu de `0.7.15`) pour contourner la contrainte MSRV >= 1.65.
  - Tentative de batch pin effectuee: certains crates ne peuvent pas etre downgrades arbitrairement a cause des contraintes semver transitives (`toml_write ^0.1.2`, `hashbrown ^0.16.1` via `indexmap`).
  - Limite structurelle GUI (stack Slint/Winit): certaines deps imposent des versions minimales de rustc non contournables par pin simple (ex: `raw-window-handle ^0.5.2` via `glutin`).
  - `build.sh` ajoute un fallback: si `rustc < 1.64`, build GUI saute proprement avec warning, backend continue d'etre build/test/package dans `buildtest`.
  - Decision ulterieure: suppression volontaire de tous les pins temporaires de dependances (retour au resolver standard via `cargo update`) apres choix d'upgrade Cargo/toolchain cote utilisateur.
  - Ajustement complementaire pour rustc 1.85 (Debian 13): reintroduction de pins utiles uniquement:
    - `tower-http` -> `0.6.8`
    - `url` -> `2.4.1` (retour sur `idna 0.4.0`, suppression stack ICU 2.2.0 exigeant rustc 1.86)
    - `image` -> `0.25.8` (au lieu de `0.25.10` qui exige rustc 1.88)
  - Validation: `cargo check -p upgrade-core -p backend-cli -p frontend-gui` OK avec rustc 1.85.
  - Contournement runtime GUI Debian 12/distrobox: `run-gui.sh` force `WINIT_UNIX_BACKEND=x11` par defaut pour eviter le panic Wayland `XKBNotFound` (surcharge possible via variable d'environnement).
  - Hardening anti-freeze minimise/restauration (environnements Debian anciens/container): launcher GUI force aussi `SLINT_BACKEND=winit-software`, `WINIT_X11_SCALE_FACTOR=1`, `LIBGL_ALWAYS_SOFTWARE=1` par defaut.
  - Retour a la strategie historique demandee:
    - ajustement pragmatique runtime: launcher GUI force maintenant `SLINT_BACKEND=winit-software` et `WINIT_UNIX_BACKEND=x11` pour eviter les panics de chargement `libwayland-egl.so` en environnements incomplets;
    - cette voie desactive la tentative Wayland au lancement depuis `run-gui.sh`;
    - plus de variable "mode special" a activer manuellement par l'utilisateur.
- Compatibilite Debian 12/13 sur normalisation APT:
  - `upgrade-core` detecte maintenant si `apt modernize-sources` est disponible.
  - Si disponible (Debian recentes), l'action planifiee l'utilise.
  - Si indisponible (cas Debian 12), le workflow n'echoue pas et passe en fallback manuel sur `.list/.sources` avec log explicite.
  - Regle explicite Debian 12 -> 13 anti-doublon:
    - si `debian.sources` est present, il est prioritaire pour la migration (`Suites:` mis a jour).
    - dans ce cas, `sources.list` n'est pas migre et est renomme en `/etc/apt/sources.bak`.
    - si `debian.sources` est absent, fallback sur migration de `/etc/apt/sources.list`.
    - la migration des suites couvre aussi les alias (`stable`, `oldstable`, `stable-updates`, `oldstable-updates`, `stable-security`, `oldstable-security`) vers le codename cible explicite.
  - En `dry-run`, ces operations sont simulees et journalisees; en mode reel, elles sont appliquees.

### 2026-05-21

- Relecture complete de `context.md` en debut de session pour reprise de contexte projet.
- Confirmation explicite des axes actifs (GUI principale, backend moteur, workflow offline, fallback privilege, packaging, suivi continu).
- Rappel utilisateur integre: journaliser systematiquement les evolutions de session directement dans `context.md`.
- Ajout de commentaires explicatifs rapides au-dessus de chaque fonction Rust dans:
  - `backend-cli/src/main.rs`
  - `frontend-gui/build.rs`
  - `frontend-gui/src/main.rs`
  - `upgrade-core/src/lib.rs` (y compris fonctions publiques et test unitaire local).
- Verification post-modification: `cargo check -p upgrade-core -p backend-cli -p frontend-gui` OK.
- Refonte de la desactivation des depots tiers (sans renommage de fichiers) dans `upgrade-core`:
  - Fichiers `.list` (et tolerance `.lsit`): les lignes actives commencant par `deb` ou `deb-src` sont commentees.
  - Fichiers `.sources` (deb822): chaque entree de depot est forcee en `Enabled: no`;
    - si `Enabled` existe deja: valeur remplacee par `no`,
    - si `Enabled` est absente: ligne `Enabled: no` ajoutee pour l'entree.
  - Support explicite des fichiers avec plusieurs depots (plusieurs stanzas) dans un meme `.sources`.
  - Le mode `dry-run` journalise les modifications sans ecriture disque.
- Validation post-refonte: `cargo check -p upgrade-core -p backend-cli -p frontend-gui` OK.
- GUI page depots tiers rendue dynamique et scrollable:
  - Remplacement des 3 cases statiques par un modele Slint `third_party_repos` (liste de longueur variable).
  - Affichage via `ScrollView` + repetition de `CheckBox`, permettant la selection quel que soit le nombre de depots (1, 10, ou plus).
  - Ajout d'un callback UI `set_third_party_enabled` pour synchroniser l'etat coche/decoché dans le modele.
  - Adaptation Rust (`frontend-gui/src/main.rs`):
    - initialisation du `VecModel<ThirdPartyRepo>` depuis la detection systeme,
    - lecture des depots re-actives depuis le modele dynamique (plus de limite a 3).
  - Correctif syntaxe Slint associe: signature callback et texte page 1 rendu valide.
- Validation post-modification UI: `cargo check -p frontend-gui -p upgrade-core -p backend-cli` OK.
- Ajustement UX logs page 1/page 2:
  - Le message de detection des depots tiers ("aucun depot" / nombre detecte) n'est plus affiche au demarrage page 1.
  - Le message est maintenant emis lors du passage effectif vers la page 2 apres clic `Suivant` (verification release OK, ou bypass debug).
  - Correction associee: texte multi-ligne page 1 rendu avec `\\n` valide Slint (suppression du literal invalide).
- Validation post-correctif: `cargo check -p frontend-gui` OK.
