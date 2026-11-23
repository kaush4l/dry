//! The call grammar: what a model writes in `act` to use a tool.
//!
//! ```text
//! write(name="plan", text="1. read\n2. answer"); reply(text="Done, see plan.")
//! ```
//!
//! A call is `name(key=value, ...)`; calls are separated by `;` (or `,` or
//! new lines). A value is quoted (`"..."` or `'...'`, where `\"`, `\\`,
//! `\n` and `\t` are escapes and `,` `;` `)` are just text) or bare (up to
//! the next `,` or `)`). Empty, `none` or `[]` mean no calls.

use std::fmt;

/// One call: a tool's name and its arguments, in the order written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub name: String,
    pub args: Vec<(String, String)>,
}

impl Call {
    pub fn arg(&self, key: &str) -> Option<&str> {
        self.args.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

impl fmt::Display for Call {
    /// Written back in the grammar, values quoted, long ones cut.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args: Vec<String> = self
            .args
            .iter()
            .map(|(k, v)| {
                let short: String = v.chars().take(60).collect();
                let more = if v.chars().count() > 60 { "…" } else { "" };
                format!("{k}=\"{}{more}\"", short.replace('\n', "\\n"))
            })
            .collect();
        write!(f, "{}({})", self.name, args.join(", "))
    }
}

/// Read the calls in `text`. Error: where it stopped making sense.
pub fn parse(text: &str) -> Result<Vec<Call>, String> {
    let text = text.trim().trim_matches('`').trim();
    let text = text.strip_prefix('[').and_then(|t| t.strip_suffix(']')).unwrap_or(text).trim();
    if text.is_empty() || matches!(text.to_ascii_lowercase().as_str(), "none" | "-" | "null") {
        return Ok(Vec::new());
    }
    let mut chars = text.chars().peekable();
    let mut calls = Vec::new();
    loop {
        while chars.next_if(|c| c.is_whitespace() || matches!(c, ';' | ',')).is_some() {}
        if chars.peek().is_none() {
            return Ok(calls);
        }
        let name: String = std::iter::from_fn(|| chars.next_if(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))).collect();
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        if name.is_empty() || chars.next() != Some('(') {
            return Err(format!("expected a call like `name(key=\"value\")` after {} call(s)", calls.len()));
        }
        let mut args = Vec::new();
        loop {
            while chars.next_if(|c| c.is_whitespace() || *c == ',').is_some() {}
            match chars.peek() {
                None => return Err(format!("`{name}(` is never closed")),
                Some(')') => {
                    chars.next();
                    break;
                }
                _ => {}
            }
            let key: String = std::iter::from_fn(|| chars.next_if(|c| *c != '=' && *c != ')' && *c != ',')).collect();
            let key = key.trim().to_string();
            if chars.next() != Some('=') || key.is_empty() {
                return Err(format!("in `{name}(...)`: an argument is written `key=\"value\"`"));
            }
            while chars.next_if(|c| c.is_whitespace()).is_some() {}
            let value = match chars.peek().copied() {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    let mut v = String::new();
                    loop {
                        match chars.next() {
                            None => return Err(format!("in `{name}(...)`: the value of {key} is never closed with {q}")),
                            Some('\\') => match chars.next() {
                                Some('n') => v.push('\n'),
                                Some('t') => v.push('\t'),
                                Some(c) => v.push(c),
                                None => v.push('\\'),
                            },
                            Some(c) if c == q => break,
                            Some(c) => v.push(c),
                        }
                    }
                    v
                }
                _ => std::iter::from_fn(|| chars.next_if(|c| *c != ',' && *c != ')')).collect::<String>().trim().to_string(),
            };
            args.push((key, value));
        }
        calls.push(Call { name, args });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_with_quoted_and_bare_values() {
        let calls = parse(r#"write(name="plan", text="a, b; c)\n\"d\""); read(name=plan) ; reply(text='hi')"#).unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].arg("text"), Some("a, b; c)\n\"d\""));
        assert_eq!(calls[1].arg("name"), Some("plan"));
        assert_eq!(calls[2].to_string(), "reply(text=\"hi\")");
        assert_eq!(parse("list()").unwrap()[0].args, vec![]);
    }

    #[test]
    fn nothing_and_mistakes() {
        for empty in ["", "none", "[]", "`none`"] {
            assert_eq!(parse(empty).unwrap(), vec![], "{empty}");
        }
        assert!(parse("reply(text=\"open").unwrap_err().contains("never closed"));
        assert!(parse("just words").is_err());
        assert!(parse("reply(\"no key\")").is_err());
    }
}
