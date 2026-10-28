//! data\msgstringtable.txt: the client's numbered messages. The server refers to them as
//! `<MSG>id</MSG>` or `<MSG>id,arg,...</MSG>` (mail titles and senders, for one).

use std::sync::OnceLock;

static TABLE: OnceLock<Vec<String>> = OnceLock::new();

pub fn load(vfs: &ro_vfs::Vfs) {
    let Some(d) = vfs.read(r"data\msgstringtable.txt").ok().flatten() else { return };
    let (text, _, _) = encoding_rs::EUC_KR.decode(&d);
    let lines: Vec<String> = text.lines().map(|l| l.strip_suffix('#').unwrap_or(l).to_string()).collect();
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
