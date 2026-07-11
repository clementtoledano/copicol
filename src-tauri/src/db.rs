use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;

use crate::detect;

/// Nombre maximum d'éléments non épinglés conservés dans l'historique.
pub const MAX_ITEMS: i64 = 100;

#[derive(Serialize, Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub content: String,
    pub kind: String,
    pub pinned: bool,
    /// Nom descriptif donné par l'utilisateur au moment de l'épinglage.
    /// `None` pour les éléments jamais épinglés ; conservé après désépinglage.
    pub label: Option<String>,
    /// Dossier auquel appartient le favori ; `None` = « sans dossier ».
    /// N'a de sens que pour les favoris épinglés.
    pub group_id: Option<i64>,
    pub created_at: i64,
    pub use_count: i64,
}

/// Un dossier de favoris créé par l'utilisateur.
#[derive(Serialize, Debug, Clone)]
pub struct Group {
    pub id: i64,
    pub name: String,
}

// ── Import / export des favoris (fichier JSON) ──────────────────────

fn schema_version() -> u32 {
    1
}
fn default_kind() -> String {
    "text".to_string()
}

/// Contenu d'un fichier d'export : la liste des dossiers (par nom), les
/// favoris avec leur nom, catégorie, dossier et contenu, et les préférences
/// (clé/valeur, reflet de la table `settings`).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FavoritesFile {
    #[serde(default = "schema_version")]
    pub version: u32,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub favorites: Vec<FavoriteEntry>,
    #[serde(default)]
    pub settings: HashMap<String, String>,
}

/// Un favori dans le fichier d'export. `group` est le **nom** du dossier (pour
/// rester lisible et indépendant des identifiants internes).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FavoriteEntry {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub group: Option<String>,
    pub content: String,
}

/// Un type de contenu présent dans l'historique, avec son nombre d'éléments.
/// Alimente la rangée d'onglets automatiques du frontend.
#[derive(Serialize, Debug, Clone)]
pub struct KindCount {
    pub kind: String,
    pub count: i64,
}

pub fn hash_content(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    init(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS items (
            id INTEGER PRIMARY KEY,
            content TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            pinned INTEGER NOT NULL DEFAULT 0,
            kind TEXT NOT NULL DEFAULT 'text',
            label TEXT,
            group_id INTEGER,
            created_at INTEGER NOT NULL,
            use_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_items_order ON items(pinned DESC, created_at DESC);
        CREATE TABLE IF NOT EXISTS groups (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    migrate(conn)
}

/// Migration des bases V1 : ajoute la colonne `kind` si absente, puis
/// re-détecte le type de chaque élément existant (≤ MAX_ITEMS lignes).
/// La table `categories` et la colonne `category_id` V1 restent en place,
/// simplement plus utilisées.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let has_kind = conn
        .prepare("SELECT 1 FROM pragma_table_info('items') WHERE name = 'kind'")?
        .exists([])?;
    if !has_kind {
        conn.execute(
            "ALTER TABLE items ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'",
            [],
        )?;
        backfill_kinds(conn)?;
    }

    // Nom descriptif des favoris (nullable, aucune valeur à recalculer).
    let has_label = conn
        .prepare("SELECT 1 FROM pragma_table_info('items') WHERE name = 'label'")?
        .exists([])?;
    if !has_label {
        conn.execute("ALTER TABLE items ADD COLUMN label TEXT", [])?;
    }

    // Dossiers de favoris : colonne `group_id` sur items + table `groups`.
    let has_group = conn
        .prepare("SELECT 1 FROM pragma_table_info('items') WHERE name = 'group_id'")?
        .exists([])?;
    if !has_group {
        conn.execute("ALTER TABLE items ADD COLUMN group_id INTEGER", [])?;
    }
    conn.execute(
        "CREATE TABLE IF NOT EXISTS groups (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            created_at INTEGER NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

// ── Préférences (clé/valeur) ────────────────────────────────────────

fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| {
        r.get(0)
    })
    .optional()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Préférence d'interface (thème, densité…) : clé libre préfixée `ui_` en
/// base pour rester à l'écart des clés système comme `autostart_enabled`.
pub fn ui_pref(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    get_setting(conn, &format!("ui_{key}"))
}

/// Mémorise une préférence d'interface (voir `ui_pref`).
pub fn set_ui_pref(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    set_setting(conn, &format!("ui_{key}"), value)
}

/// Toutes les préférences, pour l'export (clé/valeur brut de la table `settings`).
fn list_settings(conn: &Connection) -> rusqlite::Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    rows.collect()
}

const AUTOSTART_KEY: &str = "autostart_enabled";

/// Préférence de démarrage automatique avec la session ; activée par défaut
/// (comportement historique) tant que l'utilisateur ne l'a pas explicitement
/// désactivée.
pub fn autostart_enabled(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(get_setting(conn, AUTOSTART_KEY)?.map(|v| v == "1").unwrap_or(true))
}

/// Mémorise le choix de l'utilisateur pour le démarrage automatique. Ce choix
/// prime sur toute nouvelle tentative d'activation au démarrage de l'app.
pub fn set_autostart_enabled(conn: &Connection, enabled: bool) -> rusqlite::Result<()> {
    set_setting(conn, AUTOSTART_KEY, if enabled { "1" } else { "0" })
}

fn backfill_kinds(conn: &Connection) -> rusqlite::Result<()> {
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT id, content FROM items")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (id, content) in rows {
        let kind = detect::detect(&content);
        if kind != "text" {
            conn.execute("UPDATE items SET kind = ?2 WHERE id = ?1", params![id, kind])?;
        }
    }
    Ok(())
}

/// Insère un nouveau contenu (type détecté automatiquement), ou remonte
/// l'élément existant en tête d'historique s'il a déjà été copié
/// (dédoublonnage par hash). Un favori (`pinned = 1`) garde sa catégorie
/// telle quelle : elle a pu être choisie manuellement via [`set_kind`], une
/// recopie ne doit pas l'écraser silencieusement.
pub fn insert_or_touch(conn: &Connection, content: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO items (content, hash, kind, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(hash) DO UPDATE SET created_at = excluded.created_at,
                                         kind = CASE WHEN pinned = 1 THEN kind ELSE excluded.kind END",
        params![content, hash_content(content), detect::detect(content), now],
    )?;
    prune(conn)?;
    Ok(())
}

/// Catégories reconnues par la détection automatique et proposées pour la
/// reclassification manuelle d'un favori.
pub const ALL_KINDS: [&str; 9] = [
    "sql", "code", "url", "email", "json", "color", "phone", "path", "text",
];

/// Change la catégorie d'un élément indépendamment de la détection
/// automatique (reclassification manuelle d'un favori).
pub fn set_kind(conn: &Connection, id: i64, kind: &str) -> rusqlite::Result<()> {
    if !ALL_KINDS.contains(&kind) {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "catégorie inconnue: {kind}"
        )));
    }
    conn.execute("UPDATE items SET kind = ?2 WHERE id = ?1", params![id, kind])?;
    Ok(())
}

/// Modifie le contenu d'un favori (nom et catégorie conservés). Le hash est
/// recalculé ; échoue si un autre élément a déjà exactement ce contenu
/// (contrainte d'unicité sur le hash).
pub fn update_content(conn: &Connection, id: i64, content: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET content = ?2, hash = ?3 WHERE id = ?1",
        params![id, content, hash_content(content)],
    )?;
    Ok(())
}

/// Supprime les éléments non épinglés au-delà de MAX_ITEMS (les plus anciens).
fn prune(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM items WHERE pinned = 0 AND id NOT IN (
            SELECT id FROM items WHERE pinned = 0
            ORDER BY created_at DESC, id DESC LIMIT ?1
        )",
        params![MAX_ITEMS],
    )?;
    Ok(())
}

fn escape_like(term: &str) -> String {
    term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

const ITEM_COLUMNS: &str = "id, content, kind, pinned, label, group_id, created_at, use_count";

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        content: row.get(1)?,
        kind: row.get(2)?,
        pinned: row.get::<_, i64>(3)? != 0,
        label: row.get(4)?,
        group_id: row.get(5)?,
        created_at: row.get(6)?,
        use_count: row.get(7)?,
    })
}

fn search_pattern(search: Option<&str>) -> Option<String> {
    search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)))
}

/// Historique : éléments **non épinglés**, du plus récent au plus ancien.
/// `search` filtre sur le contenu, `kind` sur le type détecté. Les favoris en
/// sont exclus : ils ont leur propre vue (voir [`list_favorites`]).
pub fn list_items(
    conn: &Connection,
    search: Option<&str>,
    kind: Option<&str>,
) -> rusqlite::Result<Vec<Item>> {
    let pattern = search_pattern(search);

    let mut sql = format!("SELECT {ITEM_COLUMNS} FROM items WHERE pinned = 0");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(p) = &pattern {
        sql.push_str(" AND content LIKE ? ESCAPE '\\'");
        args.push(Box::new(p.clone()));
    }
    if let Some(k) = kind {
        sql.push_str(" AND kind = ?");
        args.push(Box::new(k.to_string()));
    }
    sql.push_str(" ORDER BY created_at DESC, id DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
        row_to_item,
    )?;
    rows.collect()
}

/// Favoris : éléments **épinglés**, classés par nom (A→Z, insensible à la casse).
/// Les rares favoris hérités sans nom passent en dernier. `search` filtre sur le
/// nom **et** le contenu.
pub fn list_favorites(conn: &Connection, search: Option<&str>) -> rusqlite::Result<Vec<Item>> {
    let pattern = search_pattern(search);

    let mut sql = format!("SELECT {ITEM_COLUMNS} FROM items WHERE pinned = 1");
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(p) = &pattern {
        sql.push_str(" AND (content LIKE ? ESCAPE '\\' OR label LIKE ? ESCAPE '\\')");
        args.push(Box::new(p.clone()));
        args.push(Box::new(p.clone()));
    }
    sql.push_str(" ORDER BY label IS NULL, label COLLATE NOCASE, id");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
        row_to_item,
    )?;
    rows.collect()
}

/// Nombre de favoris (alimente le compteur de l'onglet ⭐ Favoris).
pub fn count_favorites(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM items WHERE pinned = 1", [], |r| r.get(0))
}

/// Types présents dans l'historique (favoris exclus) avec compteurs,
/// les plus fournis d'abord.
pub fn list_kinds(conn: &Connection) -> rusqlite::Result<Vec<KindCount>> {
    let mut stmt = conn.prepare(
        "SELECT kind, COUNT(*) FROM items WHERE pinned = 0 GROUP BY kind ORDER BY COUNT(*) DESC, kind",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(KindCount {
            kind: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    rows.collect()
}

pub fn get_content(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT content FROM items WHERE id = ?1", params![id], |row| {
        row.get(0)
    })
    .optional()
}

pub fn mark_used(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET use_count = use_count + 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Épingle l'élément et lui affecte son nom descriptif (obligatoire côté UI).
/// Sert aussi à renommer un favori existant : `pinned = 1` est alors idempotent.
pub fn pin_item(conn: &Connection, id: i64, label: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET pinned = 1, label = ?2 WHERE id = ?1",
        params![id, label],
    )?;
    Ok(())
}

/// Retire l'élément des favoris en conservant son nom descriptif,
/// de sorte qu'un ré-épinglage puisse le proposer par défaut.
pub fn unpin_item(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE items SET pinned = 0 WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn delete_item(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
    Ok(())
}

/// Vide l'historique : supprime tous les éléments **non épinglés**. Les favoris
/// (et donc les dossiers) sont conservés.
pub fn clear_history(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM items WHERE pinned = 0", [])?;
    Ok(())
}

// ── Dossiers de favoris ─────────────────────────────────────────────

/// Liste les dossiers, classés par nom (insensible à la casse).
pub fn list_groups(conn: &Connection) -> rusqlite::Result<Vec<Group>> {
    let mut stmt = conn.prepare("SELECT id, name FROM groups ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], |row| {
        Ok(Group {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    rows.collect()
}

/// Crée un dossier et renvoie son identifiant. Si le nom existe déjà
/// (insensible à la casse via la contrainte UNIQUE), renvoie l'id existant.
pub fn create_group(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO groups (name, created_at) VALUES (?1, ?2)
         ON CONFLICT(name) DO NOTHING",
        params![name, crate::watcher::now_unix()],
    )?;
    conn.query_row("SELECT id FROM groups WHERE name = ?1", params![name], |r| {
        r.get(0)
    })
}

/// Renomme un dossier.
pub fn rename_group(conn: &Connection, id: i64, name: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE groups SET name = ?2 WHERE id = ?1", params![id, name])?;
    Ok(())
}

/// Supprime un dossier ; ses favoris repassent « sans dossier » (group_id = NULL).
pub fn delete_group(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE items SET group_id = NULL WHERE group_id = ?1", params![id])?;
    conn.execute("DELETE FROM groups WHERE id = ?1", params![id])?;
    Ok(())
}

/// Affecte un favori à un dossier, ou l'en retire (`group_id = None`).
pub fn set_item_group(conn: &Connection, id: i64, group_id: Option<i64>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET group_id = ?2 WHERE id = ?1",
        params![id, group_id],
    )?;
    Ok(())
}

/// Insère un favori (épinglé) avec ses métadonnées explicites. Renvoie `true`
/// si le favori a été créé, `false` si un élément avec ce contenu existait déjà
/// (dédoublonnage par hash — sert à la fusion lors de l'import).
pub fn insert_favorite(
    conn: &Connection,
    content: &str,
    label: Option<&str>,
    kind: &str,
    group_id: Option<i64>,
    now: i64,
) -> rusqlite::Result<bool> {
    let changed = conn.execute(
        "INSERT INTO items (content, hash, kind, pinned, label, group_id, created_at)
         VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6)
         ON CONFLICT(hash) DO NOTHING",
        params![content, hash_content(content), kind, label, group_id, now],
    )?;
    Ok(changed > 0)
}

/// Construit le contenu exportable : dossiers (par nom), favoris (chacun avec
/// le **nom** de son dossier) et préférences (démarrage automatique…).
pub fn export_favorites(conn: &Connection) -> rusqlite::Result<FavoritesFile> {
    let groups = list_groups(conn)?;
    let name_by_id: HashMap<i64, String> =
        groups.iter().map(|g| (g.id, g.name.clone())).collect();
    let favorites = list_favorites(conn, None)?
        .into_iter()
        .map(|f| FavoriteEntry {
            group: f.group_id.and_then(|id| name_by_id.get(&id).cloned()),
            label: f.label,
            kind: f.kind,
            content: f.content,
        })
        .collect();
    Ok(FavoritesFile {
        version: schema_version(),
        groups: groups.into_iter().map(|g| g.name).collect(),
        favorites,
        settings: list_settings(conn)?,
    })
}

/// Fusionne les favoris d'un fichier importé : recrée les dossiers manquants,
/// ajoute les favoris dont le contenu n'existe pas déjà, et applique les
/// préférences du fichier (écrasent les valeurs locales). Renvoie
/// `(importés, ignorés)`. Une catégorie inconnue est re-détectée sur le contenu.
pub fn import_favorites(
    conn: &Connection,
    file: &FavoritesFile,
    now: i64,
) -> rusqlite::Result<(usize, usize)> {
    // Cache nom → id des dossiers, en créant ceux qui manquent.
    let mut group_id_by_name: HashMap<String, i64> = HashMap::new();
    let mut group_id_for = |conn: &Connection, name: &str| -> rusqlite::Result<i64> {
        if let Some(id) = group_id_by_name.get(name) {
            return Ok(*id);
        }
        let id = create_group(conn, name)?;
        group_id_by_name.insert(name.to_string(), id);
        Ok(id)
    };

    // Les dossiers explicitement listés sont créés même s'ils n'ont aucun favori.
    for name in &file.groups {
        let name = name.trim();
        if !name.is_empty() {
            group_id_for(conn, name)?;
        }
    }

    // Préférences : le fichier importé (une sauvegarde volontaire de
    // l'utilisateur) écrase les valeurs locales.
    for (key, value) in &file.settings {
        set_setting(conn, key, value)?;
    }

    let mut imported = 0;
    let mut skipped = 0;
    for fav in &file.favorites {
        if fav.content.trim().is_empty() {
            skipped += 1;
            continue;
        }
        let group_id = match fav.group.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => Some(group_id_for(conn, name)?),
            _ => None,
        };
        let kind = if ALL_KINDS.contains(&fav.kind.as_str()) {
            fav.kind.clone()
        } else {
            detect::detect(&fav.content).to_string()
        };
        let label = fav.label.as_deref().filter(|s| !s.trim().is_empty());
        if insert_favorite(conn, &fav.content, label, &kind, group_id, now)? {
            imported += 1;
        } else {
            skipped += 1;
        }
    }
    Ok((imported, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_list() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "hello", 1).unwrap();
        insert_or_touch(&conn, "world", 2).unwrap();
        let items = list_items(&conn, None, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].content, "world"); // le plus récent d'abord
        assert_eq!(items[1].content, "hello");
    }

    #[test]
    fn dedupe_by_hash_moves_to_top() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "aaa", 1).unwrap();
        insert_or_touch(&conn, "bbb", 2).unwrap();
        insert_or_touch(&conn, "aaa", 3).unwrap(); // recopie du même contenu
        let items = list_items(&conn, None, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].content, "aaa"); // remonté en tête, pas dupliqué
    }

    #[test]
    fn prune_keeps_max_items_and_pinned() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "épinglé", 0).unwrap();
        let pinned_id = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, pinned_id, "mon favori").unwrap();
        for i in 0..MAX_ITEMS + 20 {
            insert_or_touch(&conn, &format!("item {i}"), i + 1).unwrap();
        }
        // L'historique (non épinglés) est plafonné à MAX_ITEMS ; le favori en est exclu
        let items = list_items(&conn, None, None).unwrap();
        assert_eq!(items.len() as i64, MAX_ITEMS);
        assert!(items.iter().all(|i| !i.pinned));
        assert!(!items.iter().any(|i| i.content == "item 0")); // le plus ancien purgé
        // Le favori survit à la purge, dans sa propre vue
        let favs = list_favorites(&conn, None).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].content, "épinglé");
    }

    #[test]
    fn search_filters_content() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "facture Dupont", 1).unwrap();
        insert_or_touch(&conn, "mail de réponse", 2).unwrap();
        let items = list_items(&conn, Some("fact"), None).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "facture Dupont");
        // les jokers SQL sont neutralisés
        let items = list_items(&conn, Some("%"), None).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn kind_detected_on_insert_and_filterable() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "select * from TTracteur where FTracteur_Code = '000079'", 1)
            .unwrap();
        insert_or_touch(&conn, "https://example.com", 2).unwrap();
        insert_or_touch(&conn, "du texte normal", 3).unwrap();

        let sql_items = list_items(&conn, None, Some("sql")).unwrap();
        assert_eq!(sql_items.len(), 1);
        assert!(sql_items[0].content.starts_with("select"));
        assert_eq!(sql_items[0].kind, "sql");

        let url_items = list_items(&conn, None, Some("url")).unwrap();
        assert_eq!(url_items.len(), 1);
    }

    #[test]
    fn list_kinds_counts() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "select id from a", 1).unwrap();
        insert_or_touch(&conn, "select id from b", 2).unwrap();
        insert_or_touch(&conn, "https://example.com", 3).unwrap();
        let kinds = list_kinds(&conn).unwrap();
        assert_eq!(kinds.len(), 2);
        assert_eq!(kinds[0].kind, "sql"); // le plus fourni d'abord
        assert_eq!(kinds[0].count, 2);
        assert_eq!(kinds[1].kind, "url");
        assert_eq!(kinds[1].count, 1);
    }

    #[test]
    fn migrates_v1_database_and_backfills_kinds() {
        // Simule une base V1 : table items sans colonne kind
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
                id INTEGER PRIMARY KEY,
                content TEXT NOT NULL,
                hash TEXT NOT NULL UNIQUE,
                pinned INTEGER NOT NULL DEFAULT 0,
                category_id INTEGER,
                created_at INTEGER NOT NULL,
                use_count INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (content, hash, created_at) VALUES (?1, ?2, 1)",
            params!["select * from TVehicule where IdTracteur = 13236", "h1"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO items (content, hash, created_at) VALUES (?1, ?2, 2)",
            params!["note de réunion", "h2"],
        )
        .unwrap();

        init(&conn).unwrap(); // migration + backfill

        let items = list_items(&conn, None, None).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, "text");
        assert_eq!(items[1].kind, "sql"); // re-détecté par le backfill
    }

    #[test]
    fn pin_moves_out_of_history_unpin_keeps_label_and_delete() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "temporaire", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;

        // Épinglé → quitte l'historique, entre dans les favoris
        pin_item(&conn, id, "Mon modèle").unwrap();
        assert!(list_items(&conn, None, None).unwrap().is_empty());
        let fav = list_favorites(&conn, None).unwrap();
        assert_eq!(fav.len(), 1);
        assert!(fav[0].pinned);
        assert_eq!(fav[0].label.as_deref(), Some("Mon modèle"));

        // Désépinglé → revient dans l'historique, nom conservé
        unpin_item(&conn, id).unwrap();
        assert!(list_favorites(&conn, None).unwrap().is_empty());
        let hist = list_items(&conn, None, None).unwrap();
        assert_eq!(hist.len(), 1);
        assert!(!hist[0].pinned);
        assert_eq!(hist[0].label.as_deref(), Some("Mon modèle"));

        // pin_item sert aussi à renommer
        pin_item(&conn, id, "Renommé").unwrap();
        assert_eq!(list_favorites(&conn, None).unwrap()[0].label.as_deref(), Some("Renommé"));

        delete_item(&conn, id).unwrap();
        assert!(list_favorites(&conn, None).unwrap().is_empty());
        assert!(list_items(&conn, None, None).unwrap().is_empty());
    }

    #[test]
    fn favorites_search_by_label_and_content() {
        let conn = open_in_memory().unwrap();
        // Un favori nommé « parc », dont le contenu ne contient pas le terme
        insert_or_touch(&conn, "select * from TTracteur where id = 13236", 1).unwrap();
        let fav_id = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, fav_id, "Requête parc tracteurs").unwrap();
        // Un élément d'historique dont le contenu contient « parc »
        insert_or_touch(&conn, "note sur le parc automobile", 2).unwrap();

        // La recherche dans les favoris matche le nom
        let favs = list_favorites(&conn, Some("parc")).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].id, fav_id);
        // Terme présent seulement dans le nom
        assert_eq!(list_favorites(&conn, Some("tracteurs")).unwrap().len(), 1);

        // L'historique ne cherche que le contenu et exclut les favoris
        let hist = list_items(&conn, Some("parc"), None).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].content, "note sur le parc automobile");
    }

    #[test]
    fn favorites_excluded_from_kinds_and_history_filter() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "select id from a", 1).unwrap();
        let sql_id = list_items(&conn, None, None).unwrap()[0].id;
        insert_or_touch(&conn, "https://example.com", 2).unwrap();
        pin_item(&conn, sql_id, "Requête A").unwrap();

        // Le favori SQL ne compte plus dans les onglets par type
        let kinds = list_kinds(&conn).unwrap();
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0].kind, "url");
        assert_eq!(count_favorites(&conn).unwrap(), 1);
        // Filtrer l'historique par type n'affiche pas le favori
        assert!(list_items(&conn, None, Some("sql")).unwrap().is_empty());
    }

    #[test]
    fn set_kind_reclassifies_manually_and_rejects_unknown() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "select id from a", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;
        assert_eq!(list_items(&conn, None, None).unwrap()[0].kind, "sql");

        set_kind(&conn, id, "text").unwrap();
        assert_eq!(list_items(&conn, None, None).unwrap()[0].kind, "text");

        assert!(set_kind(&conn, id, "bogus").is_err());
        // La tentative invalide n'a pas modifié la catégorie
        assert_eq!(list_items(&conn, None, None).unwrap()[0].kind, "text");
    }

    #[test]
    fn pinned_item_keeps_manual_kind_on_recopy() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "select id from a", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, id, "Ma requête").unwrap();
        set_kind(&conn, id, "text").unwrap(); // reclassification manuelle

        // Recopie du même contenu : la détection donnerait "sql", mais le
        // favori conserve sa catégorie choisie manuellement.
        insert_or_touch(&conn, "select id from a", 2).unwrap();
        assert_eq!(list_favorites(&conn, None).unwrap()[0].kind, "text");
    }

    #[test]
    fn update_content_changes_content_and_hash() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "brouillon", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, id, "Mon favori").unwrap();

        update_content(&conn, id, "version finale").unwrap();
        let fav = list_favorites(&conn, None).unwrap();
        assert_eq!(fav[0].content, "version finale");
        assert_eq!(fav[0].label.as_deref(), Some("Mon favori"));

        // Le nouveau contenu peut de nouveau être recopié sans conflit de hash
        insert_or_touch(&conn, "version finale", 2).unwrap();
    }

    #[test]
    fn groups_crud_and_membership() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "requête A", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, id, "Requête A").unwrap();

        // Création (idempotente sur le nom)
        let g1 = create_group(&conn, "SQL").unwrap();
        assert_eq!(create_group(&conn, "SQL").unwrap(), g1); // même nom → même id
        assert_eq!(list_groups(&conn).unwrap().len(), 1);

        // Affectation
        set_item_group(&conn, id, Some(g1)).unwrap();
        assert_eq!(list_favorites(&conn, None).unwrap()[0].group_id, Some(g1));

        // Renommage
        rename_group(&conn, g1, "Requêtes SQL").unwrap();
        assert_eq!(list_groups(&conn).unwrap()[0].name, "Requêtes SQL");

        // Suppression du dossier : le favori survit, sans dossier
        delete_group(&conn, g1).unwrap();
        assert!(list_groups(&conn).unwrap().is_empty());
        let fav = list_favorites(&conn, None).unwrap();
        assert_eq!(fav.len(), 1);
        assert_eq!(fav[0].group_id, None);
    }

    #[test]
    fn export_import_roundtrip_with_groups_and_dedupe() {
        let src = open_in_memory().unwrap();
        insert_or_touch(&src, "select * from a", 1).unwrap();
        let id = list_items(&src, None, None).unwrap()[0].id;
        pin_item(&src, id, "Requête A").unwrap();
        let g = create_group(&src, "SQL").unwrap();
        set_item_group(&src, id, Some(g)).unwrap();
        // Un dossier vide doit aussi être exporté
        create_group(&src, "Vide").unwrap();
        // Une préférence non par défaut doit aussi être exportée
        set_autostart_enabled(&src, false).unwrap();

        let file = export_favorites(&src).unwrap();
        assert_eq!(file.favorites.len(), 1);
        assert_eq!(file.favorites[0].group.as_deref(), Some("SQL"));
        assert!(file.groups.iter().any(|n| n == "Vide"));
        assert_eq!(file.settings.get("autostart_enabled").map(String::as_str), Some("0"));

        // Import dans une base neuve : recrée le dossier, range le favori dedans
        // et applique la préférence exportée
        let dst = open_in_memory().unwrap();
        assert!(autostart_enabled(&dst).unwrap()); // valeur par défaut avant import
        let (imported, skipped) = import_favorites(&dst, &file, 10).unwrap();
        assert_eq!((imported, skipped), (1, 0));
        assert!(!autostart_enabled(&dst).unwrap()); // écrasée par le fichier importé
        let favs = list_favorites(&dst, None).unwrap();
        assert_eq!(favs.len(), 1);
        assert_eq!(favs[0].label.as_deref(), Some("Requête A"));
        assert_eq!(favs[0].kind, "sql");
        let groups = list_groups(&dst).unwrap();
        let sql_id = groups.iter().find(|x| x.name == "SQL").map(|x| x.id);
        assert_eq!(favs[0].group_id, sql_id);
        assert!(groups.iter().any(|x| x.name == "Vide"));

        // Réimport : le contenu existe déjà → ignoré (fusion)
        let (imported2, skipped2) = import_favorites(&dst, &file, 11).unwrap();
        assert_eq!((imported2, skipped2), (0, 1));
        assert_eq!(list_favorites(&dst, None).unwrap().len(), 1);
    }

    #[test]
    fn clear_history_keeps_favorites() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "jetable", 1).unwrap();
        insert_or_touch(&conn, "à garder", 2).unwrap();
        let keep = list_items(&conn, None, None).unwrap()[0].id;
        pin_item(&conn, keep, "Gardé").unwrap();

        clear_history(&conn).unwrap();
        assert!(list_items(&conn, None, None).unwrap().is_empty());
        assert_eq!(list_favorites(&conn, None).unwrap().len(), 1);
    }

    #[test]
    fn favorites_sorted_by_label_ignoring_case() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "x", 1).unwrap();
        insert_or_touch(&conn, "y", 2).unwrap();
        insert_or_touch(&conn, "z", 3).unwrap();
        let items = list_items(&conn, None, None).unwrap(); // z, y, x (plus récent d'abord)
        pin_item(&conn, items[0].id, "banane").unwrap();
        pin_item(&conn, items[1].id, "Ananas").unwrap();
        pin_item(&conn, items[2].id, "cerise").unwrap();
        let labels: Vec<String> = list_favorites(&conn, None)
            .unwrap()
            .iter()
            .map(|f| f.label.clone().unwrap())
            .collect();
        assert_eq!(labels, ["Ananas", "banane", "cerise"]); // A→Z, insensible à la casse
    }

    #[test]
    fn autostart_preference_defaults_true_and_persists() {
        let conn = open_in_memory().unwrap();
        assert!(autostart_enabled(&conn).unwrap()); // activé par défaut

        set_autostart_enabled(&conn, false).unwrap();
        assert!(!autostart_enabled(&conn).unwrap());

        set_autostart_enabled(&conn, true).unwrap();
        assert!(autostart_enabled(&conn).unwrap());
    }
}
