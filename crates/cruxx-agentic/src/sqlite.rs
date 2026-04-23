//! SQLite step handlers for cruxx-script pipelines.
//!
//! All handlers share the arg shape:
//! `{ "db": "<path>", "sql": "<query>", "params": { ":name": "value" } }`

use cruxx_core::prelude::CruxErr;
use cruxx_script::HandlerRegistry;
use rusqlite::{Connection, types::Value as SqlValue};
use serde_json::{Map, Value, json};

use crate::error::{AgenticError, require_str};

// ── internal helpers ─────────────────────────────────────────────────────────

fn open(input: &Value) -> Result<Connection, AgenticError> {
    let db_path = require_str(input, "db")?;
    Connection::open(db_path)
        .map_err(|e| AgenticError::Other(format!("sqlite open {db_path}: {e}")))
}

fn require_sql(input: &Value) -> Result<String, AgenticError> {
    require_str(input, "sql").map(|s| s.to_string())
}

/// Convert a JSON params object `{ ":name": value }` to rusqlite param pairs.
fn json_params(input: &Value) -> Vec<(String, SqlValue)> {
    let Some(obj) = input
        .get("args")
        .and_then(|a| a.get("params"))
        .and_then(|p| p.as_object())
    else {
        return vec![];
    };
    obj.iter()
        .map(|(k, v)| {
            let sql_val = match v {
                Value::Null => SqlValue::Null,
                Value::Bool(b) => SqlValue::Integer(*b as i64),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        SqlValue::Integer(i)
                    } else {
                        SqlValue::Real(n.as_f64().unwrap_or(0.0))
                    }
                }
                Value::String(s) => SqlValue::Text(s.clone()),
                _ => SqlValue::Null,
            };
            (k.clone(), sql_val)
        })
        .collect()
}

/// Execute a query and return all rows as a JSON array.
fn rows_to_json(
    conn: &Connection,
    sql: &str,
    params: &[(String, SqlValue)],
) -> Result<Vec<Value>, AgenticError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AgenticError::Other(format!("prepare: {e}")))?;

    let col_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
        .collect();

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let mut map = Map::new();
            for (i, name) in col_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    SqlValue::Null => Value::Null,
                    SqlValue::Integer(n) => json!(n),
                    SqlValue::Real(f) => json!(f),
                    SqlValue::Text(s) => Value::String(s),
                    SqlValue::Blob(b) => Value::String(hex_encode(&b)),
                };
                map.insert(name.clone(), json_val);
            }
            Ok(map)
        })
        .map_err(|e| AgenticError::Other(format!("query: {e}")))?;

    let mut result = vec![];
    for row in rows {
        result.push(Value::Object(
            row.map_err(|e| AgenticError::Other(format!("row: {e}")))?,
        ));
    }
    Ok(result)
}

fn hex_encode(data: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(data.len() * 2);
    for b in data {
        write!(s, "{b:02x}").ok();
    }
    s
}

fn to_crux(e: AgenticError) -> CruxErr {
    CruxErr::from(e)
}

fn exec_with_params(
    conn: &Connection,
    sql: &str,
    params: &[(String, SqlValue)],
    handler_name: &str,
) -> Result<usize, CruxErr> {
    let param_refs: Vec<(&str, &dyn rusqlite::ToSql)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v as &dyn rusqlite::ToSql))
        .collect();
    conn.execute(sql, param_refs.as_slice())
        .map_err(|e| CruxErr::step_failed(handler_name, e.to_string()))
}

// ── handler registration ──────────────────────────────────────────────────────

pub fn register(registry: &mut HandlerRegistry) {
    // sqlite::exec — DDL / fire-and-forget DML
    registry.handler_value("sqlite::exec", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows_affected = exec_with_params(&conn, &sql, &params, "sqlite::exec")?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::query_many — SELECT returning array
    registry.handler_value("sqlite::query_many", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows = rows_to_json(&conn, &sql, &params).map_err(to_crux)?;
        Ok(json!({ "rows": rows }))
    });

    // sqlite::query_one — SELECT expecting exactly one row
    registry.handler_value("sqlite::query_one", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let mut rows = rows_to_json(&conn, &sql, &params).map_err(to_crux)?;
        match rows.len() {
            0 => Err(CruxErr::step_failed("sqlite::query_one", "no rows returned")),
            1 => Ok(json!({ "row": rows.remove(0) })),
            n => Err(CruxErr::step_failed(
                "sqlite::query_one",
                format!("expected 1 row, got {n}"),
            )),
        }
    });

    // sqlite::insert
    registry.handler_value("sqlite::insert", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        exec_with_params(&conn, &sql, &params, "sqlite::insert")?;
        let rowid = conn.last_insert_rowid();
        Ok(json!({ "last_insert_rowid": rowid }))
    });

    // sqlite::update
    registry.handler_value("sqlite::update", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows_affected = exec_with_params(&conn, &sql, &params, "sqlite::update")?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::delete
    registry.handler_value("sqlite::delete", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows_affected = exec_with_params(&conn, &sql, &params, "sqlite::delete")?;
        Ok(json!({ "rows_affected": rows_affected }))
    });

    // sqlite::upsert — INSERT OR REPLACE
    registry.handler_value("sqlite::upsert", |input: Value| async move {
        let conn = open(&input).map_err(to_crux)?;
        let sql = require_sql(&input).map_err(to_crux)?;
        let params = json_params(&input);
        let rows_affected = exec_with_params(&conn, &sql, &params, "sqlite::upsert")?;
        Ok(json!({ "rows_affected": rows_affected }))
    });
}
