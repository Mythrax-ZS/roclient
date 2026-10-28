//! Emote chat commands, as the original client accepts them ("/!", "/thx", ...). The value
//! is the ET_* id the server broadcasts (and the action in sprite\이팩트\emotion).

const COMMANDS: &[(&str, u8)] = &[
    ("/!", 0),
    ("/?", 1),
    ("/ho", 2),
    ("/lv", 3),
    ("/swt", 4),
    ("/ic", 5),
    ("/an", 6),
    ("/ag", 7),
    ("/$", 8),
    ("/...", 9),
    ("/lv2", 14),
    ("/thx", 15),
    ("/wah", 16),
    ("/sry", 17),
    ("/heh", 18),
    ("/swt2", 19),
    ("/hmm", 20),
    ("/no1", 21),
    ("/??", 22),
    ("/omg", 23),
    ("/oh", 24),
    ("/x", 25),
    ("/hlp", 26),
    ("/go", 27),
    ("/sob", 28),
    ("/gg", 29),
    ("/kis", 30),
    ("/kis2", 31),
    ("/pif", 32),
    ("/ok", 33),
    ("/bzz", 36),
    ("/rice", 37),
    ("/awsm", 38),
    ("/meh", 39),
    ("/shy", 40),
    ("/pat", 41),
    ("/mp", 42),
    ("/slur", 43),
    ("/com", 44),
    ("/yawn", 45),
    ("/grat", 46),
    ("/hp", 47),
    ("/fsh", 52),
    ("/spin", 53),
    ("/sigh", 54),
    ("/dum", 55),
    ("/crwd", 56),
    ("/desp", 57),
    ("/love", 65),
    ("/oops", 76),
    ("/spit", 77),
    ("/panic", 79),
    ("/whisp", 80),
];

/// The emote for a chat command, if it is one. "/dice" rolls one of the six dice faces.
pub fn command(text: &str) -> Option<u8> {
    let t = text.trim().to_ascii_lowercase();
    if t == "/dice" {
        return Some(58 + (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.subsec_nanos() % 6) as u8);
    }
    COMMANDS.iter().find(|(c, _)| *c == t).map(|(_, e)| *e)
}

/// Alt+1..Alt+0 send the first ten emotes, as the original's default emote shortcuts.
pub fn shortcut(digit: u8) -> u8 {
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9][((digit + 9) % 10) as usize]
}

/// Splits a whisper command: `/w Name text`, `/w "Name with spaces" text`.
pub fn whisper(text: &str) -> Option<(String, String)> {
    let rest = text.strip_prefix("/w ").or_else(|| text.strip_prefix("/whisper "))?.trim_start();
    let (name, msg) = match rest.strip_prefix('"') {
        Some(q) => q.split_once('"')?,
        None => rest.split_once(' ')?,
    };
    let msg = msg.trim();
    (!name.is_empty() && !msg.is_empty()).then(|| (name.to_string(), msg.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses() {
        assert_eq!(super::command("/thx"), Some(15));
        assert_eq!(super::command("hello"), None);
        assert_eq!(super::whisper("/w Bob hi there"), Some(("Bob".into(), "hi there".into())));
        assert_eq!(super::whisper("/w \"Big Bob\" hi"), Some(("Big Bob".into(), "hi".into())));
        assert_eq!(super::shortcut(1), 0);
        assert_eq!(super::shortcut(0), 9);
    }
}
