//! Minimal read-only JSONC parser (comments + trailing commas) for VS Code settings files.

/// Parse JSONC text. Returns `None` on any error (including an unterminated block comment).
pub fn parse_jsonc(text: &str) -> Option<serde_json::Value> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let cleaned = strip(text)?;
    serde_json::from_str(&cleaned).ok()
}

/// Pass 1 drops comments; pass 2 drops commas that directly precede `}` or `]`.
/// Both passes track string state so `//` or `,}` inside a string survive.
fn strip(src: &str) -> Option<String> {
    let c: Vec<char> = src.chars().collect();
    let mut no_comments = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < c.len() {
        let ch = c[i];
        if in_str {
            no_comments.push(ch);
            if ch == '\\' && i + 1 < c.len() {
                no_comments.push(c[i + 1]);
                i += 1;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
            no_comments.push(ch);
        } else if ch == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        } else if ch == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            loop {
                if i + 1 >= c.len() {
                    return None;
                }
                if c[i] == '*' && c[i + 1] == '/' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            no_comments.push(' ');
        } else {
            no_comments.push(ch);
        }
        i += 1;
    }

    let c: Vec<char> = no_comments.chars().collect();
    let mut out = String::with_capacity(c.len());
    let mut in_str = false;
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if in_str {
            out.push(ch);
            if ch == '\\' && i + 1 < c.len() {
                out.push(c[i + 1]);
                i += 1;
            } else if ch == '"' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
            out.push(ch);
        } else if ch == ',' {
            let mut j = i + 1;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if !(j < c.len() && (c[j] == '}' || c[j] == ']')) {
                out.push(ch);
            }
        } else {
            out.push(ch);
        }
        i += 1;
    }
    Some(out)
}
