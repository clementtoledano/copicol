# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Projet

copicol est un gestionnaire de presse-papiers Windows (Tauri 2) qui vit dans le tray. Fenêtre sans décorations, cachée au démarrage, affichée/cachée par le raccourci global `Ctrl+Shift+V` ; fermer ou perdre le focus cache la fenêtre au lieu de quitter.

**Convention de langue : tout est en français** — commentaires de code (Rust et TypeScript), textes d'interface, messages de commit, README.

## Commandes

```bash
npm install            # dépendances frontend
npm run tauri dev      # lance l'app en mode développement (hot reload frontend)
npm run tauri build    # produit l'installeur (MSI/NSIS sous Windows)
npm run build          # tsc + vite build — sert aussi de vérification TypeScript
```

Tests (uniquement côté Rust, dans `db.rs` et `detect.rs`) :

```bash
cd src-tauri
cargo test                    # tous les tests
cargo test nom_du_test        # un seul test
cargo check                   # vérification de compilation
```

Pas de linter configuré (ni ESLint ni clippy en CI). La CI (`build.yml`) construit l'installeur Windows à chaque push et exécute `cargo test` + `cargo check` sous Linux.

## Architecture

### Backend Rust (`src-tauri/src/`)

- **`lib.rs`** — point d'assemblage : plugins Tauri, `AppState { db: Mutex<Connection>, watcher_tx }`, raccourci global, menu du tray, événements fenêtre. Le raccourci et le tray sont non fatals (l'app doit démarrer sans eux, ex. Wayland).
- **`watcher.rs`** — thread dédié, **unique propriétaire du presse-papiers** (arboard). Polling toutes les 400 ms avec dédoublonnage par hash SHA-256. Ignore les contenus > 2 Mo et ceux marqués `ExcludeClipboardContentFromMonitorProcessing` (gestionnaires de mots de passe).
- **`db.rs`** — SQLite (rusqlite bundled) dans le dossier AppData. Tables `items`, `groups`, `settings`. Purge à 100 éléments non épinglés (`MAX_ITEMS`). Import/export JSON des favoris. Contient les tests (avec `open_in_memory`).
- **`detect.rs`** — classement automatique du contenu copié : première règle qui matche parmi url, email, color, json, path, phone, sql, code, text. Contient les tests.
- **`commands.rs`** — commandes `#[tauri::command]` exposées au frontend.

### Frontend (`src/`) — TypeScript vanilla, sans framework

- **`api.ts`** — seul fichier qui touche aux API Tauri : wrappers typés de chaque `invoke` et `listen`. L'interface `Item` doit rester le miroir de `db::Item`.
- **`main.ts`** — toute la logique UI (liste, recherche, onglets, navigation clavier, modales, menu ☰). Manipulation directe du DOM.

### Règles de câblage à respecter

- **Ajouter une commande = 3 endroits** : la fonction dans `commands.rs`, son enregistrement dans `invoke_handler` de `lib.rs`, et son wrapper dans `api.ts`.
- **Toute écriture dans le presse-papiers passe par le thread watcher** (`WatcherMsg::Write` via `watcher_tx`, avec ack) — jamais d'accès direct à arboard ailleurs, sinon le contenu serait réinséré dans l'historique par le poll suivant.
- **Après tout changement de favoris ou de dossiers**, appeler `refresh_tray_menu` (les opérations de menu doivent tourner sur le thread principal).
- Backend → frontend par événements : `clipboard-changed`, `window-shown`, `show-about`.
- Les verrous sur la base utilisent `lock().unwrap_or_else(|e| e.into_inner())` pour survivre à un mutex empoisonné.

## Release

La version existe dans **3 fichiers à bumper ensemble** : `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.

Pousser un tag `vX.Y.Z` déclenche `release.yml` : build Windows, signature des artefacts de mise à jour (secrets `TAURI_SIGNING_PRIVATE_KEY` / `..._PASSWORD`), publication d'une Release GitHub avec `latest.json` — le manifeste lu au démarrage par tauri-plugin-updater côté clients.
