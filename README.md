# dorisctl


---

## Project Overview

**Name suggestion:** `dorisctl` or `drs`
**Purpose:** A developer-first CLI for Apache Doris covering queries, schema ops, ingestion, and cluster inspection — with dual-protocol support and optional ecosystem bridges.

---

## Core Design Principles

**Minimum spec** means: single static binary, zero runtime dependencies, config file optional, sensible defaults everywhere. The tool should feel like `psql` meets `kubectl` — familiar, composable, scriptable.

**Protocol duality** is the key architectural decision. MySQL protocol (port 9030) is the natural path for queries and DDL. HTTP REST (port 8030) is unavoidable for Stream Load, cluster admin, and monitoring endpoints. A connection profile resolves both endpoints so the user never specifies ports per command — just a named profile.

---

## Crate Selection

**CLI & UX**
- `clap` (derive macros) — subcommand tree, flag parsing, shell completions
- `indicatif` — progress bars for ingestion and long-running loads
- `comfy-table` — tabular output for query results and cluster status
- `dialoguer` — interactive prompts for destructive operations

**MySQL protocol**
- `sqlx` with the MySQL driver and `runtime-tokio-rustls` feature — async, prepared statements, connection pooling

**HTTP / REST**
- `reqwest` with `rustls-tls` — avoids OpenSSL, keeps the binary self-contained
- `tokio` — unified async runtime across both transports

**Serialization & config**
- `serde` + `serde_json` — REST payloads and result parsing
- `toml` — config file format (profiles, defaults)
- `config` crate — layered config: file → env vars → CLI flags

**Output**
- `serde_json` + `csv` — `--format json|csv|table` on every query command
- `minus` or `bat` integration — pager for large result sets (optional feature flag)

**Ecosystem bridges** (behind feature flags, zero cost if unused)
- `iceberg-rust` (Apache Iceberg Rust) — catalog inspection, snapshot queries
- `postgres` / `tokio-postgres` — for Doris-to-PostgreSQL schema export or dual-write checks
- Spark: no native Rust crate needed — emit Spark SQL files or call Spark REST via `reqwest`

---

## Architecture

```
dorisctl
├── config/          profile resolution, env merging
├── transport/
│   ├── mysql.rs     sqlx pool + query execution
│   └── http.rs      reqwest client + auth helpers
├── commands/
│   ├── query.rs     SELECT / DML
│   ├── schema.rs    DDL operations
│   ├── load.rs      Stream Load, Broker Load
│   ├── admin.rs     backends, frontends, tablets, jobs
│   └── profile.rs   manage connection profiles
├── output/          table / json / csv formatters
└── ecosystem/
    ├── iceberg.rs   (feature = "iceberg")
    ├── spark.rs     (feature = "spark")
    └── postgres.rs  (feature = "postgres")
```

A `Connection` struct wraps both a lazy `sqlx::MySqlPool` and a `reqwest::Client`, sharing the profile's credentials. Commands declare which transport they need; the router picks it transparently.

---

## Command Surface

**Profile management**
```
dorisctl profile add <name> --fe-host --mysql-port --http-port --user
dorisctl profile list
dorisctl profile use <name>
```

**Query / DML** → MySQL transport
```
dorisctl query "<sql>"
dorisctl query -f script.sql
dorisctl query --format json|csv|table
dorisctl query --database <db>
```

**Schema / DDL** → MySQL transport
```
dorisctl schema list-dbs
dorisctl schema list-tables --db <name>
dorisctl schema describe <table>
dorisctl schema diff <table>     # compare local DDL file vs live
dorisctl schema apply -f ddl.sql # with --dry-run flag
```

**Ingestion** → HTTP transport
```
dorisctl load stream --table <db.tbl> --file data.csv --format csv|json|parquet
dorisctl load broker --job-name <n> --data-source <path> --broker <name>
dorisctl load status --label <label>
dorisctl load cancel --label <label>
```

**Cluster admin** → HTTP transport
```
dorisctl admin backends
dorisctl admin frontends
dorisctl admin tablets --table <db.tbl>
dorisctl admin jobs list
dorisctl admin jobs pause|resume|cancel <id>
dorisctl admin config get|set <key> <value>
```

**Ecosystem (feature-flagged)**
```
dorisctl iceberg catalogs
dorisctl iceberg snapshots --table <catalog.db.tbl>
dorisctl spark emit-ddl --table <tbl> -o spark_schema.sql
dorisctl postgres export-schema --table <tbl> | psql <target>
```

---

## Configuration

A `~/.config/dorisctl/config.toml` holds named profiles:

```toml
[defaults]
profile = "local"
format = "table"
pager = true

[profiles.local]
fe_host = "localhost"
mysql_port = 9030
http_port = 8030
user = "root"
# password via env: DORISCTL_PASSWORD or keyring

[profiles.staging]
fe_host = "doris-fe.internal"
...
```

Every flag can be overridden at call time via `-p <profile>` or env vars (`DORISCTL_PROFILE`, `DORISCTL_HOST`, etc.), following standard 12-factor precedence.

---

## Minimum Viable Milestones

**M1 — Core shell**
Profile management, MySQL transport wired to `query` and `schema list-*`, table formatter. Goal: `dorisctl query "show databases"` works end-to-end.

**M2 — DDL + ingestion**
`schema describe/apply`, Stream Load via HTTP, `load status`. This is the daily-driver slice for most developers.

**M3 — Cluster admin**
`admin backends/frontends/jobs`. Adds the HTTP client fully; Stream Load already exercises it in M2, so this is incremental.

**M4 — Ecosystem bridges**
Behind `--features iceberg,postgres,spark`. Iceberg catalog reads via `iceberg-rust`. Postgres schema export. Spark DDL emitter (pure string transformation, no external dependency).

**M5 — Polish**
Shell completions (`clap_complete`), man page generation, `--dry-run` everywhere destructive, structured logging (`tracing` + `tracing-subscriber`), release workflow (your existing `release.yml` pattern applies directly here).

---

## Iceberg / Spark / Postgres Integration Notes

**Iceberg** — Doris 2.x has native Iceberg catalog support via `CREATE CATALOG`. `dorisctl iceberg` can issue `SHOW CATALOGS` and `SHOW SNAPSHOTS` over MySQL transport, and optionally speak directly to a REST or Hive catalog via `iceberg-rust` for richer metadata without routing through Doris at all.

**Spark** — The pragmatic integration is DDL translation: emit a Doris table schema as Spark-compatible `CREATE TABLE` or a DataFrame schema JSON. Full Spark submission (via Spark Connect or Livy REST) can live behind a feature flag using plain `reqwest` — no Spark-specific Rust crate needed.

**PostgreSQL** — Useful in two directions: export a Doris schema as Postgres-compatible DDL for tooling compatibility, or run parallel queries against a Postgres source during a migration. `tokio-postgres` is lightweight and fits the async runtime already in use.

---

## Key Constraints Honored

- **Single binary**: `rustls` throughout, no system OpenSSL, `musl` target viable
- **No runtime deps**: config file optional, works with env vars alone in CI
- **Minimum feature surface**: ecosystem crates are opt-in features, not default
- **Scriptable**: `--format json` + exit codes on every command; stderr for diagnostics, stdout for data
