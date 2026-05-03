
//! MySQL protocol transport using the `mysql` crate (sync), run via spawn_blocking.

use anyhow::{Context, Result};
use mysql::{Opts, OptsBuilder, Pool};
use crate::config::Profile;

pub fn pool(profile: &Profile) -> Result<Pool> {
    let password = profile.password.as_deref().unwrap_or("");
    let opts = OptsBuilder::new()
        .ip_or_hostname(Some(&profile.fe_host))
        .tcp_port(profile.mysql_port)
        .user(Some(&profile.user))
        .pass(Some(password))
        .db_name(profile.database.as_deref());

    let opts = Opts::from(opts);
    Pool::new(opts).with_context(|| format!(
        "Cannot connect to Doris MySQL at {}:{}. \
         Check profile with `dorisctl profile list`.",
        profile.fe_host, profile.mysql_port
    ))
}

/// A lightweight result row: column names + string-ified values.
#[derive(Debug)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub rows_affected: u64,
}

/// Execute a query that returns rows (SELECT, SHOW, DESCRIBE …).
pub fn query(pool: &Pool, sql: &str) -> Result<QueryResult> {
    use mysql::prelude::Queryable;
    let mut conn = pool.get_conn()?;
    let result = conn.query_iter(sql)
        .with_context(|| format!("Query failed: {}", &sql[..sql.len().min(200)]))?;

    let columns: Vec<String> = result
        .columns()
        .as_ref()
        .iter()
        .map(|c| c.name_str().to_string())
        .collect();

    let mut rows: Vec<Vec<String>> = Vec::new();
    for row in result {
        let row = row?;
        let vals: Vec<String> = (0..columns.len())
            .map(|i| {
                let v: mysql::Value = row.get(i).unwrap_or(mysql::Value::NULL);
                value_to_string(v)
            })
            .collect();
        rows.push(vals);
    }

    Ok(QueryResult { columns, rows, rows_affected: 0 })
}

/// Execute a statement that doesn't return rows (DDL, DML).
pub fn execute(pool: &Pool, sql: &str) -> Result<u64> {
    use mysql::prelude::Queryable;
    let mut conn = pool.get_conn()?;
    conn.query_drop(sql)
        .with_context(|| format!("Execute failed: {}", &sql[..sql.len().min(200)]))?;
    Ok(0)
}

fn value_to_string(v: mysql::Value) -> String {
    match v {
        mysql::Value::NULL => "NULL".to_string(),
        mysql::Value::Bytes(b) => String::from_utf8_lossy(&b).to_string(),
        mysql::Value::Int(i) => i.to_string(),
        mysql::Value::UInt(u) => u.to_string(),
        mysql::Value::Float(f) => f.to_string(),
        mysql::Value::Double(d) => d.to_string(),
        mysql::Value::Date(y, m, d, h, min, s, _) =>
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, h, min, s),
        mysql::Value::Time(neg, d, h, m, s, _) => {
            let sign = if neg { "-" } else { "" };
            format!("{}{:02}:{:02}:{:02}", sign, d * 24 + h as u32, m, s)
        }
    }
}