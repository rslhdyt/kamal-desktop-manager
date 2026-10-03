use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub kamal_bin: Option<String>,
    pub default_destination: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, FromRow)]
pub struct Run {
    pub id: i64,
    pub project_id: i64,
    pub destination: Option<String>,
    pub command: String,
    pub args_json: String,
    pub git_sha: Option<String>,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub exit_code: Option<i64>,
    #[serde(skip)]
    pub log_path: String,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub async fn open(path: &Path) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new().filename(path).create_if_missing(true);
    connect(opts).await
}

async fn connect(opts: SqliteConnectOptions) -> Result<SqlitePool> {
    let pool = SqlitePoolOptions::new().max_connections(4).connect_with(opts).await?;
    sqlx::migrate!().run(&pool).await?;
    // Runs still open at startup were cut off by an app exit.
    sqlx::query("UPDATE runs SET finished_at = ? WHERE finished_at IS NULL")
        .bind(now_ms())
        .execute(&pool)
        .await?;
    Ok(pool)
}

/// Points run log paths at a moved data directory.
pub async fn rebase_log_paths(pool: &SqlitePool, old_dir: &str, new_dir: &str) -> Result<()> {
    sqlx::query("UPDATE runs SET log_path = ? || substr(log_path, length(?) + 1) WHERE substr(log_path, 1, length(?)) = ?")
        .bind(new_dir)
        .bind(old_dir)
        .bind(old_dir)
        .bind(old_dir)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn project_add(pool: &SqlitePool, name: &str, path: &str) -> Result<Project> {
    sqlx::query_as("INSERT INTO projects (name, path, created_at) VALUES (?, ?, ?) RETURNING *")
        .bind(name)
        .bind(path)
        .bind(now_ms())
        .fetch_one(pool)
        .await
        .with_context(|| format!("{path} is already added"))
}

pub async fn project_list(pool: &SqlitePool) -> Result<Vec<Project>> {
    Ok(sqlx::query_as("SELECT * FROM projects ORDER BY name").fetch_all(pool).await?)
}

pub async fn project_get(pool: &SqlitePool, id: i64) -> Result<Project> {
    sqlx::query_as("SELECT * FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("project {id} not found"))
}

pub async fn project_set_destination(pool: &SqlitePool, id: i64, destination: Option<&str>) -> Result<()> {
    sqlx::query("UPDATE projects SET default_destination = ? WHERE id = ?")
        .bind(destination)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Deletes the project and its runs; returns the run log paths to clean up.
pub async fn project_remove(pool: &SqlitePool, id: i64) -> Result<Vec<String>> {
    let logs: Vec<(String,)> = sqlx::query_as("SELECT log_path FROM runs WHERE project_id = ?")
        .bind(id)
        .fetch_all(pool)
        .await?;
    sqlx::query("DELETE FROM projects WHERE id = ?").bind(id).execute(pool).await?;
    Ok(logs.into_iter().map(|(p,)| p).collect())
}

pub struct NewRun<'a> {
    pub project_id: i64,
    pub destination: Option<&'a str>,
    pub command: &'a str,
    pub args: &'a [String],
    pub git_sha: Option<&'a str>,
}

/// Inserts a run; `log_path` is derived from the new id.
pub async fn run_insert(pool: &SqlitePool, run: NewRun<'_>, log_path: impl Fn(i64) -> String) -> Result<Run> {
    let mut tx = pool.begin().await?;
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO runs (project_id, destination, command, args_json, git_sha, started_at, log_path)
         VALUES (?, ?, ?, ?, ?, ?, '') RETURNING id",
    )
    .bind(run.project_id)
    .bind(run.destination)
    .bind(run.command)
    .bind(serde_json::to_string(run.args)?)
    .bind(run.git_sha)
    .bind(now_ms())
    .fetch_one(&mut *tx)
    .await?;
    let inserted = sqlx::query_as("UPDATE runs SET log_path = ? WHERE id = ? RETURNING *")
        .bind(log_path(id))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(inserted)
}

pub async fn run_finish(pool: &SqlitePool, id: i64, exit_code: Option<i32>) -> Result<()> {
    sqlx::query("UPDATE runs SET finished_at = ?, exit_code = ? WHERE id = ?")
        .bind(now_ms())
        .bind(exit_code)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn run_get(pool: &SqlitePool, id: i64) -> Result<Run> {
    sqlx::query_as("SELECT * FROM runs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .with_context(|| format!("run {id} not found"))
}

pub async fn runs_list(pool: &SqlitePool, project_id: i64, limit: i64) -> Result<Vec<Run>> {
    Ok(sqlx::query_as("SELECT * FROM runs WHERE project_id = ? ORDER BY started_at DESC, id DESC LIMIT ?")
        .bind(project_id)
        .bind(limit)
        .fetch_all(pool)
        .await?)
}

#[cfg(test)]
pub async fn memory() -> SqlitePool {
    use std::str::FromStr;
    // One connection, or each pooled connection gets its own empty in-memory DB.
    let opts = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(opts).await.unwrap();
    sqlx::migrate!().run(&pool).await.unwrap();
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn projects_and_runs_roundtrip() {
        let pool = memory().await;
        let project = project_add(&pool, "app", "/src/app").await.unwrap();
        assert!(project_add(&pool, "app", "/src/app").await.is_err(), "path is unique");

        project_set_destination(&pool, project.id, Some("staging")).await.unwrap();
        assert_eq!(project_get(&pool, project.id).await.unwrap().default_destination.as_deref(), Some("staging"));

        let args = vec!["deploy".to_string(), "-d".into(), "staging".into()];
        let new = NewRun { project_id: project.id, destination: Some("staging"), command: "deploy", args: &args, git_sha: Some("abc") };
        let run = run_insert(&pool, new, |id| format!("/logs/{id}.log")).await.unwrap();
        assert_eq!(run.log_path, format!("/logs/{}.log", run.id));
        assert_eq!(run.finished_at, None);

        run_finish(&pool, run.id, Some(1)).await.unwrap();
        let runs = runs_list(&pool, project.id, 10).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].exit_code, Some(1));
        assert!(runs[0].finished_at.is_some());

        rebase_log_paths(&pool, "/logs", "/moved/logs").await.unwrap();
        let moved = run_get(&pool, run.id).await.unwrap().log_path;
        assert_eq!(moved, format!("/moved/logs/{}.log", run.id));
        rebase_log_paths(&pool, "/elsewhere", "/x").await.unwrap();
        assert_eq!(run_get(&pool, run.id).await.unwrap().log_path, moved, "other prefixes untouched");

        assert_eq!(project_remove(&pool, project.id).await.unwrap(), vec![moved]);
        assert!(runs_list(&pool, project.id, 10).await.unwrap().is_empty(), "runs cascade");
    }
}
