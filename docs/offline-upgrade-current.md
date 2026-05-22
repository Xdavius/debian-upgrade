# Offline Upgrade (Etat Actuel)

Ce document decrit le fonctionnement actuel de la mise a niveau majeure Debian en mode offline, incluant la phase DKMS et la notification post-upgrade.

## 1. Vue generale

Le workflow offline est en 2 phases, executees via `system-update`:

1. Phase 1: `upgrade` (APT full-upgrade offline)
2. Phase 2: `dkms` (reconstruction DKMS), uniquement si necessaire

Le service offline utilise:

- unite systemd: `debian-upgrade-offline.service`
- script: `/usr/local/lib/debian-upgrade/offline-upgrade.sh`

## 2. Armement du mode offline

Avant reboot, l'application arme le mode `system-update`:

- marker `/system-update` -> `/var/lib/system-update`
- activation du service offline dans `system-update.target.wants`
- ecriture de la phase initiale `upgrade`

Fichier de phase:

- `/var/lib/debian-upgrade/offline-phase`
  - valeur `upgrade` ou `dkms`

## 3. Phase 1 - Upgrade offline

La phase 1 execute:

- `apt-get full-upgrade` non interactif
- parsing de progression pour Plymouth
- `apt-get clean` apres succes

En cas d'echec APT:

- statut post-upgrade marque `failed_upgrade`
- notification post-upgrade preparee

En cas de succes:

- si liste DKMS vide/inexistante: finalisation immediate
- si liste DKMS non vide: armement d'un second cycle `system-update` avec phase `dkms`

## 4. Phase 2 - DKMS

La phase 2 est lancee uniquement si:

- phase 1 validee (`phase1.ok`)
- liste DKMS presente

Pour chaque entree module/version:

- tentative `dkms install -m <module> -v <version>`
- si echec: tentative `dkms remove -m <module> -v <version>` (sans `--all`)

Comportement:

- echec d'un module DKMS non bloquant pour la fin de cycle
- les compteurs succes/echec sont traces
- la liste des modules en echec est tracee

## 5. Depots tiers

Pendant la preparation:

- les depots tiers sont desactives

En fin de cycle:

- reactivation automatique des depots selectionnes par l'utilisateur
- reactivation basee sur `/var/lib/debian-upgrade/third-party-reactivate.list`

## 6. Etat et traces ecrites

Etat principal post-upgrade:

- `/var/lib/debian-upgrade/post-upgrade-status.env`

Champs actuels:

- `timestamp`
- `result` (`success`, `success_dkms`, `partial_dkms`, `failed_upgrade`, ...)
- `phase`
- `dkms_total`
- `dkms_ok`
- `dkms_ko`
- `dkms_remove_ok`
- `dkms_remove_ko`
- `dkms_failed_modules` (noms modules, separes par virgules)

Flag de notification:

- `/var/lib/debian-upgrade/post-upgrade-notify.pending`

Logs:

- `/var/log/debian-upgrade-offline.log`
- `/var/lib/debian-upgrade/third-party-actions.log`

## 7. Notification post-upgrade (sans GUI)

Script de notif:

- `/usr/local/lib/debian-upgrade/check-post-upgrade-notify.sh`

Service:

- `debian-upgrade-post-notify.service`

Declenchement:

- `debian-upgrade-post-notify.path`
- watch `PathExistsGlob=/run/user/*/bus` (session utilisateur prete)

Contenu notif:

- urgence toujours `critical`
- resume du resultat
- en cas DKMS partiel:
  - comptage echec/total
  - liste des modules en echec (1 par ligne dans la notif)
  - resultat du nettoyage auto `dkms remove`
- action: `Lire le journal detaille`

Action utilisateur:

- clic `Lire le journal detaille` ouvre `/var/log/debian-upgrade-offline.log`
- le cycle de notif est considere traite (suppression du `pending`)

Si la notif est fermee/ignoree:

- `pending` reste en place
- rappel au prochain demarrage/connexion

## 8. Etat actuel des unites packagees

Le package installe/active:

- `debian-upgrade-notify.timer` (detection nouvelle majeure)
- `debian-upgrade-post-notify.path` (notif post-upgrade)

Note:

- un fichier `debian-upgrade-post-notify.timer` peut exister dans le depot comme vestige, mais le flux actif post-upgrade repose sur le `.path`.
