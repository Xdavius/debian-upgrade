# Debian Major Upgrade Assistant

Un assistant graphique pour préparer et sécuriser une montée de version majeure Debian, sans vous noyer dans les commandes système.

---

## Pour qui ?

Cet outil est fait pour vous si vous voulez :

- savoir si une nouvelle version majeure Debian est disponible,
- préparer la machine proprement avant upgrade,
- suivre ce qui se passe étape par étape,
- éviter les erreurs classiques pendant une migration sensible.

---

## Installation (recommandée)

Installez **la dernière release en `.deb`**.

1. Ouvrez la page `Releases` du projet.
2. Téléchargez le fichier `.deb` de la **release la plus récente**.
3. Installez-le :

```bash
sudo apt install ./debian-upgrade_<VERSION>_amd64.deb
```

Important : ne partez pas d'une ancienne archive ou d'un build local si vous voulez un comportement stable. Prenez bien la **dernière release `.deb`**.

Note pour les utilisateurs **Xorg** : vérifiez que le paquet `libxkbcommon-x11-0` est bien installé.

```bash
sudo apt install libxkbcommon-x11-0
```

---

## Ce que fait l'application, simplement

### 1) Vérifie si une montée majeure est disponible

L'application compare votre version Debian actuelle avec la version stable suivante.

Résultat :
- si aucune mise à niveau n'est disponible, elle vous l'indique clairement,
- sinon elle vous guide vers la préparation.

### 2) Vous guide dans une interface claire

L'interface avance par étapes visibles :

1. Vérification release
2. Sources APT
3. Pilotes DKMS
4. Préparation des paquets
5. Dry-run d'upgrade
6. Redémarrage pour phase offline

Vous voyez l'état en direct (`en cours`, `ok`, `attention`, `erreur`) avec un journal lisible.

### 3) Contrôle les sources APT

Avant migration, l'outil :

- vérifie les sources Debian,
- désactive les dépôts tiers pour limiter les conflits,
- garde la traçabilité des changements.

Après upgrade, il peut réactiver uniquement les dépôts tiers que vous avez choisis.

### 4) Prépare les paquets de manière sûre

L'application lance les actions nécessaires (nettoyage, mise à jour des index, téléchargement des paquets) en mode non interactif pour éviter les blocages.

### 5) Fait un test sans risque (dry-run)

Un test `apt-get -s dist-upgrade` est exécuté pour simuler la montée de version avant le vrai redémarrage.

Objectif : détecter les problèmes avant la phase critique.

### 6) Gère le cas des pilotes DKMS

L'outil prépare la liste DKMS à réinstaller et enchaîne une phase dédiée après la phase principale, uniquement si nécessaire.

### 7) Lance la phase offline au redémarrage

Quand tout est prêt, vous redémarrez depuis l'interface.

Au boot, l'upgrade se fait en mode offline via `system-update` pour plus de robustesse, puis la machine redémarre normalement.

### 8) Vous notifie automatiquement

Le package installe un service/timer `systemd` qui vérifie périodiquement la disponibilité d'une nouvelle version majeure et envoie une notification interactive.

Depuis cette notification, vous pouvez :
- ouvrir l'assistant,
- reporter (1 jour, 1 semaine, 1 mois).

---

## Expérience utilisateur

- Interface centrée sur la lisibilité.
- Progression visible étape par étape.
- Logs en direct pour comprendre ce qui se passe.
- Mode normal (utilisateur) + mode debug (tests).

---

## Utilisation rapide

Une fois installé, lancez :

```bash
debian-upgrade
```

Le parcours recommandé est de suivre les étapes dans l'ordre jusqu'à l'écran final de redémarrage.

---

## Informations techniques

### Composants

- `frontend-gui` : interface graphique (Rust + Slint)
- `backend-cli` : binaire d'orchestration
- `upgrade-core` : logique métier partagée

### Fichiers et scripts principaux

- Script offline : `/usr/local/lib/debian-upgrade/offline-upgrade.sh`
- Service offline : `/usr/lib/systemd/system/debian-upgrade-offline.service`
- Vérification notification : `/usr/local/lib/debian-upgrade/check-upgrade-notify.sh`

### Services systemd installés

- `debian-upgrade-notify.service`
- `debian-upgrade-notify.timer`
- `debian-upgrade-offline.service`

### Journaux utiles

- Journal systemd :

```bash
journalctl -u debian-upgrade-offline.service -b
```

- Log offline détaillé :

```bash
/var/log/debian-upgrade-offline.log
```

### Structure du dépôt

- `context.md` : vision, décisions et journal de suivi
- `docs/` : documentation d'architecture
- `packaging/` : assets et script de packaging
- `build.sh` : build/test local

### Build local (développeurs)

```bash
./build.sh
cargo check -p upgrade-core -p backend-cli -p frontend-gui
```
