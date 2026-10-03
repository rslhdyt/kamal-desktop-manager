use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::docker::{self, Container};

const SEP: &str = "@@KDM@@";

/// Everything a host poll needs in one round trip.
pub fn batch_command() -> String {
    [
        "head -n1 /proc/stat",
        "cat /proc/loadavg",
        "cat /proc/meminfo",
        "df -B1 --output=size,used / | tail -n1",
        "docker ps -a --format '{{json .}}'",
        "docker stats --no-stream --format '{{json .}}'",
    ]
    .join(&format!("; echo {SEP}; "))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HostMetrics {
    /// None on the first sample: CPU % needs two /proc/stat readings.
    pub cpu_pct: Option<f64>,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub mem_used: u64,
    pub mem_total: u64,
    pub disk_used: u64,
    pub disk_total: u64,
}

/// (total, idle) jiffies from the aggregate `cpu` line of /proc/stat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuTimes {
    total: u64,
    idle: u64,
}

pub struct Sample {
    pub metrics: HostMetrics,
    pub cpu: CpuTimes,
    pub containers: Vec<Container>,
}

pub fn parse_batch(stdout: &[String], prev_cpu: Option<CpuTimes>) -> Result<Sample> {
    let sections: Vec<&[String]> = stdout.split(|l| l == SEP).collect();
    let [stat, loadavg, meminfo, df, ps, stats] = sections[..] else {
        bail!("expected 6 sections from host poll, got {}", sections.len());
    };

    let cpu = parse_cpu(stat.first().context("empty /proc/stat")?)?;
    let cpu_pct = prev_cpu.and_then(|prev| {
        let total = cpu.total.checked_sub(prev.total)?;
        let idle = cpu.idle.checked_sub(prev.idle)?;
        (total > 0).then(|| 100.0 * (total - idle) as f64 / total as f64)
    });

    let loads: Vec<f64> = loadavg
        .first()
        .context("empty /proc/loadavg")?
        .split_whitespace()
        .take(3)
        .map(|v| v.parse().context("bad loadavg"))
        .collect::<Result<_>>()?;
    let [load1, load5, load15] = loads[..] else { bail!("bad loadavg") };

    let kb = |key: &str| -> Result<u64> {
        let line = meminfo.iter().find(|l| l.starts_with(key)).with_context(|| format!("no {key} in meminfo"))?;
        Ok(line.split_whitespace().nth(1).context("bad meminfo")?.parse::<u64>()? * 1024)
    };
    let mem_total = kb("MemTotal:")?;
    let mem_used = mem_total.saturating_sub(kb("MemAvailable:")?);

    let disk: Vec<u64> = df
        .first()
        .context("empty df")?
        .split_whitespace()
        .map(|v| v.parse().context("bad df"))
        .collect::<Result<_>>()?;
    let [disk_total, disk_used] = disk[..] else { bail!("bad df output") };

    let mut containers = docker::parse_ps(ps)?;
    docker::apply_stats(&mut containers, stats);

    Ok(Sample {
        metrics: HostMetrics { cpu_pct, load1, load5, load15, mem_used, mem_total, disk_used, disk_total },
        cpu,
        containers,
    })
}

fn parse_cpu(line: &str) -> Result<CpuTimes> {
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8) // user nice system idle iowait irq softirq steal
        .map(|v| v.parse().context("bad /proc/stat"))
        .collect::<Result<_>>()?;
    if fields.len() < 5 {
        bail!("bad /proc/stat line");
    }
    Ok(CpuTimes { total: fields.iter().sum(), idle: fields[3] + fields[4] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(stat: &str) -> Vec<String> {
        [
            stat,
            SEP,
            "0.52 0.40 0.30 1/234 5678",
            SEP,
            "MemTotal:        4000000 kB",
            "MemFree:          500000 kB",
            "MemAvailable:    3000000 kB",
            SEP,
            " 100000000000 25000000000",
            SEP,
            r#"{"CreatedAt":"2026-09-20 10:00:00 +0000 UTC","ID":"a","Image":"app:v1","Labels":"service=app,role=web","Names":"app-web-v1","State":"running","Status":"Up 2 hours"}"#,
            SEP,
            r#"{"CPUPerc":"3.00%","MemUsage":"100MiB / 3.8GiB","Name":"app-web-v1"}"#,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    #[test]
    fn batch_command_joins_sections() {
        assert!(batch_command().contains("cat /proc/loadavg; echo @@KDM@@; cat /proc/meminfo"));
    }

    #[test]
    fn parses_host_poll_and_cpu_delta() {
        let first = parse_batch(&fixture("cpu  100 0 100 700 100 0 0 0 0 0"), None).unwrap();
        assert_eq!(first.metrics.cpu_pct, None);
        assert_eq!(first.metrics.load1, 0.52);
        assert_eq!(first.metrics.mem_total, 4_000_000 * 1024);
        assert_eq!(first.metrics.mem_used, 1_000_000 * 1024);
        assert_eq!(first.metrics.disk_total, 100_000_000_000);
        assert_eq!(first.metrics.disk_used, 25_000_000_000);
        assert_eq!(first.containers[0].cpu_pct, Some(3.0));

        // +100 busy, +100 idle jiffies → 50%.
        let second = parse_batch(&fixture("cpu  150 0 150 750 150 0 0 0 0 0"), Some(first.cpu)).unwrap();
        assert_eq!(second.metrics.cpu_pct, Some(50.0));
    }

    #[test]
    fn rejects_truncated_output() {
        assert!(parse_batch(&["cpu 1 2 3 4 5".to_string()], None).is_err());
    }
}
