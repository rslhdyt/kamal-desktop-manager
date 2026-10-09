//! Scaffolding for `.kamal/secrets`: which secret names a project needs, which of
//! them a secrets file already defines, and the `kamal secrets fetch` lines that
//! would define the rest. kdm never writes the file and never reads secret values.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

#[derive(Debug, Serialize, PartialEq)]
pub struct SecretsScan {
    /// The file kamal reads for this destination, relative to the project.
    pub file: String,
    pub keys: Vec<SecretKey>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct SecretKey {
    pub name: String,
    /// The secrets file that already defines it, if any.
    pub defined_in: Option<String>,
}

/// Secret names referenced by the project's deploy config, checked against
/// `.kamal/secrets-common` and the destination's secrets file.
///
/// An exception to "read config through `kamal config`": that command fails
/// while secrets are missing, which is exactly when this is needed. Only names
/// are read, ERB is not evaluated, and the UI lets the user edit the list.
pub fn scan(project: &Path, destination: Option<&str>) -> SecretsScan {
    let has_base = crate::runner::has_base_config(project);
    let mut configs = vec![];
    if has_base {
        configs.push("config/deploy.yml".to_string());
    }
    if let Some(dest) = destination {
        configs.push(format!("config/deploy.{dest}.yml"));
    }
    let mut names = BTreeSet::new();
    for config in configs {
        if let Ok(text) = std::fs::read_to_string(project.join(config)) {
            collect_names(&text, &mut names);
        }
    }

    // Kamal only gets a destination from `-d`; standalone configs run with `-c` and read `.kamal/secrets`.
    let file = match destination {
        Some(dest) if has_base => format!(".kamal/secrets.{dest}"),
        _ => ".kamal/secrets".to_string(),
    };
    let sources: Vec<(String, BTreeSet<String>)> = [".kamal/secrets-common".to_string(), file.clone()]
        .into_iter()
        .map(|f| {
            let defined = std::fs::read_to_string(project.join(&f)).map(|t| defined_names(&t)).unwrap_or_default();
            (f, defined)
        })
        .collect();

    let keys = names
        .into_iter()
        .map(|name| {
            let defined_in = sources.iter().rev().find(|(_, d)| d.contains(&name)).map(|(f, _)| f.clone());
            SecretKey { name, defined_in }
        })
        .collect();
    SecretsScan { file, keys }
}

fn collect_names(yaml: &str, names: &mut BTreeSet<String>) {
    let Ok(value) = serde_yaml::from_str::<Value>(&strip_erb(yaml)) else { return };
    walk(&value, None, names);
}

/// Drops `<% %>` / `<%= %>` tags so templated configs still parse as YAML.
fn strip_erb(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<%") {
        out.push_str(&rest[..start]);
        match rest[start..].find("%>") {
            Some(end) => rest = &rest[start + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Secret references in a kamal config: `env.secret` (anywhere: app, roles,
/// tags, accessories), `builder.secrets`, and `registry.username`/`password`
/// given as a one-item list.
fn walk(value: &Value, parent: Option<&str>, names: &mut BTreeSet<String>) {
    let Value::Mapping(map) = value else { return };
    for (key, child) in map {
        let Some(key) = key.as_str() else { continue };
        let is_ref = matches!(key, "secret" | "secrets") || (parent == Some("registry") && matches!(key, "username" | "password"));
        if let (true, Value::Sequence(items)) = (is_ref, child) {
            for item in items.iter().filter_map(Value::as_str) {
                // `ALIAS:SECRET` reads SECRET from the secrets file.
                let name = item.rsplit(':').next().unwrap_or(item).trim();
                if is_env_name(name) {
                    names.insert(name.to_string());
                }
            }
        } else {
            walk(child, Some(key), names);
        }
    }
}

/// Names assigned in a dotenv-style secrets file. Values are never kept.
fn defined_names(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, _) = line.split_once('=')?;
            let name = name.trim();
            is_env_name(name).then(|| name.to_string())
        })
        .collect()
}

fn is_env_name(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Kamal's adapter names (`kamal secrets fetch --adapter`) and whether each requires `--account`.
const ADAPTERS: &[(&str, bool)] = &[
    ("1password", true),
    ("bitwarden", true),
    ("bitwarden-sm", false),
    ("lastpass", true),
    ("aws_secrets_manager", false),
    ("gcp", true),
    ("doppler", false),
    ("enpass", false),
    ("passbolt", false),
];

#[derive(Debug, Deserialize)]
pub struct SnippetRequest {
    pub file: String,
    /// `None` when every key comes from the environment.
    pub adapter: Option<String>,
    pub account: Option<String>,
    pub from: Option<String>,
    pub keys: Vec<KeySource>,
}

#[derive(Debug, Deserialize)]
pub struct KeySource {
    pub name: String,
    pub source: Source,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Vault,
    Env,
    Skip,
}

/// The secrets file content for `req`: one `fetch` for the vault keys, an
/// `extract` per key, and `$NAME` passthroughs for environment keys.
pub fn snippet(req: &SnippetRequest) -> Result<String> {
    for key in &req.keys {
        if !is_env_name(&key.name) {
            bail!("invalid secret name: {:?}", key.name);
        }
    }
    let vault: Vec<&str> = req.keys.iter().filter(|k| k.source == Source::Vault).map(|k| k.name.as_str()).collect();
    let account = req.account.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let from = req.from.as_deref().map(str::trim).filter(|s| !s.is_empty());

    let mut lines = vec![format!("# {} (generated by Kamal Desktop Manager)", req.file)];
    if !vault.is_empty() {
        let adapter = req.adapter.as_deref().unwrap_or_default();
        let Some(&(_, needs_account)) = ADAPTERS.iter().find(|(name, _)| *name == adapter) else {
            bail!("pick a password manager for the vault keys");
        };
        if needs_account && account.is_none() {
            bail!("{adapter} needs an account");
        }
        let mut fetch = format!("kamal secrets fetch --adapter {adapter}");
        if let Some(account) = account {
            fetch += &format!(" --account {}", quote(account));
        }
        if let Some(from) = from {
            fetch += &format!(" --from {}", quote(from));
        }
        // Bitwarden Secrets Manager fetches by id; `all` lists the project's secrets by key.
        let fetched = if adapter == "bitwarden-sm" { "all".to_string() } else { vault.join(" ") };
        lines.push(format!("SECRETS=$({fetch} {fetched})"));
        lines.extend(vault.iter().map(|name| format!("{name}=$(kamal secrets extract {name} $SECRETS)")));
    }
    lines.extend(req.keys.iter().filter(|k| k.source == Source::Env).map(|k| format!("{0}=${0}", k.name)));
    Ok(lines.join("\n") + "\n")
}

/// Single-quotes values kamal would otherwise split (Enpass vault paths, names with spaces).
fn quote(s: &str) -> String {
    if s.chars().all(|c| c.is_ascii_alphanumeric() || "-_./@:+=,".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEPLOY: &str = r#"
service: app
image: acme/app
registry:
  server: ghcr.io
  username: acme
  password:
    - KAMAL_REGISTRY_PASSWORD
env:
  clear:
    DB_HOST: db
  secret:
    - RAILS_MASTER_KEY
    - DATABASE_URL:PROD_DATABASE_URL
<% if ENV["EXTRA"] %>
builder:
  secrets:
    - GITHUB_TOKEN
<% end %>
servers:
  web:
    hosts: [<%= ENV["HOST"] %>]
    env:
      secret: [WEB_ONLY]
accessories:
  db:
    image: postgres
    env:
      secret:
        - POSTGRES_PASSWORD
"#;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (path, text) in files {
            let path = tmp.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        tmp
    }

    fn names(scan: &SecretsScan) -> Vec<(&str, Option<&str>)> {
        scan.keys.iter().map(|k| (k.name.as_str(), k.defined_in.as_deref())).collect()
    }

    #[test]
    fn scans_secret_names_and_existing_files() {
        let tmp = project(&[
            ("config/deploy.yml", DEPLOY),
            ("config/deploy.staging.yml", "env:\n  secret:\n    - STAGING_ONLY\n"),
            (".kamal/secrets-common", "RAILS_MASTER_KEY=$(cat config/master.key)\n"),
            (".kamal/secrets.staging", "# comment\nexport KAMAL_REGISTRY_PASSWORD=$KAMAL_REGISTRY_PASSWORD\nRAILS_MASTER_KEY=x\n"),
        ]);
        let scan = scan(tmp.path(), Some("staging"));
        assert_eq!(scan.file, ".kamal/secrets.staging");
        assert_eq!(
            names(&scan),
            [
                ("GITHUB_TOKEN", None),
                ("KAMAL_REGISTRY_PASSWORD", Some(".kamal/secrets.staging")),
                ("POSTGRES_PASSWORD", None),
                ("PROD_DATABASE_URL", None),
                ("RAILS_MASTER_KEY", Some(".kamal/secrets.staging")),
                ("STAGING_ONLY", None),
                ("WEB_ONLY", None),
            ]
        );
    }

    #[test]
    fn standalone_destination_reads_plain_secrets_file() {
        let tmp = project(&[("config/deploy.staging.yml", "env:\n  secret: [ONLY]\n"), (".kamal/secrets", "ONLY=1\n")]);
        let scan = scan(tmp.path(), Some("staging"));
        assert_eq!(scan.file, ".kamal/secrets");
        assert_eq!(names(&scan), [("ONLY", Some(".kamal/secrets"))]);
    }

    fn req(adapter: &str, account: &str, from: &str, keys: &[(&str, Source)]) -> SnippetRequest {
        SnippetRequest {
            file: ".kamal/secrets".into(),
            adapter: Some(adapter.into()),
            account: Some(account.into()),
            from: Some(from.into()),
            keys: keys.iter().map(|(n, s)| KeySource { name: n.to_string(), source: s.clone() }).collect(),
        }
    }

    #[test]
    fn builds_fetch_extract_and_env_lines() {
        let out = snippet(&req(
            "1password",
            "acme.1password.com",
            "Prod Vault/my-app",
            &[("RAILS_MASTER_KEY", Source::Vault), ("DATABASE_URL", Source::Vault), ("KAMAL_REGISTRY_PASSWORD", Source::Env), ("X", Source::Skip)],
        ))
        .unwrap();
        assert_eq!(
            out,
            "# .kamal/secrets (generated by Kamal Desktop Manager)\n\
             SECRETS=$(kamal secrets fetch --adapter 1password --account acme.1password.com --from 'Prod Vault/my-app' RAILS_MASTER_KEY DATABASE_URL)\n\
             RAILS_MASTER_KEY=$(kamal secrets extract RAILS_MASTER_KEY $SECRETS)\n\
             DATABASE_URL=$(kamal secrets extract DATABASE_URL $SECRETS)\n\
             KAMAL_REGISTRY_PASSWORD=$KAMAL_REGISTRY_PASSWORD\n"
        );
    }

    #[test]
    fn adapter_rules() {
        let vault = || [("A", Source::Vault)];
        assert!(snippet(&req("1password", " ", "v/i", &vault())).unwrap_err().to_string().contains("needs an account"));
        assert!(snippet(&req("keepass", "", "", &vault())).is_err());
        let doppler = snippet(&req("doppler", "", "my-app/prd", &vault())).unwrap();
        assert!(doppler.contains("SECRETS=$(kamal secrets fetch --adapter doppler --from my-app/prd A)"));
        let bws = snippet(&req("bitwarden-sm", "", "", &vault())).unwrap();
        assert!(bws.contains("--adapter bitwarden-sm all)"));
        // Environment-only needs no adapter.
        let env = SnippetRequest { file: "f".into(), adapter: None, account: None, from: None, keys: vec![KeySource { name: "A".into(), source: Source::Env }] };
        assert!(snippet(&env).unwrap().ends_with("A=$A\n"));
        let bad = SnippetRequest { file: "f".into(), adapter: None, account: None, from: None, keys: vec![KeySource { name: "A; rm".into(), source: Source::Env }] };
        assert!(snippet(&bad).is_err());
    }
}
