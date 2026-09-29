//! Literal-query policy adapted independently for this experiment.
//! Wake idea/escaping coordinates: wake-core/src/db.rs:3248-3303 at
//! 71aeca67ec80f8645d1f9d5199290c2c732036ce; MIT notice: ../LICENSE-Wake.
//! No Wake module, ranking, GUI, connection pool, or transcript is imported.
use agent_session_grep_application::fts_tokens_cjk;
use rusqlite::{Connection, Statement, params_from_iter};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Sql {
    pub route: String,
    pub sql: String,
    pub parameters: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PlanRow {
    pub id: i64,
    pub parent: i64,
    pub detail: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Prepare,
    #[cfg(feature = "cache")]
    PrepareCached,
}

pub fn methods() -> Vec<Method> {
    vec![
        Method::Prepare,
        #[cfg(feature = "cache")]
        Method::PrepareCached,
    ]
}

/// Faithful independent adaptation of the private product safe_fts_query.
/// Keep transform BEFORE literalization and trim all terminal stars per token.
pub fn literal_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter_map(|raw| {
            let stem = raw.trim_end_matches('*').replace('"', "\"\"");
            if !stem.chars().any(char::is_alphanumeric) {
                return None;
            }
            let suffix = if raw.ends_with('*') { "*" } else { "" };
            Some(format!("\"{stem}\"{suffix}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn cjk(query: &str) -> Sql {
    let literal = literal_fts_query(&fts_tokens_cjk(query));
    if literal.is_empty() {
        return empty();
    }
    Sql {
        route: "cjk_unigram_bigram_literal_prefix".into(),
        sql: "SELECT rowid FROM cjk_fts WHERE cjk_fts MATCH ?1 ORDER BY bm25(cjk_fts), rowid LIMIT 10".into(),
        parameters: vec![literal],
    }
}

pub fn escape_like(term: &str) -> String {
    let mut result = String::new();
    for c in term.chars() {
        if matches!(c, '\\' | '%' | '_') {
            result.push('\\');
        }
        result.push(c);
    }
    result
}

pub fn trigram_like(query: &str) -> Sql {
    let terms: Vec<_> = query.split_whitespace().collect();
    if terms.is_empty() {
        return empty();
    }
    if terms.iter().any(|term| term.chars().count() < 3) {
        let predicates = (1..=terms.len())
            .map(|index| format!("text LIKE ?{index} ESCAPE '\\'"))
            .collect::<Vec<_>>()
            .join(" AND ");
        return Sql {
            route: "short_term_like_scan".into(),
            sql: format!("SELECT id FROM corpus WHERE {predicates} ORDER BY id LIMIT 10"),
            parameters: terms
                .iter()
                .map(|term| format!("%{}%", escape_like(term)))
                .collect(),
        };
    }
    Sql {
        route: "trigram_match".into(),
        sql: "SELECT rowid FROM trigram_fts WHERE trigram_fts MATCH ?1 ORDER BY bm25(trigram_fts), rowid LIMIT 10".into(),
        parameters: vec![terms.iter().map(|term| format!("\"{}\"", term.replace('"', "\"\""))).collect::<Vec<_>>().join(" AND ")],
    }
}

fn empty() -> Sql {
    Sql {
        route: "no_searchable_terms".into(),
        sql: "SELECT id FROM corpus WHERE 0 ORDER BY id LIMIT 10".into(),
        parameters: vec![],
    }
}

pub fn plan(conn: &Connection, sql: &Sql) -> rusqlite::Result<Vec<PlanRow>> {
    conn.prepare(&format!("EXPLAIN QUERY PLAN {}", sql.sql))?
        .query_map(params_from_iter(sql.parameters.iter()), |row| {
            Ok(PlanRow {
                id: row.get(0)?,
                parent: row.get(1)?,
                detail: row.get(3)?,
            })
        })?
        .collect()
}

fn read_ids(statement: &mut Statement<'_>, parameters: &[String]) -> rusqlite::Result<Vec<u64>> {
    statement
        .query_map(params_from_iter(parameters.iter()), |row| {
            row.get::<_, i64>(0).map(|value| value as u64)
        })?
        .collect()
}

pub fn execute(conn: &Connection, sql: &Sql, method: Method) -> rusqlite::Result<Vec<u64>> {
    match method {
        Method::Prepare => read_ids(&mut conn.prepare(&sql.sql)?, &sql.parameters),
        #[cfg(feature = "cache")]
        Method::PrepareCached => {
            let mut statement = conn.prepare_cached(&sql.sql)?;
            read_ids(&mut statement, &sql.parameters)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literalization_matches_private_product_policy() {
        assert_eq!(
            literal_fts_query("OR NEAR(foo) src/main.rs module::sym parse_error q\"x foo*** * %"),
            "\"OR\" \"NEAR(foo)\" \"src/main.rs\" \"module::sym\" \"parse_error\" \"q\"\"x\" \"foo\"*"
        );
        assert_eq!(fts_tokens_cjk("配置备份"), "配 置 备 份 配置 置备 备份");
        assert_eq!(cjk("配置").parameters, ["\"配\" \"置\" \"配置\""]);
        // The product's Han/non-Han split separates this star before quoting.
        assert_eq!(cjk("配置*").parameters, cjk("配置").parameters);
        assert_eq!(cjk("galeword*").parameters, ["\"galeword\"*"]);
        assert!(cjk("% _ 🧭").parameters.is_empty());
    }

    #[test]
    fn short_route_uses_scalar_count_and_escapes_wildcards() {
        assert_eq!(trigram_like("界").route, "short_term_like_scan");
        assert_eq!(trigram_like("配置").route, "short_term_like_scan");
        assert_eq!(trigram_like("数据库").route, "trigram_match");
        assert_eq!(trigram_like("abcdef p_").parameters, ["%abcdef%", "%p\\_%"]);
        assert_eq!(
            trigram_like("\\ % _").parameters,
            ["%\\\\%", "%\\%%", "%\\_%"]
        );
        assert_eq!(trigram_like("galeword*").parameters, ["\"galeword*\""]);
        assert_eq!(
            trigram_like("quoted\"mark").parameters,
            ["\"quoted\"\"mark\""]
        );
    }
}
