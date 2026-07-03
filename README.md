# 📋 copicol

Un gestionnaire de presse-papiers **simple et rapide**, inspiré de CopyQ mais sans la complexité.

Appuyez sur `Ctrl+Shift+V`, tapez quelques lettres, appuyez sur `Entrée` — c'est collé.

## Fonctionnalités

- 📋 **Historique** des 100 derniers copier/coller (texte)
- 🔍 **Recherche instantanée** dans tout l'historique
- ⭐ **Épinglage** des éléments importants (jamais purgés)
- 🗂️ **Rangement automatique par type** — copicol détecte ce que vous copiez
  et le classe tout seul dans des onglets (voir plus bas)
- ⌨️ **Raccourci global** `Ctrl+Shift+V` pour afficher/cacher la fenêtre
- 📌 **Icône dans la zone de notification** — l'application vit dans le tray
- 🚀 **Collage direct** : `Entrée` colle l'élément dans l'application précédente

## Rangement automatique par type

Chaque copie est analysée à la volée et rangée dans le bon onglet — copiez un
bloc de requêtes `select * from …` et il atterrit dans l'onglet **SQL**, sans
rien faire. Les onglets n'apparaissent que s'ils contiennent des éléments,
avec leur compteur : `Tous (12) · 🗄️ SQL (5) · 🔗 URL (3) · 📄 Texte (4)`.

| Type | Détection |
|---|---|
| 🗄️ SQL | `select … from`, `insert into`, `update … set`, `create table`… |
| ⌨️ Code | accolades, mots-clés (`function`, `const`, `def`, `using`…), indentation |
| 🔗 URL | `http(s)://` ou `www.` |
| ✉️ Email | adresse seule sur une ligne |
| 🧩 JSON | objet/tableau JSON valide |
| 🎨 Couleur | `#ff8800`, `#f80`, `rgb(255, 136, 0)` — avec pastille de la couleur |
| 📞 Téléphone | 7 à 15 chiffres, formats FR inclus |
| 📁 Chemin | `C:\…`, `\\serveur\…`, `/usr/…` |
| 📄 Texte | tout le reste |

## Raccourcis clavier

| Touche | Action |
|---|---|
| `Ctrl+Shift+V` | Afficher / cacher copicol (global) |
| `↑` / `↓` | Naviguer dans la liste |
| `Entrée` | Coller l'élément sélectionné |
| `Ctrl+P` | Épingler / désépingler |
| `Suppr` | Supprimer l'élément |
| `Échap` | Cacher la fenêtre |

## Stack technique

- [Tauri 2](https://tauri.app/) — backend Rust, binaire léger (~10 Mo)
- Frontend Vite + TypeScript vanilla (pas de framework)
- SQLite ([rusqlite](https://github.com/rusqlite/rusqlite)) pour le stockage
- [arboard](https://github.com/1Password/arboard) pour la surveillance du presse-papiers
- [enigo](https://github.com/enigo-rs/enigo) pour la simulation du collage

## Architecture

```
src/                  Frontend (recherche, liste, navigation clavier)
src-tauri/src/
├─ lib.rs             Setup : tray, raccourci global, événements fenêtre
├─ watcher.rs         Surveillance du presse-papiers (polling 400 ms + hash)
├─ db.rs              SQLite : historique, dédoublonnage, purge à 100 éléments
└─ commands.rs        Commandes exposées au frontend
```

## Développement

Prérequis : [Node.js 22+](https://nodejs.org/), [Rust](https://rustup.rs/), et les
[dépendances système Tauri](https://tauri.app/start/prerequisites/) selon votre OS.

```bash
npm install
npm run tauri dev     # lance l'app en mode développement
npm run tauri build   # produit l'installeur (MSI/NSIS sous Windows)
```

Tests unitaires du backend :

```bash
cd src-tauri && cargo test
```

## Installation (Windows)

Chaque push déclenche le workflow GitHub Actions `build` qui produit
l'installeur Windows (`.msi` et `.exe`) en artefact — téléchargeable depuis
l'onglet **Actions** du dépôt.

## Feuille de route (V2+)

- Aperçu des images
- Snippets avec raccourcis
- Thème clair
- Popup au niveau du curseur
- Synchronisation entre PC
