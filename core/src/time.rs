//! Timestamps as the extension reads them: `capturedAt` goes through `Date.parse`, and a value it
//! cannot parse counts as infinitely old.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn num(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Epoch milliseconds of an ISO 8601 / RFC 3339 timestamp: `YYYY-MM-DD`, optionally followed by
/// `THH:MM[:SS[.fraction]]` and `Z` or a `±HH:MM` offset. Both writers use this shape: hook.py
/// (`datetime.isoformat()`, microseconds and `+00:00`) and the extension (`toISOString()`, `Z`).
/// A value with no offset is read as UTC. Anything else is `None`.
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = match s.find(['T', 't', ' ']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mut dp = date.split('-');
    let (y, mo, d) = (dp.next()?, dp.next()?, dp.next()?);
    if dp.next().is_some() || y.len() != 4 || mo.len() != 2 || d.len() != 2 {
        return None;
    }
    let (y, mo, d) = (num(y)?, num(mo)?, num(d)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let mut ms = days_from_civil(y, mo, d) * 86_400_000;
    let Some(rest) = rest else { return Some(ms) };

    let (clock, offset_ms) = if let Some(c) = rest.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(i) = rest.rfind(['+', '-']) {
        let (c, off) = rest.split_at(i);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let off = &off[1..];
        let (oh, om) = match off.split_once(':') {
            Some((h, m)) => (h, m),
            None if off.len() == 4 => off.split_at(2),
            None => return None,
        };
        if oh.len() != 2 || om.len() != 2 {
            return None;
        }
        let (oh, om) = (num(oh)?, num(om)?);
        if oh > 23 || om > 59 {
            return None;
        }
        (c, sign * (oh * 3_600_000 + om * 60_000))
    } else {
        (rest, 0)
    };
    let (hms, frac) = match clock.split_once('.') {
        Some((a, f)) => (a, Some(f)),
        None => (clock, None),
    };
    let mut parts = hms.split(':');
    let (h, mi) = (parts.next()?, parts.next()?);
    let sec = parts.next();
    if parts.next().is_some() || h.len() != 2 || mi.len() != 2 {
        return None;
    }
    let (h, mi) = (num(h)?, num(mi)?);
    let sec = match sec {
        Some(x) if x.len() == 2 => num(x)?,
        Some(_) => return None,
        None if frac.is_some() => return None,
        None => 0,
    };
    if h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let frac_ms = match frac {
        Some(f) => {
            num(f)?;
            let mut digits: String = f.chars().take(3).collect();
            while digits.len() < 3 {
                digits.push('0');
            }
            num(&digits)?
        }
        None => 0,
    };
    ms += h * 3_600_000 + mi * 60_000 + sec * 1000 + frac_ms;
    Some(ms - offset_ms)
}

#[cfg(test)]
mod tests {
    use super::parse_iso_ms as p;

    #[test]
    fn writers_formats() {
        assert_eq!(p("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(p("1970-01-01T00:00:01.5Z"), Some(1500));
        // hook.py: microseconds, +00:00
        assert_eq!(p("2026-11-13T10:00:00.123456+00:00"), Some(1_794_564_000_123));
        // extension: toISOString()
        assert_eq!(p("2026-11-13T10:00:00.123Z"), Some(1_794_564_000_123));
        assert_eq!(p("2026-11-13T17:00:00+07:00"), p("2026-11-13T10:00:00Z"));
        assert_eq!(p("2026-11-13T03:30:00-0630"), p("2026-11-13T10:00:00Z"));
        assert_eq!(p("2026-11-13"), Some(1_794_528_000_000));
        assert_eq!(p("2026-11-13T10:00"), p("2026-11-13T10:00:00Z"));
        assert_eq!(p("2024-02-29T00:00:00Z"), Some(1_709_164_800_000));
    }

    #[test]
    fn garbage_is_none() {
        for s in ["", "yesterday", "2026-13-01", "2026-11-13T25:00:00Z", "2026-11-13T10:00:00+7", "26-11-13", "2026-11-13T10:00:00.abcZ", "2026-11-13Tnope"] {
            assert_eq!(p(s), None, "{s}");
        }
    }
}
