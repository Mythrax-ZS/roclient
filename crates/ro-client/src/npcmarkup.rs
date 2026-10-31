//! NPC text markup, as the original client reads it in dialogs:
//!   ^RRGGBB colours (^000000 back to the default), <FONT COLOR = RRGGBB> and
//!   <FONT SIZE = n> ... </FONT>, <B> ... </B>, and links whose target sits in <INFO>:
//!   <NAVI>label<INFO>map,x,y,...</INFO></NAVI> guides there (the Navigation window),
//!   <URL>label<INFO>address,width,height</INFO></URL> opens a web page,
//!   <TIPBOX>label<INFO>id</INFO></TIPBOX> names a tip, <ITEML>/<ITEM> link an item.
//! Anything else in angle brackets stays as text.

use egui::Color32;

use crate::itemlink;

/// What a link does when clicked.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Navi { map: String, x: u16, y: u16 },
    Url(String),
    Tip(u32),
    Item(itemlink::Link),
}

/// A run of text in one style.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    /// None: the default colour.
    pub color: Option<Color32>,
    /// Font size relative to the original's 12.
    pub scale: f32,
    pub link: Option<Action>,
}

#[derive(Clone, Copy)]
struct Style {
    color: Option<Color32>,
    scale: f32,
}

fn hex(code: &str) -> Option<Option<Color32>> {
    if code.len() != 6 || !code.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(code, 16).ok()?;
    Some((v != 0).then(|| Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)))
}

/// Case-insensitive `starts_with` for ASCII tags.
fn starts(s: &str, tag: &str) -> bool {
    s.get(..tag.len()).is_some_and(|p| p.eq_ignore_ascii_case(tag))
}

/// Byte offset of `tag` in `s`, ignoring ASCII case.
fn find(s: &str, tag: &str) -> Option<usize> {
    s.to_ascii_uppercase().find(&tag.to_ascii_uppercase())
}

/// A link's target from its <INFO> text.
fn action(kind: &str, info: &str) -> Option<Action> {
    let f: Vec<&str> = info.split(',').map(str::trim).collect();
    match kind {
        "NAVI" => Some(Action::Navi { map: f.first()?.to_string(), x: f.get(1)?.parse().ok()?, y: f.get(2)?.parse().ok()? }),
        "URL" => Some(Action::Url(f.first()?.to_string())),
        "TIPBOX" => Some(Action::Tip(f.first()?.parse().ok()?)),
        _ => None,
    }
}

/// Splits NPC text into styled spans.
pub fn parse(text: &str) -> Vec<Span> {
    let mut out: Vec<Span> = vec![];
    let mut style = Style { color: None, scale: 1.0 };
    let mut fonts: Vec<Style> = vec![];
    let mut buf = String::new();
    let flush = |out: &mut Vec<Span>, buf: &mut String, style: Style| {
        if !buf.is_empty() {
            out.push(Span { text: std::mem::take(buf), color: style.color, scale: style.scale, link: None });
        }
    };
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '^' {
            if let Some(color) = rest.get(1..7).and_then(hex) {
                flush(&mut out, &mut buf, style);
                style.color = color;
                rest = &rest[7..];
                continue;
            }
        } else if c == '<' {
            // Links: <KIND>label<INFO>target</INFO></KIND>.
            if let Some(kind) = ["NAVI", "URL", "TIPBOX"].into_iter().find(|k| starts(rest, &format!("<{k}>"))) {
                let open = kind.len() + 2;
                let close = format!("</{kind}>");
                if let Some(end) = find(&rest[open..], &close) {
                    let inner = &rest[open..open + end];
                    let (label, info) = match find(inner, "<INFO>") {
                        Some(i) => (&inner[..i], inner[i + 6..].split("</INFO>").next().unwrap_or("")),
                        None => (inner, ""),
                    };
                    flush(&mut out, &mut buf, style);
                    let link = action(kind, info);
                    for mut s in parse(label) {
                        s.color = s.color.or(style.color);
                        s.scale *= style.scale;
                        s.link = link.clone();
                        out.push(s);
                    }
                    rest = &rest[open + end + close.len()..];
                    continue;
                }
            }
            if starts(rest, "<ITEML>") || starts(rest, "<ITEM>") {
                let close = if starts(rest, "<ITEML>") { "</ITEML>" } else { "</ITEM>" };
                if let Some(end) = find(rest, close).map(|e| e + close.len()) {
                    if let [itemlink::Piece::Item(link)] = itemlink::split(&rest[..end])[..] {
                        flush(&mut out, &mut buf, style);
                        out.push(Span { text: String::new(), color: style.color, scale: style.scale, link: Some(Action::Item(link)) });
                        rest = &rest[end..];
                        continue;
                    }
                }
            }
            if starts(rest, "<FONT") {
                if let Some(end) = rest.find('>') {
                    flush(&mut out, &mut buf, style);
                    fonts.push(style);
                    let attrs = rest[5..end].replace(' ', "");
                    for attr in attrs.split(|c| c == ';' || c == ',') {
                        if let Some(v) = attr.get(..6).filter(|k| k.eq_ignore_ascii_case("COLOR=")).and(attr.get(6..)) {
                            if let Some(color) = hex(v.trim_start_matches('#')) {
                                style.color = color;
                            }
                        } else if let Some(v) = attr.get(..5).filter(|k| k.eq_ignore_ascii_case("SIZE=")).and(attr.get(5..)) {
                            if let Ok(n) = v.parse::<f32>() {
                                style.scale = (n / 12.0).clamp(0.5, 2.0);
                            }
                        }
                    }
                    rest = &rest[end + 1..];
                    continue;
                }
            }
            if let Some(tag) = ["</FONT>", "<B>", "</B>"].into_iter().find(|t| starts(rest, t)) {
                flush(&mut out, &mut buf, style);
                if tag == "</FONT>" {
                    style = fonts.pop().unwrap_or(style);
                }
                rest = &rest[tag.len()..];
                continue;
            }
        }
        buf.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut out, &mut buf, style);
    out
}

/// Whether any span is a link (the line needs clickable pieces).
pub fn has_links(spans: &[Span]) -> bool {
    spans.iter().any(|s| s.link.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navi_link_with_coloured_label() {
        let s = parse("Find the <NAVI>^4D4DFF[shepherd]^000000<INFO>lasa_fild01,353,370,000,1,</INFO></NAVI> now.");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].text, "Find the ");
        assert_eq!(s[1].text, "[shepherd]");
        assert_eq!(s[1].color, Some(Color32::from_rgb(0x4d, 0x4d, 0xff)));
        assert_eq!(s[1].link, Some(Action::Navi { map: "lasa_fild01".into(), x: 353, y: 370 }));
        assert_eq!(s[2].text, " now.");
        assert_eq!(s[2].link, None);
    }

    #[test]
    fn plain_navi_from_the_carocc_dialog() {
        let s = parse("press <NAVI>[Hun]<INFO>izlude,122,207</INFO></NAVI> here");
        assert_eq!(s.iter().map(|s| s.text.as_str()).collect::<String>(), "press [Hun] here");
        assert_eq!(s[1].link, Some(Action::Navi { map: "izlude".into(), x: 122, y: 207 }));
    }

    #[test]
    fn font_tags_and_bold_are_styles_not_text() {
        let s = parse("Prefer <FONT SIZE = 14>^dd5555<B>Summoner</B>^000000</FONT> class.");
        assert_eq!(s.iter().map(|s| s.text.as_str()).collect::<String>(), "Prefer Summoner class.");
        let word = s.iter().find(|s| s.text == "Summoner").unwrap();
        assert_eq!(word.color, Some(Color32::from_rgb(0xdd, 0x55, 0x55)));
        assert!((word.scale - 14.0 / 12.0).abs() < 1e-4);
        assert_eq!(s.last().unwrap().scale, 1.0);
    }

    #[test]
    fn font_colour_and_url_and_tipbox() {
        let s = parse("<FONT COLOR = 0000CD>blue</FONT> <URL>[guide]<INFO>http://ro.gnjoy.com/guide/,1024,768</INFO></URL> <TIPBOX>[List]<INFO>9163</INFO></TIPBOX>");
        assert_eq!(s[0].color, Some(Color32::from_rgb(0, 0, 0xcd)));
        assert_eq!(s.iter().find(|s| s.text == "[guide]").unwrap().link, Some(Action::Url("http://ro.gnjoy.com/guide/".into())));
        assert_eq!(s.iter().find(|s| s.text == "[List]").unwrap().link, Some(Action::Tip(9163)));
    }

    #[test]
    fn stray_brackets_and_carets_stay() {
        let s = parse("a < b ^ c <UNKNOWN> 100^");
        assert_eq!(s.iter().map(|s| s.text.as_str()).collect::<String>(), "a < b ^ c <UNKNOWN> 100^");
    }
}
