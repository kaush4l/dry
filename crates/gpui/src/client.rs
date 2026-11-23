//! The window as a client of a host (docs/web.md, "The send path"). While
//! a `dry-server` hosts the store (its lock beside the store answers,
//! `dry_tools::host`), the server is the one writer and runs the turns:
//!
//! - the window watches the store's generation (the lock) and reads the
//!   store again when it moves, and asks the host what each engine is
//!   doing (`/api/status`) for the tints;
//! - Send and an ask's answer go to the host (`dry_tools::host::send`,
//!   `answer`); a refusal (a turn running, a stale view) shows under the
//!   composer;
//! - the window runs no turns and no callbacks of its own, and Save
//!   refuses, so it never writes over the host's state.
//!
//! With no host it works as it always did. What is not done yet: edits
//! the window makes besides sending (a new engine, the format, Delete,
//! settings) still change only its own copy, which the next reload
//! replaces; the host's measured prompt and compression lines are not
//! shown here.

use gpui::{Context, Window};

use dry_tools::host::{self, Lock, Sent, Watch};

use crate::AppView;

impl AppView {
    /// Is a host running the turns?
    pub(crate) fn hosted(&self) -> bool {
        self.host.is_some()
    }

    /// The tick's look at the lock: a host appeared, wrote, or went.
    pub(crate) fn watch_host(&mut self, found: Option<Lock>, alive: bool, cx: &mut Context<Self>) {
        match host::watch(found.as_ref(), alive, self.host_gen) {
            Watch::Alone => {
                if self.host.take().is_some() {
                    self.host_gen = None;
                    self.composer_note = Some("The host stopped: this window runs the turns again.".into());
                    cx.notify();
                }
            }
            Watch::Same => {}
            Watch::Reload(generation) => {
                let first = self.host.is_none();
                if first && self.unsaved() {
                    // Never drop what was not saved without saying so.
                    self.composer_note =
                        Some("A server hosts the store now: your unsaved changes here are not saved, and are replaced by the host's.".into());
                }
                let loaded = crate::store::load();
                if loaded.error.is_none() {
                    if loaded.state.engines.len() != self.state.engines.len() {
                        self.forget_engine_states();
                    }
                    self.saved = loaded.state.clone();
                    self.state = loaded.state;
                }
                self.host = found;
                self.host_gen = Some(generation);
                cx.notify();
            }
        }
    }

    /// What the host says each engine is doing: their tints here.
    pub(crate) fn host_activity(&mut self, engines: Vec<host::Engine>, cx: &mut Context<Self>) {
        let mut changed = false;
        for e in engines {
            let Some(i) = self.state.index_of(&e.id) else { continue };
            let now = e.activity;
            let before = self.engine_activity.get(&i).cloned();
            if now.is_turn() || matches!(now, dry_core::activity::Activity::Waiting(_)) {
                if before.as_ref() != Some(&now) {
                    self.engine_activity.insert(i, now);
                    changed = true;
                }
            } else if before.is_some() {
                self.engine_activity.remove(&i);
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// Send `text` to engine `engine` (`None`: a new one) through the host.
    pub(crate) fn send_to_host(&mut self, engine: Option<usize>, text: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(lock) = self.host.clone() else { return };
        let target = engine.and_then(|i| self.state.engines.get(i)).map_or_else(|| "new".to_string(), |e| e.id.clone());
        let base = self.host_gen;
        self.composer_note = Some("sending to the host…".into());
        let input = self.input.clone();
        cx.spawn_in(window, async move |this, cx| {
            let sent = cx.background_executor().spawn(async move { host::send(&lock, "gpui", &target, &text, base) }).await;
            this.update_in(cx, |app, window, cx| {
                app.composer_note = match sent {
                    Ok(Sent::Accepted { .. }) => {
                        input.update(cx, |input, cx| input.set_value("", window, cx));
                        None
                    }
                    Ok(Sent::Refused { error, .. }) => Some(format!("The host refused: {error}")),
                    Err(e) => Some(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Answer engine `engine`'s open ask with `text` (what `Ask::answer`
    /// made) through the host.
    pub(crate) fn answer_to_host(&mut self, engine: usize, text: &str, cx: &mut Context<Self>) {
        let Some(lock) = self.host.clone() else { return };
        let Some(e) = self.state.engines.get(engine) else { return };
        let Some(message) = open_ask(e) else {
            self.composer_note = Some("Nothing to answer: the ask was answered already.".into());
            return cx.notify();
        };
        let (id, text, base) = (e.id.clone(), text.to_string(), self.host_gen);
        cx.spawn(async move |this, cx| {
            let sent = cx.background_executor().spawn(async move { host::answer(&lock, "gpui", &id, message, &text, base) }).await;
            this.update(cx, |app, cx| {
                app.composer_note = match sent {
                    Ok(Sent::Accepted { .. }) => None,
                    Ok(Sent::Refused { error, .. }) => Some(format!("The host refused: {error}")),
                    Err(e) => Some(e),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

impl AppView {
    /// Reset history (docs/default.md, "Reset"), the confirm step's second
    /// press. With a host: it does it (`POST /api/reset`), and the lock's
    /// new generation reloads this window. Without one: this window moves
    /// the store aside itself (`dry_tools::store::reset`) and reads the
    /// default. Never while a turn runs here.
    pub(crate) fn reset_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.engine_activity.values().any(dry_core::activity::Activity::is_turn) {
            self.reset_note = Some("An engine is in a turn: let it end, then reset. Nothing was moved.".into());
            return cx.notify();
        }
        self.reset_confirm = false;
        if let Some(lock) = self.host.clone() {
            self.reset_note = Some("asking the host to reset…".into());
            cx.notify();
            cx.spawn(async move |this, cx| {
                let done = cx.background_executor().spawn(async move { host::reset(&lock) }).await;
                this.update(cx, |app, cx| {
                    app.reset_note = Some(match done {
                        Ok(Some(b)) => format!("History reset by the host. The old store is in {b}."),
                        Ok(None) => "Nothing to reset: the store is the default already.".into(),
                        Err(e) => format!("The host refused: {e}"),
                    });
                    cx.notify();
                })
                .ok();
            })
            .detach();
            return;
        }
        match dry_tools::store::reset(&dry_tools::store::default_spec()) {
            Ok(plan) => {
                let loaded = crate::store::load();
                self.mcp.clear();
                self.forget_engine_states();
                self.streaming.clear();
                self.traces.clear();
                self.tasks = Default::default();
                self.span_log = crate::spans::Log::default();
                self.saved = loaded.state.clone();
                self.state = loaded.state;
                self.store = (loaded.store, loaded.error);
                self.start_all_mcp(cx);
                self.reset_note = Some(if plan.is_empty() {
                    "Nothing to reset: the store is the default already.".into()
                } else {
                    format!("History reset. The old store is in {}.", plan.backup.display())
                });
                self.navigate("/desk", window, cx);
            }
            Err(e) => self.reset_note = Some(format!("Not reset: {e}")),
        }
        cx.notify();
    }
}

/// The newest reply whose ask is still open, by index.
pub(crate) fn open_ask(e: &dry_core::state::Engine) -> Option<usize> {
    (0..e.messages.len()).rev().find(|&n| {
        e.messages[n].role == dry_core::inference::Role::Assistant
            && dry_core::ask::ask_in(e, &e.messages[n].text).is_some()
            && dry_core::ask::state(e, n) == dry_core::ask::State::Open
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dry_core::state::State;

    #[test]
    fn the_answer_goes_to_the_newest_open_ask() {
        let mut s = State::default();
        let i = s.add_engine();
        let e = &mut s.engines[i];
        e.response = dry_core::response::Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
        e.tools = vec!["reply".into()];
        let asks = "thought: t\nact: reply(text=\"Go?\", ask=\"confirm\")";
        s.add_text(0, "hi");
        s.add_reply(0, asks);
        assert_eq!(open_ask(&s.engines[0]), Some(1));
        s.add_text(0, "Answer (confirm): yes");
        assert_eq!(open_ask(&s.engines[0]), None, "answered");
        s.add_reply(0, "plain words");
        assert_eq!(open_ask(&s.engines[0]), None, "the newest reply asks nothing");
        s.add_reply(0, asks);
        assert_eq!(open_ask(&s.engines[0]), Some(4));
    }
}
