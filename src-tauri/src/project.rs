use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

/// What the UI needs from `kamal config`. Built from Kamal's own output so
/// ERB and destination merging always match the CLI.
#[derive(Debug, Serialize, PartialEq)]
pub struct ProjectConfig {
    pub service: String,
    pub repository: String,
    pub version: String,
    pub roles: Vec<String>,
    pub hosts: Vec<String>,
    pub primary_host: Option<String>,
    pub ssh_user: String,
    pub ssh_port: u16,
    pub accessories: Vec<Accessory>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Accessory {
    pub name: String,
    /// From `host`/`hosts`; accessories placed by `roles` fall back to all app hosts.
    pub hosts: Vec<String>,
}

// `kamal config` dumps a Ruby hash with symbol keys, so YAML keys look like `:roles`.
#[derive(Deserialize)]
struct RawConfig {
    #[serde(rename = ":roles")]
    roles: Vec<String>,
    #[serde(rename = ":hosts")]
    hosts: Vec<String>,
    #[serde(rename = ":primary_host")]
    primary_host: Option<String>,
    #[serde(rename = ":version")]
    version: String,
    #[serde(rename = ":repository")]
    repository: String,
    #[serde(rename = ":service_with_version")]
    service_with_version: String,
    #[serde(rename = ":ssh_options", default)]
    ssh_options: RawSsh,
    #[serde(rename = ":accessories", default)]
    accessories: Value,
}

#[derive(Default, Deserialize)]
struct RawSsh {
    #[serde(rename = ":user")]
    user: Option<String>,
    #[serde(rename = ":port")]
    port: Option<u16>,
}

pub fn parse_config(yaml: &str) -> Result<ProjectConfig> {
    let raw: RawConfig = serde_yaml::from_str(yaml)?;
    let suffix = format!("-{}", raw.version);
    let service = raw.service_with_version.strip_suffix(&suffix).unwrap_or(&raw.service_with_version).to_string();

    Ok(ProjectConfig {
        service,
        repository: raw.repository,
        version: raw.version,
        roles: raw.roles,
        primary_host: raw.primary_host,
        ssh_user: raw.ssh_options.user.unwrap_or_else(|| "root".into()),
        ssh_port: raw.ssh_options.port.unwrap_or(22),
        accessories: accessories(&raw.accessories, &raw.hosts),
        hosts: raw.hosts,
    })
}

/// Only names and hosts are kept: accessory config also carries `env`.
fn accessories(value: &Value, all_hosts: &[String]) -> Vec<Accessory> {
    let value = match value {
        Value::Tagged(tagged) => &tagged.value,
        v => v,
    };
    let Value::Mapping(map) = value else { return vec![] };
    map.iter()
        .filter_map(|(name, config)| {
            let get = |key: &str| config.get(key).or_else(|| config.get(format!(":{key}")));
            let mut hosts: Vec<String> = match (get("host"), get("hosts")) {
                (Some(Value::String(h)), _) => vec![h.clone()],
                (_, Some(Value::Sequence(hs))) => hs.iter().filter_map(|h| h.as_str().map(String::from)).collect(),
                _ => vec![],
            };
            if hosts.is_empty() {
                hosts = all_hosts.to_vec();
            }
            Some(Accessory { name: name.as_str()?.trim_start_matches(':').to_string(), hosts })
        })
        .collect()
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Alias {
    pub name: String,
    pub command: String,
}

/// `aliases:` from the deploy config, read straight from the YAML since
/// `kamal config` leaves them out. The destination file overrides the base,
/// like Kamal's merge. A file that isn't plain YAML (ERB blocks) is skipped.
pub fn aliases(project: &Path, destination: Option<&str>) -> Vec<Alias> {
    let mut files = vec![project.join("config/deploy.yml")];
    if let Some(dest) = destination {
        files.push(project.join(format!("config/deploy.{dest}.yml")));
    }
    let mut aliases: Vec<Alias> = vec![];
    for file in files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        let Ok(yaml) = serde_yaml::from_str::<Value>(&text) else { continue };
        let Some(Value::Mapping(map)) = yaml.get("aliases") else { continue };
        for (name, command) in map {
            let (Some(name), Some(command)) = (name.as_str(), command.as_str()) else { continue };
            // Kamal's own rule; also keeps names from parsing as flags.
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(c)) || name.starts_with('-') {
                continue;
            }
            aliases.retain(|a| a.name != name);
            aliases.push(Alias { name: name.into(), command: command.into() });
        }
    }
    aliases
}

/// Destinations from `config/deploy.<dest>.yml` files.
pub fn destinations(project: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(project.join("config")) else { return vec![] };
    let mut dests: Vec<String> = entries
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|name| name.strip_prefix("deploy.")?.strip_suffix(".yml").map(String::from))
        .collect();
    dests.sort();
    dests
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"---
:roles:
- web
- job
:hosts:
- 10.0.0.1
- 10.0.0.2
:primary_host: 10.0.0.1
:version: abc123
:repository: ghcr.io/acme/app
:absolute_image: ghcr.io/acme/app:abc123
:service_with_version: my-app-abc123
:volume_args: []
:ssh_options:
  :user: deploy
  :port: 2222
  :keepalive: true
  :log_level: :fatal
:accessories:
  db:
    image: postgres:16
    host: 10.0.0.3
    env:
      clear:
        POSTGRES_USER: app
  redis:
    image: redis:7
    roles:
    - web
  search:
    image: meili
    hosts:
    - 10.0.0.1
    - 10.0.0.4
"#;

    #[test]
    fn parses_kamal_config_output() {
        let config = parse_config(FIXTURE).unwrap();
        assert_eq!(config.service, "my-app");
        assert_eq!(config.roles, vec!["web", "job"]);
        assert_eq!(config.hosts, vec!["10.0.0.1", "10.0.0.2"]);
        assert_eq!(config.ssh_user, "deploy");
        assert_eq!(config.ssh_port, 2222);
        let acc = |name: &str, hosts: &[&str]| Accessory { name: name.into(), hosts: hosts.iter().map(|h| h.to_string()).collect() };
        assert_eq!(
            config.accessories,
            vec![acc("db", &["10.0.0.3"]), acc("redis", &["10.0.0.1", "10.0.0.2"]), acc("search", &["10.0.0.1", "10.0.0.4"])]
        );
    }

    #[test]
    fn destination_aliases_override_base() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config/deploy.yml"), "service: app\naliases:\n  shell: app exec -i bash\n  console: app exec -i 'bin/rails c'\n  -x: nope\n").unwrap();
        std::fs::write(dir.join("config/deploy.staging.yml"), "aliases:\n  shell: app exec -i sh\n").unwrap();
        std::fs::write(dir.join("config/deploy.erb.yml"), "<% if true %>\naliases:\n  x: y\n<% end %>\n").unwrap();

        let names = |dest| aliases(dir, dest).into_iter().map(|a| format!("{}={}", a.name, a.command)).collect::<Vec<_>>();
        assert_eq!(names(None), ["shell=app exec -i bash", "console=app exec -i 'bin/rails c'"]);
        assert_eq!(names(Some("staging")), ["console=app exec -i 'bin/rails c'", "shell=app exec -i sh"]);
        assert_eq!(names(Some("erb")).len(), 2, "unparseable file is skipped");
    }
}
