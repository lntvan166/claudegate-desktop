//! A commit's web page from a remote URL. Pure and defensive: the remote comes
//! from repository config, so only an http(s) page on the remote's own host is
//! built, never with userinfo.

use regex::Regex;
use std::sync::OnceLock;

struct Remote {
    scheme: &'static str,
    host: String,
    port: Option<String>,
    path: String,
}

fn re(cell: &'static OnceLock<Regex>, p: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(p).unwrap())
}

fn parse_remote(url: &str) -> Option<Remote> {
    static WIN: OnceLock<Regex> = OnceLock::new();
    static SCP: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    let u = url.trim();
    if re(&WIN, r"^[A-Za-z]:[\\/]").is_match(u) {
        return None;
    }
    if !u.contains("://") {
        // scp-like: [user@]host:owner/repo.git
        let m = re(&SCP, r"^(?:[^@/\s]+@)?([^@:/\s\[\]\\]+):([^/].*)$").captures(u)?;
        return Some(Remote { scheme: "https", host: m[1].to_string(), port: None, path: m[2].to_string() });
    }
    let m = re(&URL, r"^([A-Za-z][A-Za-z0-9+.-]*)://(?:[^/@]*@)?([^/:@\s\[\]\\]+)(?::(\d+))?(/.*)?$").captures(u)?;
    let path = m.get(4).map_or("", |p| p.as_str()).to_string();
    match m[1].to_ascii_lowercase().as_str() {
        "ssh" | "git" | "git+ssh" => Some(Remote { scheme: "https", host: m[2].to_string(), port: None, path }),
        "https" => Some(Remote { scheme: "https", host: m[2].to_string(), port: m.get(3).map(|p| p.as_str().to_string()), path }),
        "http" => Some(Remote { scheme: "http", host: m[2].to_string(), port: m.get(3).map(|p| p.as_str().to_string()), path }),
        _ => None,
    }
}

pub fn web_url(remote: &str, sha: &str) -> Option<String> {
    static SEG: OnceLock<Regex> = OnceLock::new();
    static AZ: OnceLock<Regex> = OnceLock::new();
    if !super::parse::is_sha(sha) {
        return None;
    }
    let r = parse_remote(remote)?;
    let host = r.host.to_ascii_lowercase();
    if !host.starts_with(|c: char| c.is_ascii_alphanumeric()) || !host.ends_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let trimmed = r.path.trim_end_matches('/');
    let trimmed = if trimmed.len() >= 4 && trimmed[trimmed.len() - 4..].eq_ignore_ascii_case(".git") {
        &trimmed[..trimmed.len() - 4]
    } else {
        trimmed
    };
    let segs: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    let seg = re(&SEG, r"^[A-Za-z0-9._~%+-]+$");
    if segs.is_empty() || segs.iter().any(|s| !seg.is_match(s) || *s == "." || *s == ".." || has_encoded_dot_or_slash(s)) {
        return None;
    }
    let path = segs.join("/");
    let base = format!("{}://{}{}", r.scheme, host, r.port.map(|p| format!(":{p}")).unwrap_or_default());

    let az = re(&AZ, r"^v3/([^/]+)/([^/]+)/([^/]+)$").captures(&path);
    if host == "ssh.dev.azure.com" || host == "vs-ssh.visualstudio.com" {
        let c = az?;
        return Some(format!("https://dev.azure.com/{}/{}/_git/{}/commit/{sha}", &c[1], &c[2], &c[3]));
    }
    if host == "dev.azure.com" || host.ends_with(".visualstudio.com") {
        return Some(format!("{base}/{path}/commit/{sha}"));
    }
    if host == "bitbucket.org" {
        return Some(format!("{base}/{path}/commits/{sha}"));
    }
    if host == "gitlab.com" || host.starts_with("gitlab.") {
        return Some(format!("{base}/{path}/-/commit/{sha}"));
    }
    if host == "github.com" || host.starts_with("github.") {
        return Some(format!("{base}/{path}/commit/{sha}"));
    }
    None
}

/// `%2e` / `%2f` (any case) smuggle a dot segment or a slash past the segment check.
fn has_encoded_dot_or_slash(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    l.contains("%2e") || l.contains("%2f")
}
