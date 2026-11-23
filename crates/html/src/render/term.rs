//! A terminal session as a terminal: its state at a glance (a spinner
//! while it runs, a tick or a cross with its exit code when it ended), the
//! command after a prompt, Copy, and its output in colour (`crate::ansi`)
//! in a box that opens at the newest line and stays there as lines come
//! in (`term-scroll`: a reversed flex column, so the browser anchors it at
//! the bottom with no script; the server's script keeps a reader who
//! scrolled up where they were).
//!
//! Shared by the terminal artifact's page (`engine::terminal`) and the
//! live view's panel (`live::panel`), so both read the same.

use std::fmt::Write;

use dry_core::inspect::SessionView;

use super::esc;

/// The session's state: its CSS class, the short word and the glyph.
pub fn state(s: &SessionView) -> (&'static str, String, &'static str) {
    match (s.running, s.exit) {
        (true, _) => ("run", "running".to_string(), ""),
        (false, Some(0)) => ("ok", "exit 0".to_string(), "✓"),
        (false, Some(code)) => ("bad", format!("exit {code}"), "✗"),
        (false, None) => ("stop", "stopped".to_string(), "■"),
    }
}

/// The state's mark: a spinner while running (still under reduced motion), else its glyph.
pub fn mark(s: &SessionView) -> String {
    let (class, word, glyph) = state(s);
    if s.running {
        format!("<span class=\"term-mark run\" title=\"{word}\" aria-label=\"{word}\"><i class=\"spin\"></i></span>")
    } else {
        format!("<span class=\"term-mark {class}\" title=\"{}\" aria-label=\"{}\">{glyph}</span>", esc(&word), esc(&word))
    }
}

/// The state as a badge: `running`, `exit 0`, `exit 1`, `stopped`.
pub fn badge(s: &SessionView) -> String {
    let (class, word, _) = state(s);
    format!("<span class=\"term-badge {class}\">{}</span>", esc(&word))
}

/// A button that copies the nearest terminal's output (no host needed:
/// it works on every shell, so it is a plain handler, not an intent).
pub fn copy_button() -> &'static str {
    "<button class=\"btn ghost term-copy\" type=\"button\" title=\"Copy the output\" onclick=\"var b=this,t=b.closest('.term'),p=t&amp;&amp;t.querySelector('.term-out');if(p&amp;&amp;navigator.clipboard){navigator.clipboard.writeText(p.innerText).then(function(){b.textContent='Copied';setTimeout(function(){b.textContent='Copy'},1200)})}\">Copy</button>"
}

/// The output box: coloured, opening at the newest line.
pub fn output(out: &mut String, tail: &str, empty: &str) {
    if tail.trim().is_empty() {
        let _ = write!(out, "<div class=\"term-scroll\" data-stick><pre class=\"term-out empty\">{}</pre></div>", esc(empty));
        return;
    }
    let _ = write!(out, "<div class=\"term-scroll\" data-stick><pre class=\"term-out\">{}</pre></div>", crate::ansi::html(tail.trim_end_matches('\n')));
}

/// A command as a prompt line: `$ cargo test -q` (its first line; more is "…").
pub fn prompt(command: &str) -> String {
    let mut lines = command.lines();
    let first = lines.next().unwrap_or("");
    let more = if lines.next().is_some() { " …" } else { "" };
    format!("<code class=\"term-cmd trunc\" title=\"{}\"><span class=\"term-ps\">$</span> {}{more}</code>", esc(command), esc(first))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(running: bool, exit: Option<i32>, tail: &str) -> SessionView {
        SessionView {
            id: 1,
            mode: "run".into(),
            command: "cargo test\n--quiet".into(),
            running,
            status: String::new(),
            exit,
            age: "1m ago".into(),
            tail: tail.into(),
        }
    }

    #[test]
    fn the_state_reads_at_a_glance() {
        assert!(mark(&session(true, None, "")).contains("class=\"spin\""));
        assert!(badge(&session(false, Some(0), "")).contains("term-badge ok\">exit 0"));
        assert!(badge(&session(false, Some(101), "")).contains("term-badge bad\">exit 101"));
        assert!(badge(&session(false, None, "")).contains("stopped"));
        assert!(prompt("cargo test\n--quiet").contains("$</span> cargo test …"));
        let mut out = String::new();
        output(&mut out, "\u{1b}[32mok\u{1b}[0m\n", "nothing yet");
        assert!(out.contains("<span class=\"a-f2\">ok</span></pre>") && out.contains("data-stick"));
        let mut empty = String::new();
        output(&mut empty, "  ", "nothing yet");
        assert!(empty.contains("term-out empty\">nothing yet"));
    }
}
