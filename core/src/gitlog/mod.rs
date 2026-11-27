//! Read-only git log: parsing of `git log` / `git show` output and remote URLs, filters as
//! git argv, and the paged merge across repositories.

pub mod discover;
pub mod filter;
pub mod merge;
pub mod parse;
pub mod remote;

pub use discover::{ahead_behind, discover, par_map, sanitize_git_env, Checkout, GitRun, RealGit, Repo, DEFAULT_SCAN_DEPTH, PALETTE_LEN, PARALLEL};
pub use filter::{author_patterns, git_date, heads_for, is_valid_ref, log_args, normalize_path, pathspec_of, repo_searched, BranchUse, Filter};
pub use merge::{compare_commits, take_ready, RepoProgress};
pub use parse::{is_sha, parse_changes, parse_log, ChangedFile, Commit, FileStatus, unquote_c_style, LOG_FIELDS, LOG_FORMAT, count_records};
pub use remote::web_url;
