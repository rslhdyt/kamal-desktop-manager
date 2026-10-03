use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

pub const LIST_COMMAND: &str = "docker exec kamal-proxy kamal-proxy list";

/// One row of `kamal-proxy list`.
#[derive(Debug, Serialize, PartialEq)]
pub struct ProxyRoute {
    pub service: String,
    pub host: String,
    pub path: String,
    /// `<container id>:<port>` of the container receiving traffic.
    pub target: String,
    pub state: String,
    pub tls: bool,
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ params final-byte(@..~)
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `kamal-proxy list` prints a human table (coloured even without a tty) and
/// has no JSON mode, so columns are cut at the header's positions, which
/// copes with empty cells. Unknown layouts fail loudly for the raw fallback.
pub fn parse_list(stdout: &[String]) -> Result<Vec<ProxyRoute>> {
    let lines: Vec<String> = stdout.iter().map(|l| strip_ansi(l)).filter(|l| !l.trim().is_empty()).collect();
    let Some(header) = lines.first() else { return Ok(vec![]) };

    let names = ["Service", "Host", "Path", "Target", "State", "TLS"];
    let starts: Vec<usize> = names
        .iter()
        .map(|n| header.find(n).with_context(|| format!("kamal-proxy list header has no {n} column: {header:?}")))
        .collect::<Result<_>>()?;
    if starts.windows(2).any(|w| w[0] >= w[1]) {
        bail!("unexpected kamal-proxy list header: {header:?}");
    }

    Ok(lines[1..]
        .iter()
        .map(|line| {
            let cell = |i: usize| {
                let from = starts[i].min(line.len());
                let to = starts.get(i + 1).copied().unwrap_or(line.len()).min(line.len());
                line.get(from..to).unwrap_or("").trim().to_string()
            };
            ProxyRoute { service: cell(0), host: cell(1), path: cell(2), target: cell(3), state: cell(4), tls: cell(5) == "yes" }
        })
        .collect())
}

/// Follows kamal-proxy's JSON request log for the given proxy services
/// (`<service>-<role>[-<destination>]`), filtered on the host with grep.
pub fn requests_command(services: &[String], since: &str) -> Result<String> {
    if services.is_empty() {
        bail!("no proxy services to follow");
    }
    let valid = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if let Some(bad) = services.iter().find(|s| !valid(s)).or((!valid(since)).then_some(&String::new())) {
        bail!("invalid service or since value: {bad:?}");
    }
    let patterns: String = services.iter().map(|s| format!(r#" -e '"service":"{s}"'"#)).collect();
    Ok(format!("docker logs -f --tail 5000 --since {since} kamal-proxy 2>&1 | grep --line-buffered -F{patterns}"))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProxyRequest {
    pub time: String,
    pub service: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status: u16,
    pub duration_ms: f64,
    pub request_id: String,
}

#[derive(Deserialize)]
struct RawRequest {
    time: String,
    msg: String,
    #[serde(default)]
    service: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    host: String,
    #[serde(default)]
    path: String,
    status: u16,
    /// Go time.Duration: nanoseconds.
    #[serde(default)]
    duration: u64,
    #[serde(default)]
    request_id: String,
}

/// Parses one kamal-proxy log line; None for anything that isn't a request.
pub fn parse_request(line: &str) -> Option<ProxyRequest> {
    let raw: RawRequest = serde_json::from_str(line.trim()).ok()?;
    (raw.msg == "Request").then(|| ProxyRequest {
        time: raw.time,
        service: raw.service,
        method: raw.method,
        host: raw.host,
        path: raw.path,
        status: raw.status,
        duration_ms: raw.duration as f64 / 1e6,
        request_id: raw.request_id,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Captured from kamal-proxy on a real host (Kamal 2.12).
    const LIST: &str = "\x1b[3;94mService\x1b[0m            \x1b[3;94mHost\x1b[0m                     \x1b[3;94mPath\x1b[0m  \x1b[3;94mTarget\x1b[0m                    \x1b[3;94mState\x1b[0m    \x1b[3;94mTLS\x1b[0m
\x1b[1;34mesign-api-demo\x1b[0m     \x1b[mesign.rslhdyt.dev\x1b[0m        \x1b[m/\x1b[0m     \x1b[m026bec588890:4567\x1b[0m         \x1b[mrunning\x1b[0m  \x1b[myes\x1b[0m
\x1b[1;34mpghero\x1b[0m             \x1b[mpghero.webhookdump.link\x1b[0m  \x1b[m/\x1b[0m     \x1b[mwebhook-dump-pghero:8080\x1b[0m  \x1b[mrunning\x1b[0m  \x1b[myes\x1b[0m
\x1b[1;34mstore-web\x1b[0m          \x1b[m\x1b[0m                         \x1b[m/\x1b[0m     \x1b[m1c9ec56282d8:80\x1b[0m           \x1b[mpaused\x1b[0m   \x1b[mno\x1b[0m   ";

    #[test]
    fn parses_kamal_proxy_list() {
        let lines: Vec<String> = LIST.lines().map(String::from).collect();
        let routes = parse_list(&lines).unwrap();
        assert_eq!(routes.len(), 3);
        assert_eq!(
            routes[0],
            ProxyRoute {
                service: "esign-api-demo".into(),
                host: "esign.rslhdyt.dev".into(),
                path: "/".into(),
                target: "026bec588890:4567".into(),
                state: "running".into(),
                tls: true,
            }
        );
        assert_eq!(routes[1].target, "webhook-dump-pghero:8080");
        assert_eq!(routes[2].host, "", "empty host cell");
        assert_eq!(routes[2].state, "paused");
        assert!(!routes[2].tls);
    }

    #[test]
    fn unknown_list_layout_is_an_error() {
        assert!(parse_list(&["NAME  URL".to_string(), "x  y".to_string()]).is_err());
        assert!(parse_list(&[]).unwrap().is_empty());
    }

    #[test]
    fn parses_request_log_lines() {
        let line = r#"{"time":"2026-09-25T11:51:10.684529007Z","level":"INFO","msg":"Request","host":"storefront.example.com","port":443,"path":"/up","request_id":"ff0d3479","status":200,"service":"store-web","target":"1c9ec56282d8:80","duration":2500000,"method":"GET","req_content_length":0,"client_addr":"10.0.0.1","proto":"HTTP/1.1","scheme":"https","query":""}"#;
        let req = parse_request(line).unwrap();
        assert_eq!(req.service, "store-web");
        assert_eq!(req.status, 200);
        assert_eq!(req.duration_ms, 2.5);
        assert_eq!(parse_request(r#"{"time":"t","level":"WARN","msg":"No server name; using default TLS hostname","host":"x"}"#), None);
        assert_eq!(parse_request("not json"), None);
    }

    #[test]
    fn builds_requests_command() {
        assert_eq!(
            requests_command(&["store-web".into()], "30m").unwrap(),
            r#"docker logs -f --tail 5000 --since 30m kamal-proxy 2>&1 | grep --line-buffered -F -e '"service":"store-web"'"#
        );
        assert!(requests_command(&["x'; reboot".into()], "30m").is_err());
        assert!(requests_command(&["store-web".into()], "1m; reboot").is_err());
        assert!(requests_command(&[], "30m").is_err());
    }
}
