use regex::Regex;
use std::sync::LazyLock;

static EMAIL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$").unwrap()
});
static COLOR_HEX_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$").unwrap()
});
static COLOR_RGB_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^rgba?\(\s*\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}\s*(?:,\s*[0-9.]+\s*)?\)$").unwrap()
});
static WIN_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[A-Za-z]:\\|\\\\)[^\r\n]+$").unwrap());
static IP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{1,3}(?:\.\d{1,3}){3}$").unwrap());
static CODE_KEYWORD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(fn|def|function|class|import|using|namespace|let|const|var|public|private|return|void|if|else|for|while)\b").unwrap()
});
static SQL_SECONDARY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(from|where|join|order\s+by|group\s+by|values|set|into)\b").unwrap()
});

/// Détecte le type d'un contenu copié. Première règle qui matche.
/// Kinds possibles : url, email, color, json, path, phone, sql, code, text.
pub fn detect(content: &str) -> &'static str {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "text";
    }
    let single_line = !trimmed.contains('\n');

    if single_line && is_url(trimmed) {
        return "url";
    }
    if single_line && EMAIL_RE.is_match(trimmed) {
        return "email";
    }
    if single_line && (COLOR_HEX_RE.is_match(trimmed) || COLOR_RGB_RE.is_match(trimmed)) {
        return "color";
    }
    if is_json(trimmed) {
        return "json";
    }
    if single_line && is_path(trimmed) {
        return "path";
    }
    if single_line && is_phone(trimmed) {
        return "phone";
    }
    if is_sql(trimmed) {
        return "sql";
    }
    if is_code(trimmed) {
        return "code";
    }
    "text"
}

fn is_url(s: &str) -> bool {
    if s.contains(char::is_whitespace) {
        return false;
    }
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.")
}

fn is_json(s: &str) -> bool {
    (s.starts_with('{') || s.starts_with('[')) && serde_json::from_str::<serde_json::Value>(s).is_ok()
}

fn is_path(s: &str) -> bool {
    if WIN_PATH_RE.is_match(s) {
        return true;
    }
    // Chemin Unix absolu : sans espace pour éviter la prose commençant par « / »
    s.len() > 1 && s.starts_with('/') && !s.contains(char::is_whitespace)
}

fn is_phone(s: &str) -> bool {
    if IP_RE.is_match(s) {
        return false; // une adresse IP n'est pas un téléphone
    }
    let digits = s.chars().filter(|c| c.is_ascii_digit()).count();
    (7..=15).contains(&digits)
        && s.chars()
            .all(|c| c.is_ascii_digit() || " +().-".contains(c))
}

/// SQL : le contenu (ou sa première ligne utile) commence par un verbe SQL,
/// avec les compléments attendus — assez strict pour ne pas classer de la
/// prose contenant « select » ou « update ».
fn is_sql(s: &str) -> bool {
    let lower = s.to_lowercase();
    let first_word = lower.split_whitespace().next().unwrap_or("");
    let second_word = lower.split_whitespace().nth(1).unwrap_or("");
    let has = |kw: &str| {
        SQL_SECONDARY_RE
            .find_iter(&lower)
            .any(|m| m.as_str().split_whitespace().next() == Some(kw))
    };

    match first_word {
        "select" | "with" => has("from"),
        "insert" => has("into"),
        // Pour update/delete, exiger un signe d'expression SQL (= ou quote)
        // afin d'écarter la prose du genre « update the docs where needed »
        "update" => has("set") && (lower.contains('=') || lower.contains('\'')),
        "delete" => has("from") && (lower.contains('=') || lower.contains('\'')),
        "create" | "alter" | "drop" | "truncate" => matches!(
            second_word,
            "table" | "view" | "index" | "database" | "procedure" | "function" | "trigger"
        ),
        _ => false,
    }
}

/// Code : heuristique par score (accolades, mots-clés, points-virgules,
/// indentation, opérateurs). Seuil à 3 pour éviter les faux positifs.
fn is_code(s: &str) -> bool {
    let mut score = 0u32;
    let multiline = s.contains('\n');

    if multiline && s.contains('{') && s.contains('}') {
        score += 1;
    }
    let keywords: std::collections::HashSet<&str> = CODE_KEYWORD_RE
        .find_iter(s)
        .map(|m| m.as_str())
        .collect();
    score += (keywords.len() as u32).min(2);
    if s.lines().filter(|l| l.trim_end().ends_with(';')).count() >= 2 {
        score += 1;
    }
    if s.lines().any(|l| l.starts_with("    ") || l.starts_with('\t')) {
        score += 1;
    }
    if s.contains("=>") || s.contains("->") || s.contains("==") || s.contains("!=") {
        score += 1;
    }

    score >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_user_sql_example() {
        // L'exemple réel donné par l'utilisateur
        let sql = "select * from TTracteur where FTracteur_Code = '000079'\n\n\
                   select * from TVehicule where IdTracteur = 13236\n\
                   select * from TVehiculeSociete where idVehicule = 16216\n\n\
                   select * from TTicketAtt where IdVehicule = 16216\n\n\
                   select * from TTicket where IdSiteUti = 5921 and IdVehicule =152312 order by Id desc";
        assert_eq!(detect(sql), "sql");
    }

    #[test]
    fn detects_sql_variants() {
        assert_eq!(detect("SELECT id FROM users"), "sql");
        assert_eq!(detect("insert into logs (msg) values ('ok')"), "sql");
        assert_eq!(detect("UPDATE TVehicule SET IdTracteur = 13236 WHERE Id = 1"), "sql");
        assert_eq!(detect("delete from TTicket where Id = 5"), "sql");
        assert_eq!(detect("CREATE TABLE foo (id INTEGER)"), "sql");
    }

    #[test]
    fn prose_with_sql_words_is_text() {
        assert_eq!(detect("Please select an option from the menu"), "text");
        assert_eq!(detect("update the docs where needed and set the flag"), "text");
        assert_eq!(detect("I will delete the folder from my desktop"), "text");
    }

    #[test]
    fn detects_url() {
        assert_eq!(detect("https://example.com/page?q=1"), "url");
        assert_eq!(detect("www.google.fr"), "url");
        assert_eq!(detect("visite https://example.com stp"), "text");
    }

    #[test]
    fn detects_email() {
        assert_eq!(detect("clement.toledano@gmail.com"), "email");
        assert_eq!(detect("pas un email @ du tout"), "text");
    }

    #[test]
    fn detects_color() {
        assert_eq!(detect("#ff8800"), "color");
        assert_eq!(detect("#f80"), "color");
        assert_eq!(detect("#ff880080"), "color");
        assert_eq!(detect("rgb(255, 136, 0)"), "color");
        assert_eq!(detect("#zzz"), "text");
    }

    #[test]
    fn detects_json() {
        assert_eq!(detect(r#"{"name": "copicol", "v": 2}"#), "json");
        assert_eq!(detect("[1, 2, 3]"), "json");
        assert_eq!(detect("{ pas du json }"), "text");
    }

    #[test]
    fn detects_path() {
        assert_eq!(detect(r"C:\Program Files\copicol\app.exe"), "path");
        assert_eq!(detect(r"\\serveur\partage\doc.pdf"), "path");
        assert_eq!(detect("/usr/local/bin/copicol"), "path");
        assert_eq!(detect("/ divisé par deux"), "text");
    }

    #[test]
    fn detects_phone() {
        assert_eq!(detect("06 12 34 56 78"), "phone");
        assert_eq!(detect("+33 6 12 34 56 78"), "phone");
        assert_eq!(detect("13236"), "text"); // trop court
        assert_eq!(detect("192.168.1.100"), "text"); // IP, pas téléphone
    }

    #[test]
    fn detects_code() {
        let js = "const x = 5;\nfunction foo() {\n  return x;\n}";
        assert_eq!(detect(js), "code");
        let py = "def hello():\n    return 42";
        assert_eq!(detect(py), "code");
        let cs = "public class Foo {\n    private int _bar;\n    public void Baz() { _bar = 1; }\n}";
        assert_eq!(detect(cs), "code");
    }

    #[test]
    fn plain_text_fallback() {
        assert_eq!(detect("Bonjour, voici le compte-rendu de la réunion."), "text");
        assert_eq!(detect(""), "text");
    }
}
