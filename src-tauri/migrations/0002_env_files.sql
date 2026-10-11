-- A dotenv file loaded into kamal's environment per destination ('' = no destination).
-- An empty path turns off the `.env.<destination>` default.
CREATE TABLE env_files (
  project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  destination TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY (project_id, destination)
);
