# buildtest

Contenu de test local généré par `./build.sh`.

## Lancer la GUI (prioritaire)

```bash
./run-gui.sh
```

Mode debug:

```bash
./run-gui.sh --debug
```

Note: si `rustc` est trop ancien (ex: 1.63), la GUI peut etre ignoree au build et seul le backend est produit.

## Lancer une démo dry-run CLI seule

```bash
./run-dry-run-demo.sh
```
