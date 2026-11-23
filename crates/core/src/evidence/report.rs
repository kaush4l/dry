//! A run's verdict: each shot compared with its baseline, the manifest
//! that records it, and the summary a person or a model reads.
//!
//! ```text
//! evidence/manifest.json   build, commit, time, viewports, per shot: sha256, baseline, changed %, status
//! evidence/summary.md      N shots, which changed and by how much, the page-text lines that changed
//! ```
//!
//! The baseline is the evidence folder as committed: its PNG and text per
//! shot, and its manifest's hashes (a shot whose capture hashes the same
//! as the baseline is the same without decoding a pixel). Pure.

use serde::{Deserialize, Serialize};

use super::image::{self, Rect};
use super::spec::Threshold;
use super::text;

/// The most changed text lines kept per shot (the manifest and summary).
pub const TEXT_LINES: usize = 40;

/// Where a shot stands against its baseline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Within the threshold, and the same page text.
    Same,
    /// Changed, and nobody said it would: the run fails.
    Changed,
    /// Changed, and the run was told it would (`--expect`): accepted.
    Expected,
    /// Changed, and the run was told to take it as the new baseline (`--update`).
    Updated,
    /// No baseline yet: this capture becomes it.
    New,
    /// The same input captured twice gave two pictures (`--repeat`): the
    /// page is not deterministic yet; the run fails.
    Flaky,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Same => "same",
            Status::Changed => "changed",
            Status::Expected => "expected",
            Status::Updated => "updated",
            Status::New => "new",
            Status::Flaky => "flaky",
        }
    }

    /// Whether the capture differs from what was there.
    pub fn differs(self) -> bool {
        matches!(self, Status::Changed | Status::Expected | Status::Updated)
    }
}

/// One shot in the manifest.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shot {
    /// `home@1280x800`, the stem of its files.
    pub name: String,
    pub scenario: String,
    pub viewport: String,
    /// The capture's PNG.
    pub sha256: String,
    /// The baseline's PNG; none: there was none.
    #[serde(default)]
    pub baseline: Option<String>,
    /// The capture's page text.
    #[serde(default)]
    pub text_sha256: String,
    /// Changed pixels, percent of all, to two places.
    #[serde(default)]
    pub changed: f64,
    /// The changed regions, `[x, y, w, h]`.
    #[serde(default)]
    pub regions: Vec<[u32; 4]>,
    /// The page text's changed lines (`- `, `+ `), at most `TEXT_LINES`.
    #[serde(default)]
    pub text: Vec<String>,
    /// How many text lines changed in all.
    #[serde(default)]
    pub text_changed: usize,
    pub status: Status,
    /// How long the capture took, milliseconds (steps, settle, shot, text).
    #[serde(default)]
    pub ms: u64,
}

/// The whole run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// What was built: the build output's fingerprint, or `none`.
    pub build: String,
    /// `git rev-parse --short HEAD`, `+dirty` when the tree had changes.
    pub commit: String,
    /// Unix seconds when it ran.
    pub time: i64,
    pub viewports: Vec<String>,
    pub pixel: u8,
    pub percent: f64,
    pub shots: Vec<Shot>,
    /// Baseline shots this run did not take (a scenario was removed).
    #[serde(default)]
    pub removed: Vec<String>,
    /// `pass` or `fail`.
    pub status: String,
    /// The whole run, milliseconds (build and start excluded).
    #[serde(default)]
    pub ms: u64,
}

impl Manifest {
    pub fn from_json(text: &str) -> Result<Manifest, String> {
        serde_json::from_str(text).map_err(|e| format!("manifest.json does not read: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default() + "\n"
    }

    /// The baseline hash a shot had, from this (the previous) manifest.
    pub fn sha_of(&self, name: &str) -> Option<&str> {
        self.shots.iter().find(|s| s.name == name).map(|s| s.sha256.as_str())
    }

    /// The shots that fail the run: unexpected changes, flaky captures.
    pub fn failed(&self) -> Vec<&Shot> {
        self.shots.iter().filter(|s| matches!(s.status, Status::Changed | Status::Flaky)).collect()
    }

    /// One line: `12 shots · 1 changed (home@390x844 2.10%) · fail`.
    pub fn line(&self) -> String {
        let n = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        let mut parts = vec![n(self.shots.len(), "shot", "shots")];
        let changed: Vec<&Shot> = self.shots.iter().filter(|s| s.status == Status::Changed).collect();
        if changed.is_empty() {
            parts.push("0 changed".into());
        } else {
            let names: Vec<String> = changed.iter().take(3).map(|s| format!("{} {:.2}%", s.name, s.changed)).collect();
            let more = if changed.len() > 3 { format!(", +{}", changed.len() - 3) } else { String::new() };
            parts.push(format!("{} changed ({}{more})", changed.len(), names.join(", ")));
        }
        for st in [Status::Flaky, Status::Expected, Status::Updated, Status::New] {
            let k = self.shots.iter().filter(|s| s.status == st).count();
            if k > 0 {
                parts.push(format!("{k} {}", st.word()));
            }
        }
        if !self.removed.is_empty() {
            parts.push(format!("{} removed", self.removed.len()));
        }
        parts.push(self.status.clone());
        parts.join(" · ")
    }

    /// `summary.md`: for a person and a model. Deterministic for the same
    /// captures (no time in it), so an unchanged rerun leaves it as it was.
    pub fn summary(&self) -> String {
        let mut out = String::from("# Evidence\n\n");
        out.push_str(&format!("{}\n\n", self.line()));
        out.push_str(&format!(
            "Commit {} · build {} · tolerance {} per channel, threshold {}%\n\n",
            self.commit, self.build, self.pixel, self.percent
        ));
        out.push_str("| shot | status | changed | regions | text lines |\n|---|---|---|---|---|\n");
        for s in &self.shots {
            let regions = match s.regions.len() {
                0 => String::new(),
                k => format!(
                    "{k}: {}",
                    s.regions.iter().take(3).map(|r| Rect::new(r[0], r[1], r[2], r[3]).text()).collect::<Vec<_>>().join("; ")
                ),
            };
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                s.name,
                s.status.word(),
                if s.status == Status::New { "—".to_string() } else { format!("{:.2}%", s.changed) },
                regions,
                if s.text_changed == 0 { String::new() } else { s.text_changed.to_string() }
            ));
        }
        for name in &self.removed {
            out.push_str(&format!("| {name} | removed | | | |\n"));
        }
        for s in self.shots.iter().filter(|s| s.status.differs()) {
            out.push_str(&format!("\n## {} ({}, {:.2}%)\n\n", s.name, s.status.word(), s.changed));
            out.push_str(&format!(
                "Before `{0}.png`, after `{0}{1}.png`, diff `{0}.diff.png`.\n",
                s.name,
                if s.status == Status::Changed { ".new" } else { "" }
            ));
            if s.text.is_empty() {
                out.push_str("\nThe page text is the same.\n");
            } else {
                out.push_str("\n```diff\n");
                for l in &s.text {
                    out.push_str(l);
                    out.push('\n');
                }
                if s.text_changed > s.text.len() {
                    out.push_str(&format!("… {} more\n", s.text_changed - s.text.len()));
                }
                out.push_str("```\n");
            }
        }
        out
    }
}

/// One shot compared: its manifest row (status `Same`, `Changed` or
/// `New`; the runner turns `Changed` into `Expected` or `Updated`), and
/// the diff image when it changed.
#[derive(Clone, Debug)]
pub struct Compared {
    pub shot: Shot,
    pub diff_png: Option<Vec<u8>>,
}

/// What was captured, and what was there before.
pub struct Pair<'a> {
    pub name: &'a str,
    pub scenario: &'a str,
    pub viewport: &'a str,
    pub before_png: Option<&'a [u8]>,
    pub before_text: Option<&'a str>,
    /// The previous manifest's hash for it.
    pub before_sha: Option<&'a str>,
    pub after_png: &'a [u8],
    pub after_text: &'a str,
}

/// Compare one capture with its baseline.
pub fn compare(p: &Pair, th: Threshold) -> Result<Compared, String> {
    let sha = super::sha::sha256(p.after_png);
    let mut shot = Shot {
        name: p.name.to_string(),
        scenario: p.scenario.to_string(),
        viewport: p.viewport.to_string(),
        sha256: sha.clone(),
        baseline: None,
        text_sha256: super::sha::sha256(p.after_text.as_bytes()),
        changed: 0.0,
        regions: Vec::new(),
        text: Vec::new(),
        text_changed: 0,
        status: Status::New,
        ms: 0,
    };
    let Some(before) = p.before_png else { return Ok(Compared { shot, diff_png: None }) };
    let before_sha = super::sha::sha256(before);
    shot.baseline = Some(before_sha.clone());
    let lines = text::diff(p.before_text.unwrap_or(""), p.after_text);
    shot.text_changed = lines.len();
    shot.text = lines.into_iter().take(TEXT_LINES).collect();
    // The fast path: the same bytes as the baseline (and as its manifest said).
    let same_bytes = before_sha == sha && p.before_sha.is_none_or(|s| s == sha);
    let mut diff_png = None;
    if !same_bytes {
        let d = image::diff(&image::decode(before)?, &image::decode(p.after_png)?, th.pixel);
        shot.changed = d.percent();
        shot.regions = d.boxes.iter().map(|r| [r.x, r.y, r.w, r.h]).collect();
        if d.changed > 0 {
            diff_png = Some(image::encode(&d.image));
        }
        // Over the threshold: more than `percent` of the pixels (any at all
        // when it is 0), counted exactly, not from the rounded percent.
        let raw = if d.total == 0 { 0.0 } else { d.changed as f64 * 100.0 / d.total as f64 };
        let over = d.resized || (d.changed > 0 && raw > th.percent);
        if !over {
            diff_png = None;
            shot.regions.clear();
        }
        shot.status = if over { Status::Changed } else { Status::Same };
    } else {
        shot.status = Status::Same;
    }
    if shot.text_changed > 0 {
        shot.status = Status::Changed;
    }
    Ok(Compared { shot, diff_png })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::image::{Image, encode};

    fn png(fill: [u8; 4]) -> Vec<u8> {
        encode(&Image::new(20, 10, fill))
    }

    fn pair<'a>(before: Option<&'a [u8]>, after: &'a [u8], bt: &'a str, at: &'a str) -> Pair<'a> {
        Pair {
            name: "home@20x10",
            scenario: "home",
            viewport: "20x10",
            before_png: before,
            before_text: Some(bt),
            before_sha: None,
            after_png: after,
            after_text: at,
        }
    }

    #[test]
    fn the_same_capture_is_same_without_a_diff() {
        let a = png([255; 4]);
        let c = compare(&pair(Some(&a), &a, "x", "x"), Threshold::default()).unwrap();
        assert_eq!((c.shot.status, c.shot.changed, c.diff_png.is_none()), (Status::Same, 0.0, true));
        assert_eq!(c.shot.baseline.as_deref(), Some(c.shot.sha256.as_str()));
    }

    #[test]
    fn a_first_capture_is_new() {
        let a = png([255; 4]);
        let c = compare(&Pair { before_png: None, before_text: None, ..pair(None, &a, "", "x") }, Threshold::default()).unwrap();
        assert_eq!((c.shot.status, c.shot.baseline), (Status::New, None));
    }

    #[test]
    fn a_change_over_the_threshold_fails_with_its_diff() {
        let before = Image::new(20, 10, [255; 4]);
        let mut after = before.clone();
        after.fill(Rect::new(0, 0, 4, 5), [0, 0, 0, 255]);
        let (b, a) = (encode(&before), encode(&after));
        let c = compare(&pair(Some(&b), &a, "x", "x"), Threshold { pixel: 8, percent: 0.0 }).unwrap();
        assert_eq!((c.shot.status, c.shot.changed), (Status::Changed, 10.0));
        assert_eq!(c.shot.regions, vec![[0, 0, 8, 8]]);
        assert!(c.diff_png.is_some());
        // Under a 20% threshold the same change passes, with no diff kept.
        let c = compare(&pair(Some(&b), &a, "x", "x"), Threshold { pixel: 8, percent: 20.0 }).unwrap();
        assert_eq!((c.shot.status, c.shot.changed, c.diff_png.is_none(), c.shot.regions.len()), (Status::Same, 10.0, true, 0));
        // Within the tolerance it is no change at all.
        let faint = encode(&Image::new(20, 10, [250, 250, 250, 255]));
        let c = compare(&pair(Some(&b), &faint, "x", "x"), Threshold { pixel: 8, percent: 0.0 }).unwrap();
        assert_eq!(c.shot.status, Status::Same);
    }

    #[test]
    fn a_text_change_alone_is_a_change() {
        let a = png([255; 4]);
        let c = compare(&pair(Some(&a), &a, "button \"Save\"", "button \"Send\""), Threshold::default()).unwrap();
        assert_eq!(c.shot.status, Status::Changed);
        assert_eq!(c.shot.text, ["- button \"Save\"", "+ button \"Send\""]);
    }

    #[test]
    fn the_manifest_reads_back_and_the_summary_says_what_changed() {
        let a = png([255; 4]);
        let mut changed = compare(&pair(Some(&a), &a, "a", "b"), Threshold::default()).unwrap().shot;
        changed.changed = 2.5;
        let mut same = changed.clone();
        same.name = "landing@20x10".into();
        same.status = Status::Same;
        same.text.clear();
        same.text_changed = 0;
        let m = Manifest {
            build: "abc".into(),
            commit: "1234567".into(),
            time: 1,
            viewports: vec!["20x10".into()],
            pixel: 8,
            percent: 0.0,
            shots: vec![same, changed],
            removed: vec!["old@20x10".into()],
            status: "fail".into(),
            ms: 10,
        };
        assert_eq!(Manifest::from_json(&m.to_json()).unwrap(), m);
        assert_eq!(m.line(), "2 shots · 1 changed (home@20x10 2.50%) · 1 removed · fail");
        let s = m.summary();
        assert!(s.contains("| home@20x10 | changed | 2.50% |"), "{s}");
        assert!(s.contains("## home@20x10 (changed, 2.50%)"));
        assert!(s.contains("after `home@20x10.new.png`"));
        assert!(s.contains("```diff\n- a\n+ b\n```"));
        assert!(s.contains("| old@20x10 | removed |"));
        assert!(!s.contains("time"), "no time in the summary: an unchanged rerun leaves it the same");
        assert_eq!(m.sha_of("home@20x10"), Some(m.shots[1].sha256.as_str()));
    }
}
