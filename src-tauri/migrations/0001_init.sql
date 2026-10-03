CREATE TABLE projects (
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  path TEXT NOT NULL UNIQUE,
  kamal_bin TEXT,
  default_destination TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE runs (
  id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  destination TEXT,
  command TEXT NOT NULL,
  args_json TEXT NOT NULL,
  git_sha TEXT,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  exit_code INTEGER,
  log_path TEXT NOT NULL
);

CREATE INDEX runs_by_project ON runs(project_id, started_at DESC);
