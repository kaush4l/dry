//! Spans for the turns this window runs itself (`dry_core::spans`): the
//! window has its own loop (`ask_step`, `run_calls`, `wait_calls`,
//! `finish_calls`, `compress`), so it keeps one `Trace` per engine and
//! calls it at the same points the runner does. A turn's spans go
//! through `Tasks` and are appended to the engine's `spans.jsonl` beside
//! its files. With a host the host runs every turn and is the one writer:
//! nothing is recorded here then.
//!
//! What the views draw (`views/analytics.rs`) is every log as on disk,
//! read again every few seconds (`refresh_spans`) and after each turn.

use std::collections::BTreeMap;

use dry_core::spans::{Daily, Span, Trace, Trigger};
use gpui::Context;

use crate::AppView;

/// Unix milliseconds now.
pub(crate) fn now() -> i64 {
    dry_tools::spans::now_ms()
}

/// Every engine's spans and the rolled-up days, as the views read them.
#[derive(Default)]
pub(crate) struct Log {
    pub spans: BTreeMap<String, Vec<Span>>,
    pub daily: Vec<Daily>,
}

impl Log {
    pub fn all(&self) -> Vec<Span> {
        self.spans.values().flatten().cloned().collect()
    }
}

impl AppView {
    /// Engine `engine`'s trace for the turn running now: made at the
    /// turn's first move (step 1: what the newest message is; a later
    /// step: a callback resumed it).
    pub(crate) fn trace(&mut self, engine: usize, step: usize) -> Option<&mut Trace> {
        if self.hosted() {
            return None;
        }
        let e = self.state.engines.get(engine)?;
        let id = e.id.clone();
        if !self.traces.contains_key(&id) {
            let trigger = if step > 1 { Trigger::Callback } else { dry_core::spans::trigger_of(e) };
            let goal = e.messages.iter().rev().find(|m| m.role == dry_core::inference::Role::User).map_or("", |m| m.text.as_str());
            let mut t = Trace::new(&id, trigger, goal);
            t.moved(now());
            self.traces.insert(id.clone(), t);
        }
        self.traces.get_mut(&id)
    }

    /// Engine `engine`'s turn ended with `outcome`: its spans written.
    pub(crate) fn end_trace(&mut self, engine: usize, outcome: &str, more: dry_core::spans::Attrs) {
        let Some(id) = self.state.engines.get(engine).map(|e| e.id.clone()) else { return };
        let Some(mut t) = self.traces.remove(&id) else { return };
        t.end(now(), outcome, more);
        self.record_spans(t.take());
    }

    /// Spans just closed: under their tasks, appended to their logs.
    pub(crate) fn record_spans(&mut self, mut spans: Vec<Span>) {
        if spans.is_empty() || self.hosted() {
            return;
        }
        for e in &self.state.engines {
            self.tasks.set_parent(&e.id, e.parent.as_deref());
        }
        self.tasks.adopt(&mut spans);
        if let Err(e) = dry_tools::spans::append(&self.disk().engines, &spans) {
            eprintln!("spans not written: {e}");
        }
        for s in spans {
            self.span_log.spans.entry(s.engine.clone()).or_default().push(s);
        }
    }

    /// Read every log again, off this thread (the host's turns, other
    /// windows', and the retention's rollup).
    pub(crate) fn refresh_spans(&mut self, cx: &mut Context<Self>) {
        let dir = self.disk().engines;
        cx.spawn(async move |this, cx| {
            let (spans, daily) = cx.background_executor().spawn(async move { dry_tools::spans::load_all(&dir) }).await;
            this.update(cx, |app, cx| {
                app.span_log = Log { spans, daily };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
