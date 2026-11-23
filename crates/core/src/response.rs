//! The response: the contract for what a model's answer must contain.
//!
//! A contract is only fields, each a `key` and a `description`, usually a
//! table in a markdown file (`responses.md`):
//!
//! ```text
//! format: toon
//!
//! | key     | description                                   |
//! |---------|-----------------------------------------------|
//! | thought | What you understood and what you will do.     |
//! | act     | The tool calls for this step. ...             |
//! ```
//!
//! There is no field that means "finished": answering the user is a tool
//! call too (`reply(...)` in `act`), so the contract stays the same on
//! every step of the loop.
//!
//! How the answer is *written* is separate: a `ResponseFormat` turns the
//! contract into instructions for the model and reads its answer back.
//! `Json` and `Toon` (one `key: value` line per field, the separator
//! configurable) implement it; `Format` picks one by name, so an engine can
//! switch at run time without touching its contract.

/// One field of the contract.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub key: String,
    pub description: String,
    /// Asked for, but an answer without it still reads (as empty).
    pub optional: bool,
}

/// The contract: its fields, in the order the model writes them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Responses {
    pub fields: Vec<Field>,
}

/// A read answer: each field's value, in the contract's order.
pub type Answer = Vec<(String, String)>;

impl Responses {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn get<'a>(answer: &'a Answer, key: &str) -> Option<&'a str> {
        answer.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Read a contract from markdown: every table row `| key | description |`
    /// (the header and `|---|` rows are skipped). A description ending in
    /// `(optional)` makes the field optional. A `format: <name>` line, if
    /// any, says how answers are written; it is returned too.
    pub fn from_md(text: &str) -> Result<(Responses, Option<Format>), String> {
        let mut fields = Vec::new();
        let mut format = None;
        for line in text.lines().map(str::trim) {
            if let Some(name) = line.strip_prefix("format:") {
                format = Some(Format::parse_name(name.trim()).ok_or_else(|| format!("unknown format \"{}\"", name.trim()))?);
                continue;
            }
            let Some(row) = line.strip_prefix('|').and_then(|l| l.strip_suffix('|')) else { continue };
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            let [key, description, ..] = cells[..] else { continue };
            if key == "key" || key.chars().all(|c| c == '-' || c == ':') {
                continue;
            }
            let (description, optional) = match description.strip_suffix("(optional)") {
                Some(d) => (d.trim().to_string(), true),
                None => (description.to_string(), false),
            };
            fields.push(Field { key: key.to_string(), description, optional });
        }
        if fields.is_empty() {
            return Err("no fields: a response needs a `| key | description |` table".into());
        }
        Ok((Responses { fields }, format))
    }

    /// The contract as markdown, readable by `from_md`.
    pub fn to_md(&self, format: &Format) -> String {
        let mut out = format!("format: {}\n\n| key | description |\n|---|---|\n", format.name());
        for f in &self.fields {
            let opt = if f.optional { " (optional)" } else { "" };
            out.push_str(&format!("| {} | {}{opt} |\n", f.key, f.description));
        }
        out
    }

    /// Fields that must be in every answer but are missing from `answer`.
    fn missing(&self, answer: &Answer) -> Vec<&str> {
        self.fields.iter().filter(|f| !f.optional && Responses::get(answer, &f.key).is_none()).map(|f| f.key.as_str()).collect()
    }

    /// Put `values` in contract order; error if a required field is missing.
    fn ordered(&self, mut values: Vec<(String, String)>) -> Result<Answer, String> {
        let mut answer = Answer::new();
        for f in &self.fields {
            if let Some(i) = values.iter().position(|(k, _)| *k == f.key) {
                answer.push(values.swap_remove(i));
            }
        }
        match self.missing(&answer)[..] {
            [] => Ok(answer),
            ref keys => Err(format!("the answer has no {}", keys.join(", "))),
        }
    }
}

/// How an answer is written. Anything that can tell the model the shape
/// and read the answer back is a format.
pub trait ResponseFormat {
    /// The name in files and on buttons (`json`, `toon`).
    fn name(&self) -> String;
    /// Tell the model how to answer: the shape, then each field.
    fn instructions(&self, r: &Responses) -> String;
    /// Read an answer into fields. Error: what is wrong with it.
    fn parse(&self, r: &Responses, text: &str) -> Result<Answer, String>;
    /// Write an answer in this format (examples, tests).
    fn write(&self, answer: &Answer) -> String;
}

/// `{"key": "value", ...}`: universal, a bit noisy.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Json;

/// One `key: value` line per field, a long value continuing on the next
/// lines. Fewer tokens than JSON and easy to read. The separator between
/// key and value is configurable (`: ` by default).
#[derive(Clone, Debug, PartialEq)]
pub struct Toon {
    pub separator: String,
}

impl Default for Toon {
    fn default() -> Self {
        Toon { separator: ": ".into() }
    }
}

fn field_list(r: &Responses) -> String {
    r.fields
        .iter()
        .map(|f| format!("- {}: {}{}", f.key, f.description, if f.optional { " (may be left out)" } else { "" }))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Drop a ```fence``` around the whole answer, which models like to add.
fn unfence(text: &str) -> &str {
    let t = text.trim();
    let Some(rest) = t.strip_prefix("```") else { return t };
    let rest = rest.split_once('\n').map_or("", |(_, body)| body);
    rest.trim_end().strip_suffix("```").unwrap_or(rest).trim()
}

impl ResponseFormat for Json {
    fn name(&self) -> String {
        "json".into()
    }

    fn instructions(&self, r: &Responses) -> String {
        let example: Answer = r.fields.iter().map(|f| (f.key.clone(), "...".into())).collect();
        format!(
            "Answer with one JSON object and nothing else. Its keys, in this order:\n{}\nLike this:\n{}",
            field_list(r),
            self.write(&example)
        )
    }

    fn parse(&self, r: &Responses, text: &str) -> Result<Answer, String> {
        let text = unfence(text);
        let (start, end) = (text.find('{'), text.rfind('}'));
        let (Some(start), Some(end)) = (start, end) else { return Err("no JSON object in the answer".into()) };
        let value: serde_json::Value = serde_json::from_str(&text[start..=end]).map_err(|e| format!("not JSON: {e}"))?;
        let obj = value.as_object().ok_or("the JSON is not an object")?;
        let values = obj
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().map_or_else(|| v.to_string(), str::to_string)))
            .collect();
        r.ordered(values)
    }

    fn write(&self, answer: &Answer) -> String {
        let map: serde_json::Map<String, serde_json::Value> =
            answer.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))).collect();
        serde_json::Value::Object(map).to_string()
    }
}

impl ResponseFormat for Toon {
    fn name(&self) -> String {
        if self.separator == Toon::default().separator { "toon".into() } else { format!("toon{}", self.separator.trim_end()) }
    }

    fn instructions(&self, r: &Responses) -> String {
        let sep = &self.separator;
        let example: Answer = r.fields.iter().map(|f| (f.key.clone(), "...".into())).collect();
        format!(
            "Answer with one line per key, written `key{sep}value`, in this order, and nothing else. A long value may go on over the next lines. The keys:\n{}\nLike this:\n{}",
            field_list(r),
            self.write(&example)
        )
    }

    fn parse(&self, r: &Responses, text: &str) -> Result<Answer, String> {
        let sep = self.separator.trim();
        let mut values: Vec<(String, String)> = Vec::new();
        for line in unfence(text).lines() {
            if line.trim_start().starts_with("```") {
                continue; // a fence the model put around part of it
            }
            // A line that starts with a contract key and the separator
            // starts that field; any other line continues the current one.
            let opens = r.fields.iter().find_map(|f| {
                let rest = line.trim_start().strip_prefix("- ").unwrap_or(line.trim_start()).strip_prefix(f.key.as_str())?;
                rest.trim_start().strip_prefix(sep).map(|v| (f.key.clone(), v.trim().to_string()))
            });
            match (opens, values.last_mut()) {
                (Some(kv), _) => values.push(kv),
                (None, Some((_, v))) => {
                    v.push('\n');
                    v.push_str(line);
                }
                (None, None) => {} // text before the first key: dropped
            }
        }
        // TOON first; an answer the model wrote as JSON instead still reads.
        let toon = if values.is_empty() {
            Err(format!("no `key{}value` lines in the answer", self.separator))
        } else {
            r.ordered(values.into_iter().map(|(k, v)| (k, v.trim().to_string())).collect())
        };
        toon.or_else(|e| Json.parse(r, text).map_err(|_| e))
    }

    fn write(&self, answer: &Answer) -> String {
        answer.iter().map(|(k, v)| format!("{k}{}{v}", self.separator)).collect::<Vec<_>>().join("\n")
    }
}

/// The formats there are, picked by name: `json`, `toon`, or `toon` with
/// its own separator right after it (`toon=` writes `key= value`).
#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    Json(Json),
    Toon(Toon),
}

impl Default for Format {
    fn default() -> Self {
        Format::Toon(Toon::default())
    }
}

impl Format {
    pub fn parse_name(name: &str) -> Option<Format> {
        match name.trim() {
            "json" => Some(Format::Json(Json)),
            "toon" => Some(Format::Toon(Toon::default())),
            other => {
                let sep = other.strip_prefix("toon")?.trim();
                (!sep.is_empty()).then(|| Format::Toon(Toon { separator: format!("{sep} ") }))
            }
        }
    }

    fn inner(&self) -> &dyn ResponseFormat {
        match self {
            Format::Json(f) => f,
            Format::Toon(f) => f,
        }
    }
}

impl ResponseFormat for Format {
    fn name(&self) -> String {
        self.inner().name()
    }
    fn instructions(&self, r: &Responses) -> String {
        self.inner().instructions(r)
    }
    fn parse(&self, r: &Responses, text: &str) -> Result<Answer, String> {
        self.inner().parse(r, text)
    }
    fn write(&self, answer: &Answer) -> String {
        self.inner().write(answer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> Responses {
        Responses::from_md(include_str!("../../../responses.md")).expect("responses.md reads").0
    }

    fn answer() -> Answer {
        vec![("thought".into(), "say hello".into()), ("act".into(), "reply(text=\"hi, there\")".into())]
    }

    #[test]
    fn the_shipped_contract_reads_and_has_no_exit_field() {
        let (r, format) = Responses::from_md(include_str!("../../../responses.md")).unwrap();
        let keys: Vec<&str> = r.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, ["thought", "act"]);
        assert_eq!(format.map(|f| f.name()), Some("toon".into()));
        assert!(r.fields[1].description.contains("reply("), "answering the user is a call");
        assert_eq!(Responses::from_md(&r.to_md(&Format::default())).unwrap().0, r);
    }

    #[test]
    fn every_format_reads_back_what_it_writes() {
        let r = contract();
        for name in ["json", "toon", "toon="] {
            let f = Format::parse_name(name).unwrap();
            assert_eq!(f.name(), name);
            assert_eq!(f.parse(&r, &f.write(&answer())).unwrap(), answer(), "{name}");
            assert!(f.instructions(&r).contains("- act: "), "{name} lists the fields");
        }
    }

    #[test]
    fn toon_values_go_over_lines_and_fences_and_chatter_are_dropped() {
        let r = contract();
        let text = "Sure!\n```\nthought: first line\nsecond line\n- act: reply(text=\"ok\")\n```";
        let a = Format::default().parse(&r, text).unwrap();
        assert_eq!(Responses::get(&a, "thought"), Some("first line\nsecond line"));
        assert_eq!(Responses::get(&a, "act"), Some("reply(text=\"ok\")"));
    }

    #[test]
    fn json_in_prose_reads_and_a_missing_field_is_named() {
        let r = contract();
        let json = Format::parse_name("json").unwrap();
        let a = json.parse(&r, "Here: {\"act\": \"reply(text=\\\"x\\\")\", \"thought\": \"t\"} done").unwrap();
        assert_eq!(a[0].0, "thought", "contract order, not the answer's");
        assert_eq!(json.parse(&r, "{\"thought\": \"t\"}").unwrap_err(), "the answer has no act");
        assert!(Format::default().parse(&r, "just words").is_err());
    }

    #[test]
    fn toon_is_the_default_and_falls_back_to_json() {
        let r = contract();
        assert_eq!(Format::default().name(), "toon");
        let a = Format::default().parse(&r, "{\"thought\": \"t\", \"act\": \"reply(text=\\\"x\\\")\"}").unwrap();
        assert_eq!(Responses::get(&a, "thought"), Some("t"));
        // Neither TOON nor JSON: the TOON error is the one named.
        assert!(Format::default().parse(&r, "just words").unwrap_err().contains("no `key: value` lines"));
    }
}
