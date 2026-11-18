//! Order-keeping JSON for `~/.claude/settings.json`: parsed as `JSON.parse` would and printed as
//! `JSON.stringify` would, so a file the app writes is byte-for-byte what the extension writes for
//! the same input. `serde_json::Value` sorts object keys (no `preserve_order` here, and turning it
//! on would change every other map in the crate), which would reorder the user's whole config.
//!
//! JavaScript's rules kept here: object keys in insertion order except array-index keys
//! (`"0"`..`"4294967294"`), which come first in ascending order; a duplicate key keeps its first
//! position and its last value; every number is a double, printed by `Number.prototype.toString`;
//! strings escape only `"`, `\` and control characters (`\u00xx` lowercase). Numbers parse with
//! serde_json's `float_roundtrip`, so a long literal rounds to the same double `JSON.parse` gives.

use serde::de::{Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum OJson {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<OJson>),
    Obj(Vec<(String, OJson)>),
}

impl OJson {
    /// `JSON.parse`, strict (no comments, no trailing commas).
    pub fn parse(text: &str) -> Result<OJson, serde_json::Error> {
        let mut de = serde_json::Deserializer::from_str(text);
        let v = OJson::deserialize(&mut de)?;
        de.end()?;
        Ok(v)
    }

    pub fn get(&self, key: &str) -> Option<&OJson> {
        match self {
            OJson::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut OJson> {
        match self {
            OJson::Obj(m) => m.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// `obj[key] = value`: replaces in place, or appends. No-op on a non-object.
    pub fn set(&mut self, key: &str, value: OJson) {
        if let OJson::Obj(m) = self {
            match m.iter_mut().find(|(k, _)| k == key) {
                Some(slot) => slot.1 = value,
                None => m.push((key.to_owned(), value)),
            }
        }
    }

    /// An object's keys in JavaScript's enumeration order (empty for a non-object).
    pub fn keys(&self) -> Vec<&str> {
        match self {
            OJson::Obj(m) => ordered(m).into_iter().map(|(k, _)| k.as_str()).collect(),
            _ => Vec::new(),
        }
    }

    /// JavaScript truthiness (`!x` is false).
    pub fn truthy(&self) -> bool {
        match self {
            OJson::Null => false,
            OJson::Bool(b) => *b,
            OJson::Num(n) => *n != 0.0 && !n.is_nan(),
            OJson::Str(s) => !s.is_empty(),
            OJson::Arr(_) | OJson::Obj(_) => true,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            OJson::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `JSON.stringify(v)`.
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        write_value(self, None, "", &mut out);
        out
    }

    /// `JSON.stringify(v, null, 2)`.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        write_value(self, Some("  "), "", &mut out);
        out
    }
}

/// A canonical array index: what JavaScript enumerates first, in numeric order.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    let n: u64 = key.parse().ok()?;
    (n < u32::MAX as u64).then_some(n as u32)
}

fn ordered(m: &[(String, OJson)]) -> Vec<&(String, OJson)> {
    let mut idx: Vec<(u32, &(String, OJson))> = m.iter().filter_map(|e| array_index(&e.0).map(|i| (i, e))).collect();
    idx.sort_by_key(|(i, _)| *i);
    let mut out: Vec<&(String, OJson)> = idx.into_iter().map(|(_, e)| e).collect();
    out.extend(m.iter().filter(|e| array_index(&e.0).is_none()));
    out
}

fn write_value(v: &OJson, gap: Option<&str>, indent: &str, out: &mut String) {
    match v {
        OJson::Null => out.push_str("null"),
        OJson::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        OJson::Num(n) => out.push_str(&js_number(*n)),
        OJson::Str(s) => quote(s, out),
        OJson::Arr(a) => {
            if a.is_empty() {
                out.push_str("[]");
                return;
            }
            write_list(a.iter().map(|x| (None, x)), gap, indent, '[', ']', out);
        }
        OJson::Obj(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            write_list(ordered(m).into_iter().map(|(k, x)| (Some(k.as_str()), x)), gap, indent, '{', '}', out);
        }
    }
}

fn write_list<'a>(
    items: impl Iterator<Item = (Option<&'a str>, &'a OJson)>,
    gap: Option<&str>,
    indent: &str,
    open: char,
    close: char,
    out: &mut String,
) {
    let inner = format!("{indent}{}", gap.unwrap_or(""));
    out.push(open);
    for (i, (key, x)) in items.enumerate() {
        if i > 0 {
            out.push(',');
        }
        if gap.is_some() {
            out.push('\n');
            out.push_str(&inner);
        }
        if let Some(k) = key {
            quote(k, out);
            out.push(':');
            if gap.is_some() {
                out.push(' ');
            }
        }
        write_value(x, gap, &inner, out);
    }
    if gap.is_some() {
        out.push('\n');
        out.push_str(indent);
    }
    out.push(close);
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `(digits, exponent)` of `{:e}`: `9.5e2` is `("95", 2)`.
fn sci_parts(sci: &str) -> (String, i64) {
    let (mant, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    (mant.chars().filter(|c| *c != '.').collect(), exp.parse().unwrap_or(0))
}

/// Adds one to the last digit of a digit string; `None` when that carries into a new digit.
fn bump(d: &str) -> Option<String> {
    let mut b = d.as_bytes().to_vec();
    for i in (0..b.len()).rev() {
        if b[i] == b'9' {
            b[i] = b'0';
        } else {
            b[i] += 1;
            return String::from_utf8(b).ok();
        }
    }
    None
}

/// The shortest round-trip digits ECMA's Number::toString picks for `x > 0`. Rust's `{:e}` gives
/// shortest round-trip digits too, but on an exact tie between two of them (the value lies
/// precisely halfway, e.g. 963.96636962890625) ECMA takes the even one and Rust may not.
fn shortest_digits(x: f64) -> (String, i64) {
    let (d, e) = sci_parts(&format!("{x:e}"));
    // An f64 has at most 767 significant decimal digits, so this is its exact value.
    let (exact, ee) = sci_parts(&format!("{x:.800e}"));
    let k = d.len();
    if ee != e || exact.len() <= k || d.as_bytes()[k - 1] % 2 == 0 {
        return (d, e);
    }
    let rest = exact[k..].trim_end_matches('0');
    if rest != "5" {
        return (d, e); // not a tie
    }
    let lower = exact[..k].to_owned();
    let other = if d == lower { bump(&lower) } else { Some(lower) };
    let parses_back = |c: &str| {
        let txt = format!("{}{}{}e{e}", &c[..1], if c.len() > 1 { "." } else { "" }, &c[1..]);
        txt.parse::<f64>().ok() == Some(x)
    };
    match other {
        Some(o) if o.as_bytes()[k - 1] % 2 == 0 && parses_back(&o) => (o, e),
        _ => (d, e),
    }
}

/// `Number.prototype.toString()` (ECMA-262 Number::toString, radix 10). Rust's `{:e}` gives the
/// same shortest round-trip digits; only the layout differs.
pub fn js_number(n: f64) -> String {
    if n.is_nan() || n.is_infinite() {
        return "null".into(); // JSON.stringify's answer; JSON text cannot hold either anyway
    }
    if n == 0.0 {
        return "0".into();
    }
    let (digits, e) = shortest_digits(n.abs());
    let k = digits.len() as i64;
    let pos = e + 1; // ECMA's n: the decimal point sits after `pos` digits
    let mut s = String::new();
    if n < 0.0 {
        s.push('-');
    }
    if k <= pos && pos <= 21 {
        s.push_str(&digits);
        s.push_str(&"0".repeat((pos - k) as usize));
    } else if 0 < pos && pos <= 21 {
        s.push_str(&digits[..pos as usize]);
        s.push('.');
        s.push_str(&digits[pos as usize..]);
    } else if -6 < pos && pos <= 0 {
        s.push_str("0.");
        s.push_str(&"0".repeat((-pos) as usize));
        s.push_str(&digits);
    } else {
        s.push_str(&digits[..1]);
        if k > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('e');
        s.push(if pos - 1 >= 0 { '+' } else { '-' });
        s.push_str(&(pos - 1).abs().to_string());
    }
    s
}

impl<'de> Deserialize<'de> for OJson {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<OJson, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = OJson;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("any JSON value")
            }
            fn visit_unit<E>(self) -> Result<OJson, E> {
                Ok(OJson::Null)
            }
            fn visit_bool<E>(self, b: bool) -> Result<OJson, E> {
                Ok(OJson::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<OJson, E> {
                Ok(OJson::Num(n as f64))
            }
            fn visit_u64<E>(self, n: u64) -> Result<OJson, E> {
                Ok(OJson::Num(n as f64))
            }
            fn visit_f64<E>(self, n: f64) -> Result<OJson, E> {
                Ok(OJson::Num(n))
            }
            fn visit_str<E>(self, s: &str) -> Result<OJson, E> {
                Ok(OJson::Str(s.to_owned()))
            }
            fn visit_string<E>(self, s: String) -> Result<OJson, E> {
                Ok(OJson::Str(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<OJson, A::Error> {
                let mut v = Vec::new();
                while let Some(x) = a.next_element()? {
                    v.push(x);
                }
                Ok(OJson::Arr(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<OJson, A::Error> {
                let mut obj = OJson::Obj(Vec::new());
                while let Some((k, v)) = a.next_entry::<String, OJson>()? {
                    obj.set(&k, v); // a duplicate keeps its first position, last value wins
                }
                Ok(obj)
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_javascript() {
        for (n, s) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (1.5, "1.5"),
            (-2.5e-10, "-2.5e-10"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (0.000001, "0.000001"),
            (0.1, "0.1"),
            (123.456, "123.456"),
            (12345678901234567890.0, "12345678901234567000"),
            (963.96636962890625, "963.9663696289062"), // a tie: ECMA takes the even digit
            (5e-324, "5e-324"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
        ] {
            assert_eq!(js_number(n), s, "{n}");
        }
    }

    #[test]
    fn index_keys_first_then_insertion_order() {
        let v = OJson::parse(r#"{"b":1,"10":2,"a":3,"2":4,"01":5}"#).unwrap();
        assert_eq!(v.to_compact(), r#"{"2":4,"10":2,"b":1,"a":3,"01":5}"#);
    }

    #[test]
    fn strict_parse() {
        assert!(OJson::parse("{// c\n}").is_err());
        assert!(OJson::parse("[1,]").is_err());
        assert!(OJson::parse("{} x").is_err());
    }
}
