//! Read-only git log: parsing of `git log` / `git show` output and remote URLs, filters as
//! git argv, and the paged merge across repositories.

pub mod combined;
pub mod discover;
pub mod filter;
pub mod history;
pub mod line_merge;
pub mod merge;
pub mod parse;
pub mod remote;

pub use combined::{
    combine_repo, exclude_for, left_and_right, left_side, merge_base_args, parse_range_log, range_log_args, read_combined, show_raw_args, Blob, CombinedFile, CombinedRead, Fallback, GitRead, LeftSide, Mode, NetStatus, RangeCommit, RawChange, ReadError, RepoRead, COMBINED_LIMIT, GITLINK_MODE, MERGE_LIMIT_BYTES,
    MERGE_LIMIT_LINES, RANGE_FORMAT, SLOW_FILES_AT_ONCE,
};
pub use discover::{ahead_behind, discover, par_map, par_map_limit, sanitize_git_env, Checkout, GitRun, RealGit, Repo, DEFAULT_SCAN_DEPTH, PALETTE_LEN, PARALLEL};
pub use filter::{author_patterns, git_date, heads_for, is_valid_ref, log_args, normalize_path, pathspec_of, repo_searched, BranchUse, Filter};
pub use history::{history_args, history_paths_args, parse_history, parse_history_paths, HistoryFile, HISTORY_FORMAT};
pub use line_merge::{merge3, MergeResult, MAX_EDITS};
pub use merge::{compare_commits, take_ready, RepoProgress};
pub use parse::{is_sha, parse_changes, parse_log, ChangedFile, Commit, FileStatus, unquote_c_style, LOG_FIELDS, LOG_FORMAT, count_records};
pub use remote::web_url;
