use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Nombre maximum d'éléments non épinglés conservés dans l'historique.
pub const MAX_ITEMS: i64 = 100;

#[derive(Serialize, Debug, Clone)]
pub struct Item {
    pub id: i64,
    pub content: String,
    pub pinned: bool,
    pub category_id: Option<i64>,
    pub created_at: i64,
    pub use_count: i64,
}

#[derive(Serialize, Debug, Clone)]
pub struct Category {
    pub id: i64,
    pub name: String,
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
        CREATE TABLE IF NOT EXISTS categories (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            position INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS items (
            id INTEGER PRIMARY KEY,
            content TEXT NOT NULL,
            hash TEXT NOT NULL UNIQUE,
            pinned INTEGER NOT NULL DEFAULT 0,
            category_id INTEGER REFERENCES categories(id) ON DELETE SET NULL,
            created_at INTEGER NOT NULL,
            use_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_items_order ON items(pinned DESC, created_at DESC);
        INSERT OR IGNORE INTO categories (name, position) VALUES ('Travail', 1);
        INSERT OR IGNORE INTO categories (name, position) VALUES ('Personnel', 2);
        INSERT OR IGNORE INTO categories (name, position) VALUES ('Code', 3);",
    )
}

/// Insère un nouveau contenu, ou remonte l'élément existant en tête
/// d'historique s'il a déjà été copié (dédoublonnage par hash).
pub fn insert_or_touch(conn: &Connection, content: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO items (content, hash, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(hash) DO UPDATE SET created_at = excluded.created_at",
        params![content, hash_content(content), now],
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

/// Liste les éléments : épinglés d'abord, puis du plus récent au plus ancien.
/// `search` filtre sur le contenu, `category` sur la catégorie.
pub fn list_items(
    conn: &Connection,
    search: Option<&str>,
    category: Option<i64>,
) -> rusqlite::Result<Vec<Item>> {
    let pattern = search
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)));

    let mut sql = String::from(
        "SELECT id, content, pinned, category_id, created_at, use_count FROM items WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(p) = &pattern {
        sql.push_str(" AND content LIKE ? ESCAPE '\\'");
        args.push(Box::new(p.clone()));
    }
    if let Some(c) = category {
        sql.push_str(" AND category_id = ?");
        args.push(Box::new(c));
    }
    sql.push_str(" ORDER BY pinned DESC, created_at DESC, id DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())), |row| {
        Ok(Item {
            id: row.get(0)?,
            content: row.get(1)?,
            pinned: row.get::<_, i64>(2)? != 0,
            category_id: row.get(3)?,
            created_at: row.get(4)?,
            use_count: row.get(5)?,
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

pub fn toggle_pin(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET pinned = 1 - pinned WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn delete_item(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM items WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn set_category(conn: &Connection, id: i64, category: Option<i64>) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE items SET category_id = ?2 WHERE id = ?1",
        params![id, category],
    )?;
    Ok(())
}

pub fn list_categories(conn: &Connection) -> rusqlite::Result<Vec<Category>> {
    let mut stmt = conn.prepare("SELECT id, name FROM categories ORDER BY position, id")?;
    let rows = stmt.query_map([], |row| {
        Ok(Category {
            id: row.get(0)?,
            name: row.get(1)?,
        })
    })?;
    rows.collect()
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
        toggle_pin(&conn, pinned_id).unwrap();
        for i in 0..MAX_ITEMS + 20 {
            insert_or_touch(&conn, &format!("item {i}"), i + 1).unwrap();
        }
        let items = list_items(&conn, None, None).unwrap();
        assert_eq!(items.len() as i64, MAX_ITEMS + 1); // 100 + l'épinglé
        assert!(items[0].pinned); // l'épinglé survit et reste en tête
        assert_eq!(items[0].content, "épinglé");
        // les plus anciens non épinglés ont été purgés
        assert!(!items.iter().any(|i| i.content == "item 0"));
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
    fn category_assignment_and_filter() {
        let conn = open_in_memory().unwrap();
        let cats = list_categories(&conn).unwrap();
        assert_eq!(cats.len(), 3);
        let code = cats.iter().find(|c| c.name == "Code").unwrap().id;
        insert_or_touch(&conn, "fn main() {}", 1).unwrap();
        insert_or_touch(&conn, "coucou", 2).unwrap();
        let id = list_items(&conn, Some("main"), None).unwrap()[0].id;
        set_category(&conn, id, Some(code)).unwrap();
        let items = list_items(&conn, None, Some(code)).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].content, "fn main() {}");
    }

    #[test]
    fn delete_and_pin_toggle() {
        let conn = open_in_memory().unwrap();
        insert_or_touch(&conn, "temporaire", 1).unwrap();
        let id = list_items(&conn, None, None).unwrap()[0].id;
        toggle_pin(&conn, id).unwrap();
        assert!(list_items(&conn, None, None).unwrap()[0].pinned);
        toggle_pin(&conn, id).unwrap();
        assert!(!list_items(&conn, None, None).unwrap()[0].pinned);
        delete_item(&conn, id).unwrap();
        assert!(list_items(&conn, None, None).unwrap().is_empty());
    }
}
