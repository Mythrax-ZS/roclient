//! Item links in chat text: `<ITEML>...</ITEML>` (`<ITEM>` before 2016), as rAthena's
//! `create_item_link` writes them for @iteminfo, @whodrops, broadcasts and so on. The
//! original client draws each as the item's name, which opens the item's description when
//! clicked. Inside the tag (PACKETVER 20200724+), base 62 throughout:
//!   5-digit equip mask, '1' or '0' (equipment or not), the item id,
//!   then optional `%` refine, `&` look, `'` enchant grade,
//!   then `)` card (four of them), then `+` option `,` param `-` value.

/// One linked item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Link {
    pub id: u32,
    pub refine: u8,
    pub grade: u8,
    pub cards: [u32; 4],
    pub equip: bool,
}

/// A piece of a chat line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a str),
    Item(Link),
}

/// Splits `text` into plain text and item links. A tag that can't be read stays as text.
pub fn split(text: &str) -> Vec<Piece<'_>> {
    let mut out = vec![];
    let mut rest = text;
    while let Some((start, open)) = ["<ITEML>", "<ITEM>"].iter().filter_map(|t| rest.find(t).map(|i| (i, *t))).min() {
        let close = if open == "<ITEML>" { "</ITEML>" } else { "</ITEM>" };
        let body_at = start + open.len();
        let Some(len) = rest[body_at..].find(close) else { break };
        match parse(&rest[body_at..body_at + len]) {
            Some(link) => {
                if start > 0 {
                    out.push(Piece::Text(&rest[..start]));
                }
                out.push(Piece::Item(link));
            }
            None => out.push(Piece::Text(&rest[..body_at + len + close.len()])),
        }
        rest = &rest[body_at + len + close.len()..];
    }
    if !rest.is_empty() {
        out.push(Piece::Text(rest));
    }
    out
}

fn base62(s: &str) -> Option<u32> {
    s.chars().try_fold(0u32, |n, c| {
        let d = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 10,
            'A'..='Z' => c as u32 - 'A' as u32 + 36,
            _ => return None,
        };
        n.checked_mul(62)?.checked_add(d)
    })
}

/// Reads a tag's contents.
fn parse(body: &str) -> Option<Link> {
    if body.len() < 7 || !body.is_ascii() {
        return None;
    }
    let equip = &body[5..6] == "1";
    // The id runs to the first field marker (any of the separators of every version).
    let rest = &body[6..];
    let end = rest.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(rest.len());
    let id = base62(&rest[..end])?;
    let mut link = Link { id, refine: 0, grade: 0, cards: [0; 4], equip };
    let mut card = 0;
    let mut fields = &rest[end..];
    while let Some(mark) = fields.chars().next() {
        let value_end = fields[1..].find(|c: char| !c.is_ascii_alphanumeric()).map_or(fields.len(), |i| i + 1);
        let value = base62(&fields[1..value_end]).unwrap_or(0);
        match mark {
            '%' => link.refine = value.min(255) as u8,
            '\'' => link.grade = value.min(255) as u8,
            ')' if card < 4 => {
                link.cards[card] = value;
                card += 1;
            }
            // Look, options: not shown.
            _ => {}
        }
        fields = &fields[value_end..];
    }
    Some(link)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteminfo_output_reads_as_items() {
        // @iteminfo lines from the server, as they arrived in chat.
        let line = "Item: 'Hydra_Card'/'<ITEML>000020135'00)00)00)00)00</ITEML>' (4035) Type: Card";
        let pieces = split(line);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0], Piece::Text("Item: 'Hydra_Card'/'"));
        assert_eq!(pieces[1], Piece::Item(Link { id: 4035, refine: 0, grade: 0, cards: [0; 4], equip: false }));
        assert_eq!(pieces[2], Piece::Text("' (4035) Type: Card"));
        for (tag, id) in [("<ITEML>0000001DF'00)00)00)00)00</ITEML>", 6303), ("<ITEML>0000003e8'00)00)00)00)00</ITEML>", 12408), ("<ITEML>0000004cQD'00)00)00)00)00</ITEML>", 1002703)] {
            assert!(matches!(split(tag)[..], [Piece::Item(Link { id: i, .. })] if i == id), "{tag}");
        }
    }

    #[test]
    fn refine_cards_and_equipment_are_read() {
        // A +7 equip (look 02, grade 1) holding two cards (4035 = "135").
        let pieces = split("<ITEML>000221F9kD%07&02'01)135)135)00)00+0a,00-0f</ITEML>");
        let [Piece::Item(link)] = pieces[..] else { panic!("{pieces:?}") };
        assert_eq!(link.id, base62("F9kD").unwrap());
        assert!(link.equip);
        assert_eq!((link.refine, link.grade, link.cards), (7, 1, [4035, 4035, 0, 0]));
    }

    #[test]
    fn unreadable_tags_stay_as_text() {
        assert_eq!(split("plain"), vec![Piece::Text("plain")]);
        assert_eq!(split("a <ITEML>x</ITEML> b"), vec![Piece::Text("a <ITEML>x</ITEML>"), Piece::Text(" b")]);
        assert_eq!(split("open <ITEML>0000"), vec![Piece::Text("open <ITEML>0000")]);
    }
}
