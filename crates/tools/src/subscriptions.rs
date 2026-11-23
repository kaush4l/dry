//! The subscriptions' probes on this machine (docs/elements.md,
//! "subscriptions"): `models.json` and `sessions.json` read, and each row
//! checked. The rows and their text are `dry_core::subscriptions`.
//!
//! - A CLI or a session program: on PATH (`crate::find_program`, the same
//!   look-up as `command -v`); nothing is started.
//! - A model on a local server (`127.0.0.1`, `localhost`): `GET <base>/models`,
//!   at most `LOCAL_TIMEOUT`, with no key sent.
//! - A remote API model: whether its key variable is set. Only whether:
//!   the value is never read into anything.
//!
//! `Watch` keeps the last result and says when to probe again: every
//! `EVERY`, and at once when either file changes. Shells probe off their
//! UI thread (the local check can take up to a second).

use std::time::{Duration, Instant, SystemTime};

use dry_core::subscriptions::{Check, Presence, Subscriptions};

/// How long a local server has to answer.
pub const LOCAL_TIMEOUT: Duration = Duration::from_millis(800);
/// How often the rows are probed again.
pub const EVERY: Duration = Duration::from_secs(60);

/// One row's presence on this machine.
pub fn probe_one(c: &Check) -> Presence {
    match c {
        Check::Program(p) => match crate::find_program(p) {
            Some(_) => Presence::Present,
            None => Presence::Absent(format!("{p} is not on PATH")),
        },
        Check::Local(base) => {
            let url = format!("{}/models", base.trim_end_matches('/'));
            let answer = ureq::get(&url).config().http_status_as_error(false).timeout_global(Some(LOCAL_TIMEOUT)).build().call();
            match answer {
                Ok(r) if r.status().is_success() => Presence::Present,
                Ok(r) => Presence::Absent(format!("{url} answered {}", r.status())),
                Err(_) => Presence::Absent(format!("{} does not answer", host_of(base))),
            }
        }
        Check::Key(var) => {
            if std::env::var_os(var).is_some_and(|v| !v.is_empty()) {
                Presence::Present
            } else {
                Presence::Absent(format!("{var} not set"))
            }
        }
        Check::Nothing => Presence::Present,
        Check::Broken(why) => Presence::Absent(why.clone()),
    }
}

fn host_of(base: &str) -> &str {
    base.split_once("://").map_or(base, |(_, r)| r.split('/').next().unwrap_or(r))
}

/// Every row of this machine's `models.json` and `sessions.json`, probed.
pub fn probe() -> Subscriptions {
    dry_core::subscriptions::build(crate::settings::read_models(), crate::read_sessions(), &mut probe_one)
}

/// The spans of the usage window (`USAGE_DAYS`), from every engine's log.
pub fn recent<'a>(logs: impl IntoIterator<Item = &'a Vec<dry_core::spans::Span>>) -> Vec<dry_core::spans::Span> {
    let range = dry_core::analytics::Range::days(crate::spans::now_ms(), crate::live::clock().offset, dry_core::subscriptions::USAGE_DAYS);
    logs.into_iter().flat_map(|v| v.iter().filter(|s| range.has(s)).cloned()).collect()
}

/// When `models.json` and `sessions.json` last changed.
fn stamp() -> (Option<SystemTime>, Option<SystemTime>) {
    let at = |p: std::path::PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    (at(crate::root().join("models.json")), at(crate::sessions_file()))
}

/// When to probe again.
#[derive(Debug, Default)]
pub struct Watch {
    at: Option<Instant>,
    stamp: (Option<SystemTime>, Option<SystemTime>),
}

impl Watch {
    /// Is a probe due: never probed, `EVERY` passed, or a file changed?
    /// Marks it taken when it is.
    pub fn due(&mut self) -> bool {
        let now = stamp();
        let due = self.at.is_none_or(|t| t.elapsed() >= EVERY) || now != self.stamp;
        if due {
            self.at = Some(Instant::now());
            self.stamp = now;
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_only_ever_set_or_not_and_its_value_never_reaches_the_prompt() {
        let fake = "sk-dry-FAKE-0123456789abcdef";
        // SAFETY: a variable only this test reads.
        unsafe { std::env::set_var("DRY_TEST_SUBSCRIPTION_KEY", fake) };
        let models = dry_core::models::Models::from_json(
            r#"{ "default": "paid",
                 "providers": { "acme": { "base_url": "https://api.acme.example/v1", "key_env": "DRY_TEST_SUBSCRIPTION_KEY" } },
                 "models": [ { "name": "paid", "id": "acme/r/big" },
                             { "name": "unpaid", "id": "acme/r/big", "key_env": "DRY_TEST_SUBSCRIPTION_KEY_NOT_SET" },
                             { "name": "nowhere", "kind": "cli", "provider": "claude", "model": "haiku", "program": "/no/such/claude" } ] }"#,
        );
        let s = dry_core::subscriptions::build(models, Err("none here".into()), &mut probe_one);
        let text = s.text();
        assert!(!text.contains(fake), "{text}");
        assert!(!format!("{s:?}").contains(fake), "not even held");
        let words: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        assert_eq!(
            words[1..4],
            [
                "paid api big present",
                "unpaid api big absent DRY_TEST_SUBSCRIPTION_KEY_NOT_SET not set",
                "nowhere cli claude haiku absent /no/such/claude is not on PATH"
            ],
            "{text}"
        );
        // And in the head's prompt.
        let mut state = dry_core::shipped::state().unwrap();
        let mut reg = dry_core::capability::Registry::default();
        reg.subscriptions = Some(s);
        state.equip(reg);
        let prompt = dry_core::prompt::render(&state.engines[0]).to_text();
        assert!(prompt.contains("subscriptions (default: paid)"), "{prompt}");
        assert!(!prompt.contains(fake));
    }

    #[test]
    fn a_local_server_that_does_not_answer_is_absent_quickly() {
        // A port nothing listens on (bound, then let go).
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let start = Instant::now();
        let p = probe_one(&Check::Local(format!("http://127.0.0.1:{port}/v1")));
        assert_eq!(p, Presence::Absent(format!("127.0.0.1:{port} does not answer")));
        assert!(start.elapsed() <= Duration::from_secs(1), "{:?}", start.elapsed());
    }
}
