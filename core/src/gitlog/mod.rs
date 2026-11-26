//! Read-only git log: parsing of `git log` / `git show` output and remote URLs.

pub mod parse;
pub mod remote;

pub use parse::{is_sha, parse_changes, parse_log, ChangedFile, Commit, FileStatus, unquote_c_style, LOG_FORMAT};
pub use remote::web_url;
