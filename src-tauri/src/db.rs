use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
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
    pub created_at: i64,
    pub use_count: i64,
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
            created_at INTEGER NOT NULL,
            use_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_items_order ON items(pinned DESC, created_at DESC);",
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
    Ok(())
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
/// (dédoublonnage par hash).
pub fn insert_or_touch(conn: &Connection, content: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO items (content, hash, kind, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(hash) DO UPDATE SET created_at = excluded.created_at,
                                         kind = excluded.kind",
        params![content, hash_content(content), detect::detect(content), now],
    )?;
    prune(conn)?;
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

const ITEM_COLUMNS: &str = "id, content, kind, pinned, label, created_at, use_count";

fn row_to_item(row: &rusqlite::Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: row.get(0)?,
        content: row.get(1)?,
        kind: row.get(2)?,
        pinned: row.get::<_, i64>(3)? != 0,
        label: row.get(4)?,
        created_at: row.get(5)?,
        use_count: row.get(6)?,
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
}
