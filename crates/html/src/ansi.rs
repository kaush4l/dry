//! Terminal output as HTML: the colours and weights a program printed
//! with ANSI escape codes (SGR: `ESC [ … m`) become spans with classes
//! (`a-f1` red, `a-b` bold, …) the stylesheet colours per mode, so a
//! test run reads green and red the way it does in a terminal. Every
//! other escape (cursor moves, titles, OSC links) is dropped, and a line
//! a program rewrote with `\r` (a progress bar) shows its last state.
//!
//! Pure and small, so the server and the browser draw the same output.

use std::fmt::Write;

use crate::render::esc;

/// The look an SGR code leaves on: what the next characters are drawn with.
#[derive(Clone, Default, PartialEq)]
struct Pen {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
    fg: Option<Colour>,
    bg: Option<Colour>,
}

#[derive(Clone, Copy, PartialEq)]
enum Colour {
    /// One of the 16 the stylesheet names (`a-f0` … `a-f15`).
    Named(u8),
    Rgb(u8, u8, u8),
}

impl Pen {
    fn is_plain(&self) -> bool {
        *self == Pen::default()
    }

    /// The span's class and style.
    fn open(&self, out: &mut String) {
        let (fg, bg) = if self.inverse { (self.bg.or(Some(Colour::Named(0))), self.fg.or(Some(Colour::Named(7)))) } else { (self.fg, self.bg) };
        let mut class = String::new();
        let mut style = String::new();
        for (on, name) in [(self.bold, "a-b"), (self.dim, "a-d"), (self.italic, "a-i"), (self.underline, "a-u")] {
            if on {
                class.push(' ');
                class.push_str(name);
            }
        }
        for (c, kind, prop) in [(fg, "f", "color"), (bg, "g", "background")] {
            match c {
                Some(Colour::Named(n)) => {
                    let _ = write!(class, " a-{kind}{n}");
                }
                Some(Colour::Rgb(r, g, b)) => {
                    let _ = write!(style, "{prop}:#{r:02x}{g:02x}{b:02x};");
                }
                None => {}
            }
        }
        out.push_str("<span");
        if !class.is_empty() {
            let _ = write!(out, " class=\"{}\"", class.trim_start());
        }
        if !style.is_empty() {
            let _ = write!(out, " style=\"{style}\"");
        }
        out.push('>');
    }

    /// Apply one SGR sequence's numbers (`1;31`, `38;5;208`).
    fn apply(&mut self, params: &str) {
        let nums: Vec<u16> = if params.is_empty() { vec![0] } else { params.split([';', ':']).map(|p| p.parse().unwrap_or(0)).collect() };
        let mut i = 0;
        while i < nums.len() {
            match nums[i] {
                0 => *self = Pen::default(),
                1 => self.bold = true,
                2 => self.dim = true,
                3 => self.italic = true,
                4 => self.underline = true,
                7 => self.inverse = true,
                22 => {
                    self.bold = false;
                    self.dim = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                27 => self.inverse = false,
                n @ 30..=37 => self.fg = Some(Colour::Named((n - 30) as u8)),
                n @ 90..=97 => self.fg = Some(Colour::Named((n - 90 + 8) as u8)),
                39 => self.fg = None,
                n @ 40..=47 => self.bg = Some(Colour::Named((n - 40) as u8)),
                n @ 100..=107 => self.bg = Some(Colour::Named((n - 100 + 8) as u8)),
                49 => self.bg = None,
                n @ (38 | 48) => {
                    let colour = match nums.get(i + 1) {
                        Some(5) => {
                            let c = nums.get(i + 2).copied().map(|v| xterm(v.min(255) as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = |k: usize| nums.get(i + k).copied().unwrap_or(0).min(255) as u8;
                            let rgb = Colour::Rgb(c(2), c(3), c(4));
                            i += 4;
                            Some(rgb)
                        }
                        _ => None,
                    };
                    if n == 38 {
                        self.fg = colour;
                    } else {
                        self.bg = colour;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// An xterm 256-colour index: the first 16 by name, the cube and the greys as RGB.
fn xterm(n: u8) -> Colour {
    match n {
        0..=15 => Colour::Named(n),
        16..=231 => {
            let n = n - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Colour::Rgb(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        _ => {
            let v = 8 + (n - 232) * 10;
            Colour::Rgb(v, v, v)
        }
    }
}

/// Each line as its last state: what follows the last `\r` that is not
/// the end of the line (a progress bar drawn over itself).
fn settle_returns(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        // Keep the codes that came before the last `\r`, so the colour it
        // was drawn in survives, and only the text after it.
        match line.rfind('\r') {
            Some(at) => {
                let codes: String = sgr_only(&line[..at]);
                out.push_str(&codes);
                out.push_str(&line[at + 1..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Only the SGR sequences of `text`, in order.
fn sgr_only(text: &str) -> String {
    let mut out = String::new();
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && b.get(i + 1) == Some(&b'[') {
            let start = i;
            i += 2;
            while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                i += 1;
            }
            if b.get(i) == Some(&b'm') {
                out.push_str(&text[start..=i]);
            }
        }
        i += 1;
    }
    out
}

/// `text` (a program's output) as HTML: escaped, its SGR colours as spans.
pub fn html(text: &str) -> String {
    let text = settle_returns(text);
    let mut out = String::with_capacity(text.len() + 64);
    let mut pen = Pen::default();
    let mut open = false;
    let mut run = String::new();
    let flush = |out: &mut String, run: &mut String, pen: &Pen, open: &mut bool| {
        if run.is_empty() {
            return;
        }
        if !*open && !pen.is_plain() {
            pen.open(out);
            *open = true;
        }
        out.push_str(&esc(run));
        run.clear();
    };
    let mut chars = text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c != '\u{1b}' {
            // Other control characters (bell, backspace) are not drawn.
            if c == '\n' || c == '\t' || !c.is_control() {
                run.push(c);
            }
            continue;
        }
        match chars.peek().map(|&(_, c)| c) {
            // CSI: `ESC [` parameters, then one final byte.
            Some('[') => {
                chars.next();
                let mut params = String::new();
                let mut fin = None;
                for (_, c) in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        fin = Some(c);
                        break;
                    }
                    params.push(c);
                }
                if fin == Some('m') {
                    let mut next = pen.clone();
                    next.apply(&params);
                    if next != pen {
                        flush(&mut out, &mut run, &pen, &mut open);
                        if open {
                            out.push_str("</span>");
                            open = false;
                        }
                        pen = next;
                    }
                }
            }
            // OSC: `ESC ]` … ended by BEL or `ESC \` (titles, links).
            Some(']') => {
                chars.next();
                while let Some((_, c)) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next_if(|&(_, c)| c == '\\');
                        break;
                    }
                }
            }
            // Any other escape: the one character after it.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    flush(&mut out, &mut run, &pen, &mut open);
    if open {
        out.push_str("</span>");
    }
    out
}

/// `text` with every escape sequence removed: the words alone.
pub fn strip(text: &str) -> String {
    let html = html(text);
    // The HTML has only spans and entities: undo both.
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&amp;", "&")
}

/// Whether `text` has any escape sequence in it.
pub fn has_codes(text: &str) -> bool {
    text.contains('\u{1b}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_and_weights_become_spans() {
        let out = html("\u{1b}[32m✓\u{1b}[0m add \u{1b}[1;31m2 failed\u{1b}[0m");
        assert_eq!(out, "<span class=\"a-f2\">✓</span> add <span class=\"a-b a-f1\">2 failed</span>");
        assert_eq!(html("\u{1b}[2m12:00\u{1b}[22m ok"), "<span class=\"a-d\">12:00</span> ok");
        assert_eq!(html("\u{1b}[94mhi\u{1b}[39m"), "<span class=\"a-f12\">hi</span>");
        assert_eq!(html("\u{1b}[38;5;208mx\u{1b}[0m"), "<span style=\"color:#ff8700;\">x</span>");
        assert_eq!(html("\u{1b}[38;2;1;2;3mx"), "<span style=\"color:#010203;\">x</span>");
    }

    #[test]
    fn text_is_escaped_and_other_codes_are_dropped() {
        assert_eq!(html("<b>&</b>"), "&lt;b&gt;&amp;&lt;/b&gt;");
        assert_eq!(html("a\u{1b}[2Kb\u{1b}]0;title\u{7}c\u{1b}]8;;http://x\u{1b}\\d"), "abcd");
        assert_eq!(html("bell\u{7} back\u{8}"), "bell back");
        assert_eq!(html("\u{1b}[31m"), "", "a code with no text draws nothing");
        assert_eq!(html("x\u{1b}"), "x", "a lone escape at the end");
    }

    #[test]
    fn a_rewritten_line_shows_its_last_state() {
        assert_eq!(strip("10%\r50%\r100% done\nnext\r\n"), "100% done\nnext\n");
        assert_eq!(html("\u{1b}[32m10%\r100%\u{1b}[0m"), "<span class=\"a-f2\">100%</span>");
    }

    #[test]
    fn strip_leaves_the_words() {
        assert_eq!(strip("\u{1b}[1mrunning\u{1b}[0m <6> checks & more"), "running <6> checks & more");
        assert!(has_codes("\u{1b}[0m") && !has_codes("plain"));
    }
}
