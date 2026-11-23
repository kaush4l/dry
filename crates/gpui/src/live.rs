//! The window's side of `file` and `terminal` artifacts: the disk the core
//! reads and runs through (`dry_tools::live::Disk`), and the callback.
//!
//! Every second, and whenever a turn ends, the window asks each idle
//! engine for its callbacks (`dry_core::turn::callbacks`, one path for
//! terminals and children). Its terminals: which sessions ended. A `start` session
//! that ended is a message (`session 2 \`cargo test\` finished, exit 0`),
//! added the way you add one, and the engine runs a turn, as an alert
//! would. An engine in the middle of a turn is left for a later tick, so
//! its turn never races the callback's. A callback is a new turn, never a
//! budget: no count limits it.
//!
//! Child engines (`dry_core::engines`) call back on the same tick: a child
//! that replied either gives its waiting parent's `engine_call` its result
//! (and the parent's turn goes on), or, asked with `engine_start`, wakes
//! the parent with a message and a new turn. A child's own turns run like
//! any engine's (`ask`, then `ask_step`), started when its parent's step
//! made it or gave it a message.

use std::collections::{HashMap, HashSet};

use gpui::Context;

use crate::AppView;
use dry_core::card::Status;
use dry_core::inspect::{ArtifactView, SlotView};
use dry_core::prompt::Rendered;

/// The exact view of an engine (`dry_core::inspect`), as it was at one
/// moment: taken with the disk the turn reads, so it is what is sent.
/// Taken when an engine page opens, when a step's prompt is sent, after
/// each step, and on Refresh; kept until then, so a frame never reads the
/// disk.
pub(crate) struct Inspect {
    /// The engine it is of (indices shift when one is deleted).
    pub engine: String,
    /// Unix seconds.
    pub taken: i64,
    /// When it was taken: `page opened`, `step 2, as sent`, ...
    pub when: String,
    pub artifacts: Vec<ArtifactView>,
    pub slots: Vec<SlotView>,
    pub notes: Vec<(String, String)>,
    /// `prompt ≈ … of … tokens`, against the model's window as known.
    pub size: String,
    /// The same as a share of the window (1.0 is full).
    pub ratio: f64,
    /// The whole prompt as one text, for Copy.
    pub text: String,
}

/// The snapshots taken, by engine index, and which cards are open (by
/// `engine-id/name`; closed by default).
#[derive(Default)]
pub(crate) struct Inspects {
    pub taken: HashMap<usize, Inspect>,
    pub open: HashSet<String>,
    /// The live views' own state (`views::live_view`).
    pub live: crate::views::live_view::LiveState,
}

impl AppView {
    /// The disk for this app's store: logs go into its engine folders.
    pub(crate) fn disk(&self) -> dry_tools::live::Disk {
        dry_tools::live::Disk::for_store(&self.store.0)
    }

    /// How the shell sees engine `j` now, for its parent's snapshot and
    /// tools (`dry_core::engines::Seen`).
    pub(crate) fn seen(&self, j: usize) -> dry_core::engines::Seen {
        let a = self.activity(j);
        let busy = self.engine_activity.get(&j).is_some_and(|a| a.is_turn())
            || matches!(self.engine_status.get(&j), Some(Status::Working(_)))
            || self.state.engines.get(j).is_some_and(|e| !e.waiting.is_empty());
        let failed = match self.engine_status.get(&j) {
            Some(Status::Failed(why) | Status::Missing(why)) => Some(why.clone()),
            _ => None,
        };
        dry_core::engines::Seen { label: a.label(), busy, failed }
    }

    /// Engine `i`'s family now: its level, children, shared contexts
    /// (`dry_core::engines::family`), with this machine's clock offset.
    pub(crate) fn family(&self, i: usize) -> dry_core::engines::Family {
        let mut f = dry_core::engines::family(&self.state, i, &|j| self.seen(j));
        f.offset = dry_tools::live::clock().offset;
        f
    }

    /// Every engine's callbacks, through one path (`dry_core::turn::
    /// callbacks`): children its turn waits for fill their lines and the
    /// turn goes on; `engine_start` children that replied and terminal
    /// `start` sessions that ended wake it with one message and a new
    /// turn. An engine in a turn is left: what came for it waits, and all
    /// of it is delivered together once it is free. Run by the 1 s tick
    /// and whenever a turn ends.
    pub(crate) fn settle_callbacks(&mut self, cx: &mut Context<Self>) {
        let disk = self.disk();
        let mut changed = false;
        for i in 0..self.state.engines.len() {
            let busy = self.engine_activity.get(&i).is_some_and(|a| a.is_turn()) || matches!(self.engine_status.get(&i), Some(Status::Working(_)));
            let seen: Vec<dry_core::engines::Seen> = (0..self.state.engines.len()).map(|j| self.seen(j)).collect();
            let before = self.state.engines[i].artifacts.clone();
            let settled = dry_core::turn::callbacks(&mut self.state, i, busy, &disk, &|j| seen[j].clone());
            if before != self.state.engines[i].artifacts {
                // A session ended: the live views show it now.
                self.take_inspect(i, "after a callback".into(), None);
                changed = true;
            }
            if settled.resumed {
                let n = self.state.engines[i].run.step as usize + 1;
                self.engine_activity.remove(&i);
                self.ask_step(i, n, false, cx);
                changed = true;
            } else if !settled.wake.is_empty() && self.state.add_wake(i, &settled.wake.join("\n")) {
                self.ask(i, cx);
                changed = true;
            }
        }
        changed |= self.refresh_live();
        if changed {
            cx.notify();
        }
    }

    /// The file artifacts the engines hold, `(engine, artifact, label)`,
    /// for the Code page's explorer.
    pub(crate) fn file_artifacts(&self) -> Vec<(usize, String, String)> {
        let mut out = Vec::new();
        for (i, e) in self.state.engines.iter().enumerate() {
            for (name, kind) in dry_core::live::present(e) {
                if kind != dry_core::live::Live::File {
                    continue;
                }
                let open = dry_core::file::State::parse(&e.artifacts[&name]).map_or(0, |s| s.open.len());
                out.push((i, name.clone(), format!("{} · {name} — {open} open", e.name)));
            }
        }
        out
    }

    /// Open engine `engine`'s file artifact `name`'s open files as tabs.
    pub(crate) fn show_file_artifact(&mut self, engine: usize, name: &str, cx: &mut Context<Self>) {
        let Some(text) = self.state.engines.get(engine).and_then(|e| e.artifacts.get(name)) else { return };
        let Ok(state) = dry_core::file::State::parse(text) else { return };
        for path in state.tabs() {
            self.open_file(&path, cx);
        }
    }

    /// Take engine `i`'s exact view now (`when`: why). `rendered`: the
    /// prompt just rendered for sending, so it is shown as it went.
    pub(crate) fn take_inspect(&self, i: usize, when: String, rendered: Option<&Rendered>) {
        let Some(e) = self.state.engines.get(i) else { return };
        let disk = self.disk();
        let family = self.family(i);
        let at = dry_core::live::Snapshots { clock: Some(dry_tools::live::clock()), host: Some(&disk), family: Some(&family) };
        let own;
        let rendered = match rendered {
            Some(r) => r,
            None => {
                own = dry_core::prompt::render_with(e, &at);
                &own
            }
        };
        let key = format!("{} {}", e.inference.base_url, e.inference.model);
        let window = self.windows.get(&key).cloned().unwrap_or_else(|| dry_core::context::Window::of(&e.inference, None));
        let (slots, notes) = dry_core::inspect::slots(e, rendered);
        let budget = dry_core::context::measure(rendered, &window);
        let taken = Inspect {
            engine: e.id.clone(),
            taken: dry_core::live::Host::now(&disk),
            when,
            artifacts: dry_core::inspect::artifacts(e, &at),
            slots,
            notes,
            size: budget.line(),
            ratio: budget.ratio(),
            text: rendered.to_text(),
        };
        let mut x = self.inspects.borrow_mut();
        x.taken.insert(i, taken);
        x.live.forget();
    }

    /// Engine `i`'s exact view, taken now if there is none for it yet.
    pub(crate) fn inspect(&self, i: usize) -> Option<std::cell::Ref<'_, Inspect>> {
        let id = self.state.engines.get(i)?.id.clone();
        if self.inspects.borrow().taken.get(&i).is_none_or(|x| x.engine != id) {
            self.take_inspect(i, "page opened".into(), None);
        }
        std::cell::Ref::filter_map(self.inspects.borrow(), |x| x.taken.get(&i)).ok()
    }
}
