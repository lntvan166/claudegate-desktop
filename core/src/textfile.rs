use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFile {
    Missing,
    Binary,
    Text(String),
}

/// Reads a file for display. Invalid UTF-8 or a NUL in the first 8 KiB is `Binary`.
pub fn read_text(path: &Path) -> TextFile {
    let Ok(bytes) = std::fs::read(path) else {
        return TextFile::Missing;
    };
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return TextFile::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(s) => TextFile::Text(s),
        Err(_) => TextFile::Binary,
    }
}
