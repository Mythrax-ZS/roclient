//! data\msgstringtable.txt: the client's numbered messages. The server refers to them as
//! `<MSG>id</MSG>` or `<MSG>id,arg,...</MSG>` (mail titles and senders, for one).

use std::sync::OnceLock;

static TABLE: OnceLock<Vec<String>> = OnceLock::new();

pub fn load(vfs: &ro_vfs::Vfs) {
    // Every layer's copy, merged line by line: a translation pack's wording wins, and the
    // base data fills ids the pack is missing or leaves blank (its table can be shorter).
    let mut lines: Vec<String> = vec![];
    for d in vfs.read_all(r"data\msgstringtable.txt").unwrap_or_default() {
        let (text, _, _) = encoding_rs::EUC_KR.decode(&d);
        for (i, l) in text.lines().enumerate() {
            let l = l.strip_suffix('#').unwrap_or(l);
            match lines.get_mut(i) {
                Some(have) if have.trim().is_empty() => *have = l.to_string(),
                Some(_) => {}
                None => lines.push(l.to_string()),
            }
        }
    }
    if lines.is_empty() {
        return;
    }
    tracing::info!(lines = lines.len(), "message strings");
    let _ = TABLE.set(lines);
}

/// Line `id` (zero-based) with its %d / %s filled from `args`.
fn line(id: usize, args: &[&str]) -> Option<String> {
    let s = TABLE.get()?.get(id)?.clone();
    let mut out = String::new();
    let mut args = args.iter();
    let mut rest = s.as_str();
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let spec = rest[i + 1..].chars().next();
        match spec {
            Some('%') => out.push('%'),
            Some('d' | 's' | 'u') => out.push_str(args.next().copied().unwrap_or("")),
            Some(c) => {
                out.push('%');
                out.push(c);
            }
            None => out.push('%'),
        }
        rest = &rest[i + 1 + spec.map_or(0, char::len_utf8)..];
    }
    out.push_str(rest);
    Some(out)
}

/// Replaces every `<MSG>...</MSG>` in `text`; unknown or blank ones become `fallback`.
pub fn resolve(text: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let (Some(a), Some(b)) = (rest.find("<MSG>"), rest.find("</MSG>")) {
        if b < a {
            break;
        }
        out.push_str(&rest[..a]);
        let inner = &rest[a + 5..b];
        let mut parts = inner.split(',');
        let id = parts.next().and_then(|n| n.trim().parse::<usize>().ok());
        let args: Vec<&str> = parts.collect();
        let s = id.and_then(|id| line(id, &args)).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| fallback.to_string());
        out.push_str(&s);
        rest = &rest[b + 6..];
    }
    out.push_str(rest);
    out
}

/// Message `id` with its %d / %s filled from `args` (None when the table lacks it).
pub fn get(id: usize, args: &[&str]) -> Option<String> {
    line(id, args).filter(|s| !s.trim().is_empty())
}
