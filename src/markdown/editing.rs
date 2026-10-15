//! Markdown-aware editing helpers: list/quote continuation and task toggling.

#[derive(Clone, Debug, PartialEq)]
pub struct Prefix {
    pub indent: String,
    /// `> ` quote markers (possibly nested).
    pub quote: String,
    pub marker: Marker,
    pub task: Option<bool>,
    /// Char length of the whole prefix.
    pub len: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Marker {
    None,
    Bullet(char),
    Ordered(u64, char),
}

impl Prefix {
    pub fn is_list(&self) -> bool {
        self.marker != Marker::None
    }
}

pub fn parse_prefix(line: &str) -> Option<Prefix> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    let indent: String = chars[..i].iter().collect();
    let qstart = i;
    while i < chars.len() && chars[i] == '>' {
        i += 1;
        if i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
    }
    let quote: String = chars[qstart..i].iter().collect();
    let mut marker = Marker::None;
    if i + 1 < chars.len() + 1 && i < chars.len() && "-*+".contains(chars[i]) && chars.get(i + 1) == Some(&' ') {
        marker = Marker::Bullet(chars[i]);
        i += 2;
    } else {
        let d = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if d > 0 && d < 10 {
            let delim = chars.get(i + d).copied();
            if matches!(delim, Some('.') | Some(')')) && chars.get(i + d + 1) == Some(&' ') {
                let n: u64 = chars[i..i + d].iter().collect::<String>().parse().unwrap_or(1);
                marker = Marker::Ordered(n, delim.unwrap());
                i += d + 2;
            }
        }
    }
    let mut task = None;
    if marker != Marker::None && chars.len() >= i + 3 && chars[i] == '[' && chars[i + 2] == ']' {
        let state = chars[i + 1];
        if (state == ' ' || state == 'x' || state == 'X')
            && (chars.len() == i + 3 || chars[i + 3] == ' ') {
                task = Some(state != ' ');
                i += if chars.len() > i + 3 { 4 } else { 3 };
            }
    }
    if quote.is_empty() && marker == Marker::None {
        return None;
    }
    Some(Prefix { indent, quote, marker, task, len: i })
}

/// What pressing Enter at the end of `line` should do.
pub enum Continue {
    /// Start the new line with this prefix.
    With(String),
    /// The item was empty: clear the current line's marker (end the list).
    EndList,
}

pub fn continuation(line: &str) -> Option<Continue> {
    let p = parse_prefix(line)?;
    let rest: String = line.chars().skip(p.len).collect();
    if rest.trim().is_empty() {
        return Some(Continue::EndList);
    }
    Some(Continue::With(next_prefix(&p)))
}

pub fn next_prefix(p: &Prefix) -> String {
    let marker = match &p.marker {
        Marker::None => String::new(),
        Marker::Bullet(c) => format!("{c} "),
        Marker::Ordered(n, d) => format!("{}{d} ", n + 1),
    };
    let task = if p.task.is_some() { "[ ] " } else { "" };
    format!("{}{}{}{}", p.indent, p.quote, marker, task)
}

/// Same prefix as `p` (for `O`, which inserts above).
pub fn same_prefix(p: &Prefix) -> String {
    let marker = match &p.marker {
        Marker::None => String::new(),
        Marker::Bullet(c) => format!("{c} "),
        Marker::Ordered(n, d) => format!("{n}{d} "),
    };
    let task = if p.task.is_some() { "[ ] " } else { "" };
    format!("{}{}{}{}", p.indent, p.quote, marker, task)
}

/// Toggle a task checkbox; turns list items / plain lines into tasks.
pub fn toggle_task(line: &str) -> String {
    match parse_prefix(line) {
        Some(p) if p.task.is_some() => {
            let chars: Vec<char> = line.chars().collect();
            // Prefix ends in "[s] " or "[s]"; find the state char.
            let idx = if chars[p.len - 1] == ' ' { p.len - 3 } else { p.len - 2 };
            let mut out = chars.clone();
            out[idx] = if p.task == Some(true) { ' ' } else { 'x' };
            out.into_iter().collect()
        }
        Some(p) if p.is_list() => {
            let chars: Vec<char> = line.chars().collect();
            let head: String = chars[..p.len].iter().collect();
            let tail: String = chars[p.len..].iter().collect();
            format!("{head}[ ] {tail}")
        }
        _ => {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            let rest: String = line.chars().skip(indent.chars().count()).collect();
            format!("{indent}- [ ] {rest}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cont(s: &str) -> Option<String> {
        match continuation(s)? {
            Continue::With(p) => Some(p),
            Continue::EndList => Some("<end>".into()),
        }
    }

    #[test]
    fn continuation_rules() {
        assert_eq!(cont("- foo").as_deref(), Some("- "));
        assert_eq!(cont("  * foo").as_deref(), Some("  * "));
        assert_eq!(cont("9. foo").as_deref(), Some("10. "));
        assert_eq!(cont("- [x] foo").as_deref(), Some("- [ ] "));
        assert_eq!(cont("> quote").as_deref(), Some("> "));
        assert_eq!(cont("> - a").as_deref(), Some("> - "));
        assert_eq!(cont("- ").as_deref(), Some("<end>"));
        assert_eq!(cont("plain"), None);
        assert_eq!(cont("-not a list"), None);
    }

    #[test]
    fn task_toggle() {
        assert_eq!(toggle_task("- [ ] a"), "- [x] a");
        assert_eq!(toggle_task("- [x] a"), "- [ ] a");
        assert_eq!(toggle_task("- a"), "- [ ] a");
        assert_eq!(toggle_task("  text"), "  - [ ] text");
    }
}
