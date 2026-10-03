use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One line of `docker ps --format '{{json .}}'`.
#[derive(Deserialize)]
struct RawContainer {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Names")]
    names: String,
    #[serde(rename = "Image")]
    image: String,
    #[serde(rename = "State")]
    state: String,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "CreatedAt")]
    created_at: String,
    #[serde(rename = "Labels", default)]
    labels: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub state: String,
    pub status: String,
    pub created_at: String,
    /// Kamal labels; None for containers Kamal didn't start.
    pub service: Option<String>,
    pub role: Option<String>,
    pub destination: Option<String>,
    /// Running, but not the newest running version of its role (Kamal's
    /// `app stale_containers` rule) — usually left over from a failed deploy.
    pub stale: bool,
    pub cpu_pct: Option<f64>,
    pub mem_bytes: Option<u64>,
}

pub fn parse_ps(lines: &[String]) -> serde_json::Result<Vec<Container>> {
    let mut containers = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let raw: RawContainer = serde_json::from_str(l)?;
            let labels: HashMap<&str, &str> = raw.labels.split(',').filter_map(|kv| kv.split_once('=')).collect();
            let label = |k: &str| labels.get(k).filter(|v| !v.is_empty()).map(|v| v.to_string());
            Ok(Container {
                service: label("service"),
                role: label("role"),
                destination: label("destination"),
                id: raw.id,
                name: raw.names,
                image: raw.image,
                state: raw.state,
                status: raw.status,
                created_at: raw.created_at,
                stale: false,
                cpu_pct: None,
                mem_bytes: None,
            })
        })
        .collect::<serde_json::Result<Vec<_>>>()?;
    mark_stale(&mut containers);
    Ok(containers)
}

fn mark_stale(containers: &mut [Container]) {
    let mut newest: HashMap<(Option<String>, Option<String>, Option<String>), String> = HashMap::new();
    for c in containers.iter().filter(|c| c.state == "running" && c.service.is_some() && c.role.is_some()) {
        let key = (c.service.clone(), c.role.clone(), c.destination.clone());
        // CreatedAt ("2026-09-20 10:11:12 +0000 UTC") sorts chronologically as text.
        let entry = newest.entry(key).or_insert_with(|| c.created_at.clone());
        if c.created_at > *entry {
            *entry = c.created_at.clone();
        }
    }
    for c in containers.iter_mut().filter(|c| c.state == "running") {
        let key = (c.service.clone(), c.role.clone(), c.destination.clone());
        c.stale = newest.get(&key).is_some_and(|newest| c.created_at < *newest);
    }
}

/// One line of `docker stats --no-stream --format '{{json .}}'`.
#[derive(Deserialize)]
struct RawStats {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "CPUPerc")]
    cpu_perc: String,
    #[serde(rename = "MemUsage")]
    mem_usage: String,
}

/// Fills cpu_pct / mem_bytes from docker stats lines, matched by name.
pub fn apply_stats(containers: &mut [Container], lines: &[String]) {
    let stats: HashMap<String, (Option<f64>, Option<u64>)> = lines
        .iter()
        .filter_map(|l| serde_json::from_str::<RawStats>(l).ok())
        .map(|s| {
            let cpu = s.cpu_perc.trim_end_matches('%').parse().ok();
            let mem = s.mem_usage.split('/').next().and_then(parse_bytes);
            (s.name, (cpu, mem))
        })
        .collect();
    for c in containers {
        if let Some((cpu, mem)) = stats.get(&c.name) {
            c.cpu_pct = *cpu;
            c.mem_bytes = *mem;
        }
    }
}

/// Parses docker's human sizes: "123.4MiB", "1.2GB", "512B".
fn parse_bytes(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s.find(|c: char| c.is_ascii_alphabetic())?;
    let (num, unit) = s.split_at(split);
    let factor: f64 = match unit {
        "B" => 1.0,
        "KiB" => 1024.0,
        "MiB" => 1024f64.powi(2),
        "GiB" => 1024f64.powi(3),
        "TiB" => 1024f64.powi(4),
        "kB" | "KB" => 1e3,
        "MB" => 1e6,
        "GB" => 1e9,
        "TB" => 1e12,
        _ => return None,
    };
    Some((num.trim().parse::<f64>().ok()? * factor) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ps_line(name: &str, state: &str, created: &str, labels: &str) -> String {
        format!(
            r#"{{"Command":"\"/rails/bin/docker-…\"","CreatedAt":"{created}","ID":"{name}-id","Image":"ghcr.io/acme/app:v","Labels":"{labels}","Names":"{name}","State":"{state}","Status":"Up"}}"#
        )
    }

    #[test]
    fn parses_labels_and_flags_stale() {
        let lines = vec![
            ps_line("app-web-new", "running", "2026-09-20 10:00:00 +0000 UTC", "service=app,role=web,destination="),
            ps_line("app-web-old", "running", "2026-09-19 10:00:00 +0000 UTC", "service=app,role=web,destination="),
            ps_line("app-web-older", "exited", "2026-09-18 10:00:00 +0000 UTC", "service=app,role=web,destination="),
            ps_line("app-job-x", "running", "2026-09-17 10:00:00 +0000 UTC", "service=app,role=job,destination="),
            ps_line("kamal-proxy", "running", "2026-09-01 10:00:00 +0000 UTC", ""),
            String::new(),
        ];
        let cs = parse_ps(&lines).unwrap();
        let stale: Vec<_> = cs.iter().filter(|c| c.stale).map(|c| c.name.as_str()).collect();
        assert_eq!(stale, ["app-web-old"]);
        assert_eq!(cs[0].service.as_deref(), Some("app"));
        assert_eq!(cs[0].role.as_deref(), Some("web"));
        assert_eq!(cs[0].destination, None, "empty label is no destination");
        assert_eq!(cs[4].service, None);
    }

    #[test]
    fn applies_docker_stats() {
        let mut cs = parse_ps(&[ps_line("app-web", "running", "x", "service=app,role=web")]).unwrap();
        let stats = [r#"{"BlockIO":"0B / 0B","CPUPerc":"12.50%","Container":"abc","ID":"abc","MemPerc":"5%","MemUsage":"256MiB / 3.8GiB","Name":"app-web","NetIO":"1kB / 2kB","PIDs":"10"}"#.to_string()];
        apply_stats(&mut cs, &stats);
        assert_eq!(cs[0].cpu_pct, Some(12.5));
        assert_eq!(cs[0].mem_bytes, Some(256 * 1024 * 1024));
        assert_eq!(parse_bytes("1.5kB"), Some(1500));
        assert_eq!(parse_bytes("??"), None);
    }
}
