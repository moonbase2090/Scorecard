// SPDX-License-Identifier: MIT
//! Export saved Scorecard findings to Waypoint without duplicating tickets on retries.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

use sc_core::{Finding, Scorecard};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::{Host, Url};

use crate::ExportArgs;

const EXPORT_LABEL: &str = "scorecard-export-v1";
const GROUP_LABEL_PREFIX: &str = "scorecard-group-";
const FINDING_MARKER_PREFIX: &str = "scorecard-finding-v1:";
const COMMENT_MAX_BYTES: usize = 1_900;
const REQUEST_TIMEOUT_SECONDS: u64 = 20;

pub(super) fn run(args: ExportArgs) -> i32 {
    match run_export(args) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("sc export: {error}");
            2
        }
    }
}

fn run_export(args: ExportArgs) -> Result<(), String> {
    if !args.waypoint {
        return Err("select an export target with --waypoint".into());
    }
    validate_project(&args.project)?;
    let report = read_report(&args.input)?;
    let groups = group_findings(&report.sections.findings)?;

    if args.dry_run {
        print_plan(&args.project, &args.input, &groups);
        return Ok(());
    }
    if groups.is_empty() {
        println!(
            "No findings to export to Waypoint project {}.",
            args.project
        );
        return Ok(());
    }

    let base = args
        .api_url
        .or_else(|| env::var("WAYPOINT_API_URL").ok())
        .ok_or_else(|| "set WAYPOINT_API_URL or pass --api-url".to_owned())?;
    let token = env::var(&args.token_env)
        .map_err(|_| format!("set {} to a Waypoint bearer token", args.token_env))?;
    if token.trim().is_empty() {
        return Err(format!("{} is empty", args.token_env));
    }
    let agent_id = args
        .agent_id
        .or_else(|| env::var("WAYPOINT_AGENT_ID").ok())
        .or_else(|| env::var("PMUX_AGENT").ok())
        .filter(|value| !value.trim().is_empty());
    if let Some(agent_id) = agent_id.as_deref() {
        validate_agent_id(agent_id)?;
    }

    let mut api = WaypointApi::new(&base, token, agent_id)?;
    let summary = export_groups(&mut api, &args.project, &report, &groups)?;
    println!(
        "Waypoint export to {}: {} ticket(s) created, {} finding update(s) posted, {} unchanged, {} archived.",
        args.project, summary.created, summary.updated, summary.unchanged, summary.archived
    );
    Ok(())
}

fn read_report(path: &Path) -> Result<Scorecard, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("could not read report {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "could not parse Scorecard report {}: {error}",
            path.display()
        )
    })
}

fn validate_project(project: &str) -> Result<(), String> {
    if project.is_empty()
        || project.len() > 80
        || !project
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(
            "--project must be a Waypoint project slug (letters, numbers, '.', '_' or '-')".into(),
        );
    }
    Ok(())
}

fn validate_agent_id(agent_id: &str) -> Result<(), String> {
    if agent_id.trim() != agent_id
        || agent_id.is_empty()
        || agent_id.len() > 128
        || agent_id.chars().any(char::is_control)
    {
        return Err("Waypoint agent identity is empty or contains invalid characters".into());
    }
    Ok(())
}

#[derive(Debug)]
struct FindingGroup<'a> {
    file: String,
    symbol: Option<String>,
    label: String,
    findings: Vec<&'a Finding>,
}

fn group_findings(findings: &[Finding]) -> Result<Vec<FindingGroup<'_>>, String> {
    let mut grouped = BTreeMap::<(String, Option<String>), Vec<&Finding>>::new();
    for finding in findings {
        let file = normalize_report_path(&finding.file)?;
        grouped
            .entry((file, finding.symbol.clone()))
            .or_default()
            .push(finding);
    }

    Ok(grouped
        .into_iter()
        .map(|((file, symbol), mut findings)| {
            findings.sort_by_key(|finding| finding.fingerprint());
            let key = group_key(&file, symbol.as_deref());
            FindingGroup {
                file,
                symbol,
                label: format!("{GROUP_LABEL_PREFIX}{key}"),
                findings,
            }
        })
        .collect())
}

fn normalize_report_path(path: &str) -> Result<String, String> {
    let normalized = path.replace('\\', "/");
    let drive_absolute = normalized.as_bytes().get(1) == Some(&b':');
    if normalized.is_empty()
        || normalized.starts_with('/')
        || drive_absolute
        || normalized.split('/').any(|part| part == "..")
    {
        return Err(format!(
            "refusing to export non-relative finding path `{}`",
            path.chars().take(120).collect::<String>()
        ));
    }
    Ok(normalized)
}

fn group_key(file: &str, symbol: Option<&str>) -> String {
    let mut hash = Sha256::new();
    hash.update(b"scorecard-waypoint-group-v1\0");
    for value in [file, symbol.unwrap_or("")] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

fn print_plan(project: &str, input: &Path, groups: &[FindingGroup<'_>]) {
    println!(
        "Dry run: {} file/function group(s) from {} would be reconciled in Waypoint project {}. No network requests were made.",
        groups.len(),
        input.display(),
        project
    );
    for group in groups {
        println!(
            "  {}{} — {} finding(s)",
            group.file,
            group
                .symbol
                .as_deref()
                .map(|symbol| format!("::{symbol}"))
                .unwrap_or_default(),
            group.findings.len()
        );
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ExportSummary {
    created: usize,
    updated: usize,
    unchanged: usize,
    archived: usize,
}

fn export_groups<A: Waypoint>(
    api: &mut A,
    project: &str,
    report: &Scorecard,
    groups: &[FindingGroup<'_>],
) -> Result<ExportSummary, String> {
    let tickets = api.board(project, EXPORT_LABEL)?;
    let mut by_group = BTreeMap::<String, Vec<WaypointTicket>>::new();
    for ticket in tickets {
        for label in &ticket.labels {
            if label.starts_with(GROUP_LABEL_PREFIX) {
                by_group
                    .entry(label.clone())
                    .or_default()
                    .push(ticket.clone());
            }
        }
    }

    let mut summary = ExportSummary::default();
    for group in groups {
        let found = by_group
            .get(&group.label)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if found.len() > 1 {
            return Err(format!(
                "multiple Waypoint tickets carry export key {} in project {}; resolve duplicates before retrying",
                group.label, project
            ));
        }
        let (mut ticket, is_new) = if let Some(ticket) = found.first() {
            (ticket.clone(), false)
        } else {
            let ticket = api.create(project, &group_title(group), &[EXPORT_LABEL, &group.label])?;
            (ticket, true)
        };

        if is_new {
            summary.created += 1;
            by_group
                .entry(group.label.clone())
                .or_default()
                .push(ticket.clone());
        }
        if ticket.archived {
            summary.archived += 1;
            continue;
        }

        for finding in &group.findings {
            let body = finding_comment(report, finding, &group.file)?;
            let marker = format!("{FINDING_MARKER_PREFIX}{}", finding.fingerprint());
            let already_posted = ticket.comments.iter().any(|comment| {
                comment.body.as_deref().and_then(|body| body.lines().next())
                    == Some(marker.as_str())
            });
            if already_posted {
                summary.unchanged += 1;
                continue;
            }
            ticket = api.comment(project, &ticket.id, ticket.version, &body)?;
            summary.updated += 1;
        }
    }
    Ok(summary)
}

fn group_title(group: &FindingGroup<'_>) -> String {
    let file = safe_line(&group.file);
    let location = group
        .symbol
        .as_deref()
        .map(|symbol| format!("{} ({file})", safe_line(symbol)))
        .unwrap_or(file);
    truncate_text(&format!("Scorecard findings: {location}"), 180)
}

fn finding_comment(
    report: &Scorecard,
    finding: &Finding,
    relative_file: &str,
) -> Result<String, String> {
    let fingerprint = finding.fingerprint();
    let line = finding.span.as_ref().map(|span| span.start_line);
    let location = line
        .map(|line| format!("{relative_file}:{line}"))
        .unwrap_or_else(|| relative_file.to_owned());
    let symbol = finding
        .symbol
        .as_deref()
        .map(|symbol| format!("\nSymbol: {symbol}"))
        .unwrap_or_default();
    let action = finding
        .suggested_action
        .as_deref()
        .unwrap_or("No suggested action was included in the report.");
    let body = format!(
        "{FINDING_MARKER_PREFIX}{fingerprint}\nScorecard report {}\nRule: {}\nEngine: {}\nSeverity: {}\nLocation: {location}{symbol}\nFinding: {}\nSuggested action: {}",
        report.identity.id,
        safe_line(&finding.rule),
        safe_line(&finding.engine),
        safe_line(&finding.severity),
        safe_line(&finding.message),
        safe_line(action)
    );
    Ok(truncate_text(&body, COMMENT_MAX_BYTES))
}

fn safe_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn truncate_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_owned();
    }
    let suffix = "…";
    let limit = max_bytes.saturating_sub(suffix.len());
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{suffix}", &text[..end])
}

trait Waypoint {
    fn board(&mut self, project: &str, label: &str) -> Result<Vec<WaypointTicket>, String>;
    fn create(
        &mut self,
        project: &str,
        title: &str,
        labels: &[&str],
    ) -> Result<WaypointTicket, String>;
    fn comment(
        &mut self,
        project: &str,
        id: &str,
        version: u64,
        body: &str,
    ) -> Result<WaypointTicket, String>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WaypointTicket {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    version: u64,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    comments: Vec<WaypointComment>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct WaypointComment {
    #[serde(default)]
    body: Option<String>,
}

#[derive(Deserialize)]
struct BoardResponse {
    #[serde(default)]
    tickets: Vec<WaypointTicket>,
}

struct WaypointApi {
    agent: ureq::Agent,
    base: Url,
    token: String,
    acting_as: Option<String>,
}

impl WaypointApi {
    fn new(api_url: &str, token: String, acting_as: Option<String>) -> Result<Self, String> {
        let mut base =
            Url::parse(api_url).map_err(|_| "--api-url is not a valid URL".to_owned())?;
        let local_http = base.scheme() == "http" && is_loopback(base.host());
        if base.scheme() != "https" && !local_http {
            return Err("--api-url must use HTTPS (HTTP is allowed only on loopback)".into());
        }
        if !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
        {
            return Err("--api-url must not contain credentials, a query, or a fragment".into());
        }
        let path = base.path().trim_end_matches('/');
        let path = if path.ends_with("/v1") {
            path.to_owned()
        } else {
            format!("{path}/v1")
        };
        base.set_path(&format!("{}/", path.trim_end_matches('/')));
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self {
            agent: ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
                .build(),
            base,
            token,
            acting_as,
        })
    }

    fn request<T: DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, String> {
        let url = self
            .base
            .join(path)
            .map_err(|_| "could not build Waypoint API request URL".to_owned())?;
        let mut request = self
            .agent
            .request(method, url.as_str())
            .set("Authorization", &format!("Bearer {}", self.token))
            .set("Accept", "application/json");
        if let Some(agent_id) = &self.acting_as {
            request = request.set("X-Waypoint-Act-As", agent_id);
        }
        let response = match body {
            Some(body) => request.send_json(body),
            None => request.call(),
        }
        .map_err(|error| match error {
            ureq::Error::Status(status, _) => {
                format!("Waypoint API returned HTTP {status} for {method} {path}")
            }
            ureq::Error::Transport(_) => {
                format!("could not reach Waypoint while sending {method} {path}")
            }
        })?;
        response
            .into_json()
            .map_err(|_| format!("Waypoint returned invalid JSON for {method} {path}"))
    }
}

impl Waypoint for WaypointApi {
    fn board(&mut self, project: &str, label: &str) -> Result<Vec<WaypointTicket>, String> {
        let path = format!(
            "projects/{}/board?include_archived=true&label={}",
            encode_component(project),
            encode_component(label)
        );
        self.request::<BoardResponse, ()>("GET", &path, None)
            .map(|response| response.tickets)
    }

    fn create(
        &mut self,
        project: &str,
        title: &str,
        labels: &[&str],
    ) -> Result<WaypointTicket, String> {
        let body = serde_json::json!({
            "project": project,
            "title": title,
            "ticket_type": "task",
            "labels": labels,
        });
        self.request("POST", "tickets", Some(&body))
    }

    fn comment(
        &mut self,
        project: &str,
        id: &str,
        version: u64,
        body: &str,
    ) -> Result<WaypointTicket, String> {
        let path = format!(
            "tickets/{}/comments?project={}",
            encode_component(id),
            encode_component(project)
        );
        let request = serde_json::json!({ "version": version, "body": body });
        self.request("POST", &path, Some(&request))
    }
}

fn encode_component(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn is_loopback(host: Option<Host<&str>>) -> bool {
    match host {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => IpAddr::V4(address).is_loopback(),
        Some(Host::Ipv6(address)) => IpAddr::V6(address).is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sc_core::{Finding, Scorecard};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn finding(file: &str, symbol: Option<&str>, message: &str) -> Finding {
        Finding {
            id: format!("lint:{file}:{symbol:?}"),
            rule: "lint.failed".into(),
            engine: "lint".into(),
            severity: "warning".into(),
            file: file.into(),
            span: None,
            symbol: symbol.map(str::to_owned),
            message: message.into(),
            evidence: json!({"source": "do-not-export"}),
            suggested_action: Some("Fix the warning".into()),
            disposition: "ask".into(),
        }
    }

    fn report(findings: Vec<Finding>) -> Scorecard {
        let mut report = Scorecard::skeleton(".", 30);
        report.sections.findings = findings;
        report
    }

    #[test]
    fn findings_are_grouped_by_relative_file_and_symbol() {
        let findings = vec![
            finding("src/a.rs", Some("parse"), "first"),
            finding("src/a.rs", Some("parse"), "second"),
            finding("src/a.rs", Some("render"), "third"),
            finding("src/a.rs", None, "fourth"),
        ];
        let groups = group_findings(&findings).unwrap();
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].findings.len(), 1);
        assert_eq!(groups[1].findings.len(), 2);
        assert_eq!(groups[2].findings.len(), 1);
        assert!(groups[1].label.starts_with(GROUP_LABEL_PREFIX));
    }

    #[test]
    fn refusing_absolute_or_parent_paths_prevents_path_leaks() {
        assert!(normalize_report_path("/Users/private/src/lib.rs").is_err());
        assert!(normalize_report_path("C:\\Users\\private\\src\\lib.rs").is_err());
        assert!(normalize_report_path("../private.rs").is_err());
        assert_eq!(normalize_report_path("src\\lib.rs").unwrap(), "src/lib.rs");
    }

    #[test]
    fn context_uses_fingerprint_and_action_without_exporting_evidence() {
        let report = report(vec![finding("src/a.rs", Some("parse"), "unsafe input")]);
        let finding = &report.sections.findings[0];
        let body = finding_comment(&report, finding, "src/a.rs").unwrap();
        assert!(body.contains(&format!("{FINDING_MARKER_PREFIX}{}", finding.fingerprint())));
        assert!(body.contains("Suggested action: Fix the warning"));
        assert!(!body.contains("do-not-export"));
    }

    #[test]
    fn dry_run_reads_saved_report_without_credentials_or_network() {
        let report = report(vec![finding("src/a.rs", Some("parse"), "unsafe input")]);
        let input = std::env::temp_dir().join(format!(
            "scorecard-waypoint-export-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&input, serde_json::to_vec(&report).unwrap()).unwrap();

        let result = run_export(ExportArgs {
            waypoint: true,
            project: "demo".into(),
            input: input.clone(),
            api_url: None,
            token_env: "SC_WAYPOINT_TOKEN_THAT_IS_NOT_SET".into(),
            agent_id: None,
            dry_run: true,
        });

        fs::remove_file(input).unwrap();
        assert!(result.is_ok());
    }

    #[derive(Default)]
    struct FakeWaypoint {
        tickets: Vec<WaypointTicket>,
        fail_next_comment: bool,
    }

    impl Waypoint for FakeWaypoint {
        fn board(&mut self, _project: &str, _label: &str) -> Result<Vec<WaypointTicket>, String> {
            Ok(self.tickets.clone())
        }

        fn create(
            &mut self,
            _project: &str,
            title: &str,
            labels: &[&str],
        ) -> Result<WaypointTicket, String> {
            let ticket = WaypointTicket {
                id: format!("ticket-{}", self.tickets.len() + 1),
                title: title.to_owned(),
                version: 1,
                labels: labels.iter().map(|label| (*label).to_owned()).collect(),
                archived: false,
                comments: Vec::new(),
            };
            self.tickets.push(ticket.clone());
            Ok(ticket)
        }

        fn comment(
            &mut self,
            _project: &str,
            id: &str,
            _version: u64,
            body: &str,
        ) -> Result<WaypointTicket, String> {
            if self.fail_next_comment {
                self.fail_next_comment = false;
                return Err("injected interruption".into());
            }
            let ticket = self
                .tickets
                .iter_mut()
                .find(|ticket| ticket.id == id)
                .unwrap();
            ticket.comments.push(WaypointComment {
                body: Some(body.to_owned()),
            });
            ticket.version += 1;
            Ok(ticket.clone())
        }
    }

    #[test]
    fn retries_after_ticket_creation_resume_without_duplicate_tickets_or_comments() {
        let original_report = report(vec![
            finding("src/a.rs", Some("parse"), "first"),
            finding("src/a.rs", Some("parse"), "second"),
        ]);
        let groups = group_findings(&original_report.sections.findings).unwrap();
        let mut waypoint = FakeWaypoint {
            fail_next_comment: true,
            ..FakeWaypoint::default()
        };

        assert!(export_groups(&mut waypoint, "scorecard", &original_report, &groups).is_err());
        assert_eq!(waypoint.tickets.len(), 1);
        assert!(waypoint.tickets[0].comments.is_empty());

        let summary = export_groups(&mut waypoint, "scorecard", &original_report, &groups).unwrap();
        assert_eq!(waypoint.tickets.len(), 1);
        assert_eq!(waypoint.tickets[0].comments.len(), 2);
        assert_eq!(summary.created, 0);
        assert_eq!(summary.updated, 2);

        let refreshed_report = report(vec![
            finding("src/a.rs", Some("parse"), "first"),
            finding("src/a.rs", Some("parse"), "second"),
        ]);
        assert_ne!(original_report.identity.id, refreshed_report.identity.id);
        let refreshed_groups = group_findings(&refreshed_report.sections.findings).unwrap();
        let second = export_groups(
            &mut waypoint,
            "scorecard",
            &refreshed_report,
            &refreshed_groups,
        )
        .unwrap();
        assert_eq!(waypoint.tickets.len(), 1);
        assert_eq!(waypoint.tickets[0].comments.len(), 2);
        assert_eq!(second.unchanged, 2);
        assert_eq!(second.updated, 0);
    }

    #[test]
    fn http_export_uses_v1_and_second_run_is_unchanged() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            let mut comment_body = String::new();
            for request_index in 0..4 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0; 4096];
                loop {
                    let count = stream.read(&mut chunk).unwrap();
                    if count == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&chunk[..count]);
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some((headers, body)) = text.split_once("\r\n\r\n") {
                        let content_length = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                            .unwrap_or(0);
                        if body.len() >= content_length {
                            break;
                        }
                    }
                }
                let text = String::from_utf8(bytes).unwrap();
                let (headers, body) = text.split_once("\r\n\r\n").unwrap();
                let request_line = headers.lines().next().unwrap().to_owned();
                requests.push((request_line, headers.to_owned()));
                let body_value: serde_json::Value = if body.is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(body).unwrap()
                };
                let response = match request_index {
                    0 => json!({ "project": "demo", "tickets": [] }),
                    1 => json!({
                        "id": "SC-1", "title": body_value["title"], "version": 1,
                        "labels": body_value["labels"], "archived": false, "comments": []
                    }),
                    2 => {
                        comment_body = body_value["body"].as_str().unwrap().to_owned();
                        json!({
                            "id": "SC-1", "title": "Scorecard findings: parse (src/a.rs)",
                            "version": 2, "labels": [EXPORT_LABEL, group_label_for_test()],
                            "archived": false, "comments": [{"body": comment_body}]
                        })
                    }
                    _ => json!({
                        "project": "demo", "tickets": [{
                            "id": "SC-1", "title": "Scorecard findings: parse (src/a.rs)",
                            "version": 2, "labels": [EXPORT_LABEL, group_label_for_test()],
                            "archived": false, "comments": [{"body": comment_body}]
                        }]
                    }),
                };
                let payload = serde_json::to_vec(&response).unwrap();
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len()).unwrap();
                stream.write_all(&payload).unwrap();
            }
            requests
        });

        let original_report = report(vec![finding("src/a.rs", Some("parse"), "unsafe input")]);
        let refreshed_report = report(vec![finding("src/a.rs", Some("parse"), "unsafe input")]);
        assert_ne!(original_report.identity.id, refreshed_report.identity.id);
        let input = std::env::temp_dir().join(format!(
            "scorecard-waypoint-http-export-{}.json",
            std::process::id()
        ));
        fs::write(&input, serde_json::to_vec(&original_report).unwrap()).unwrap();
        let token_env = "SC_TEST_WAYPOINT_API_TOKEN";
        env::set_var(token_env, "test-token");
        let args = || ExportArgs {
            waypoint: true,
            project: "demo".into(),
            input: input.clone(),
            api_url: Some(format!("http://{address}")),
            token_env: token_env.into(),
            agent_id: Some("scorecard.test".into()),
            dry_run: false,
        };
        assert!(run_export(args()).is_ok());
        fs::write(&input, serde_json::to_vec(&refreshed_report).unwrap()).unwrap();
        assert!(run_export(args()).is_ok());
        env::remove_var(token_env);
        fs::remove_file(input).unwrap();
        let requests = server.join().unwrap();
        assert!(requests[0].0.starts_with("GET /v1/projects/demo/board?"));
        assert!(requests[1].0.starts_with("POST /v1/tickets "));
        assert!(requests[2]
            .0
            .starts_with("POST /v1/tickets/SC-1/comments?project=demo "));
        for (_, headers) in &requests {
            assert!(headers.contains("Authorization: Bearer test-token"));
            assert!(headers.contains("X-Waypoint-Act-As: scorecard.test"));
        }
    }

    fn group_label_for_test() -> String {
        group_findings(&[finding("src/a.rs", Some("parse"), "unsafe input")]).unwrap()[0]
            .label
            .clone()
    }
}
