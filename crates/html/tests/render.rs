//! The renderer, drawn from texts the way the browser bundle has them:
//! the bundle loader reads the repo's files as the disk loader does, and
//! the same page is live in the browser and read-only on the server.

use std::collections::BTreeMap;
use std::path::Path;

use dry_core::inference::{Message, OpenAi, Role};
use dry_core::response::Responses;
use dry_core::state::State;
use dry_html::world::{Access, World, load_files};

/// The repo's app files as texts by path, as `crates/web` bundles them.
fn repo_files() -> BTreeMap<String, String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = BTreeMap::new();
    let mut todo: Vec<String> = ["routes", "components", "themes", "scenes"].iter().map(|s| s.to_string()).collect();
    while let Some(rel) = todo.pop() {
        for e in std::fs::read_dir(root.join(&rel)).unwrap().flatten() {
            let name = format!("{rel}/{}", e.file_name().to_string_lossy());
            if e.path().is_dir() {
                todo.push(name);
            } else {
                out.insert(name, std::fs::read_to_string(e.path()).unwrap());
            }
        }
    }
    for f in ["models.json", "sessions.json", "responses.md"] {
        out.insert(f.to_string(), std::fs::read_to_string(root.join(f)).unwrap());
    }
    out
}

#[test]
fn the_bundle_loader_reads_what_the_disk_loader_reads() {
    let from_texts = load_files(&repo_files());
    let from_disk = dry_tools::load();
    let sorted = |mut v: Vec<(String, dry_core::pages::Page)>| {
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    };
    assert_eq!(sorted(from_texts.pages), sorted(from_disk.pages));
    assert_eq!(from_texts.components, from_disk.components);
    assert_eq!(from_texts.local, from_disk.local);
    assert_eq!(from_texts.errors, from_disk.errors);
}

/// One engine whose last reply asks to pick a colour.
fn asking() -> State {
    let mut state = State::default();
    let i = state.add_engine();
    let e = &mut state.engines[i];
    e.inference = OpenAi { base_url: "http://localhost:1234/v1".into(), model: "m".into(), ..OpenAi::default() };
    e.response = Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
    e.tools = vec!["reply".into()];
    e.messages.push(Message { role: Role::User, text: "pick for me".into() });
    e.messages.push(Message {
        role: Role::Assistant,
        text: "thought: ask\nact: reply(text=\"Which?\", ask=\"choice\", options=\"Red|Blue\")".into(),
    });
    state
}

fn page(access: Access, path: &str) -> String {
    let mut w = World::from_files(&repo_files(), asking(), "", None);
    w.access = access;
    let p = dry_html::page::parts(&w, path, &dry_html::Query::default()).unwrap();
    assert_eq!(p.status, 200, "{path}");
    format!("{}{}", p.nav, p.pane)
}

#[test]
fn the_same_page_is_live_in_the_browser_and_read_only_on_the_server() {
    let live = page(Access::Live, "/desk/0");
    assert!(live.contains("data-composer data-engine=\"0\""), "the composer sends");
    assert!(live.contains("data-act=\"ask\" data-value=\"Red\""), "the ask answers");
    assert!(live.contains("data-engine=\"0\" data-msg=\"1\""), "the ask knows its reply");
    assert!(live.contains("data-act=\"new-thread\"") && live.contains("data-act=\"delete-engine\""));
    assert!(live.contains("in this browser"));

    let ro = page(Access::ReadOnly, "/desk/0");
    assert!(!ro.contains("data-act="), "nothing acts on the server");
    assert!(ro.contains(dry_html::render::READ_ONLY) && ro.contains("read-only"));

    // Every standard route draws in both.
    for path in ["/", "/desk", "/engines", "/settings", "/settings/ui", "/code", "/desk/0/artifacts/none", "/engines/0/artifacts/none"] {
        page(Access::Live, path);
        page(Access::ReadOnly, path);
    }
}

#[test]
fn a_reply_with_a_form_and_a_table_draws_both() {
    let mut state = asking();
    state.engines[0].messages[1].text = "thought: ask\nact: reply(text=\"Book it?\", ask=\"form\", title=\"Booking\", fields=\"Day:date|Guests:number(1-12)|Seat:choice(in,out)|Notes:text\", show=\"table\", data=\"Time|Free;18:00|yes;20:00|no\")".into();
    let draw = |state: &State, access: Access| {
        let mut w = World::from_files(&repo_files(), state.clone(), "", None);
        w.access = access;
        let p = dry_html::page::parts(&w, "/desk/0", &dry_html::Query::default()).unwrap();
        p.pane
    };
    let live = draw(&state, Access::Live);
    assert_eq!(live.matches("class=\"ask-field\"").count(), 4, "{live}");
    assert!(live.contains("data-act=\"ask-pick\" data-value=\"out\""), "a field's option only picks");
    assert!(live.contains("name=\"number\"") && live.contains("name=\"text\"") && live.contains("name=\"day\""));
    assert_eq!(live.matches("data-act=\"ask\"").count(), 1, "one Submit");
    assert!(live.contains("<th>Time</th>") && live.contains("<td>20:00</td>"), "the table shows");
    // The table comes before the form, both under the reply.
    assert!(live.find("show-table").unwrap() < live.find("data-ask=\"form\"").unwrap());
    let ro = draw(&state, Access::ReadOnly);
    assert!(!ro.contains("data-act=") && ro.contains("<td>20:00</td>"), "read-only: the table shows, nothing acts");
    // Answered: read-only, one line per field.
    state.engines[0].messages.push(Message { role: Role::User, text: "Answer (form): submitted\nDay: (blank)\nGuests: 4\nSeat: out\nNotes: (blank)".into() });
    let done = draw(&state, Access::Live);
    assert!(done.contains("ask answered") && done.contains("Guests: 4") && !done.contains("data-act=\"ask-pick\""), "{done}");
}

#[test]
fn the_scene_is_a_setting_drawn_in_the_themes_colours_and_its_own() {
    let draw = |state: State, local: &str, query: &str| {
        let w = World::from_files(&repo_files(), state, local, None);
        let mut q = dry_html::Query::default();
        if !query.is_empty() {
            q.theme = Some(query.to_string());
        }
        dry_html::page::parts(&w, "/", &q).unwrap()
    };
    // Nothing in app.md: the shipped default's cat, in the default theme's blue.
    let p = draw(asking(), "", "");
    assert_eq!(p.scene, "cat");
    let light = p.tokens.split(":root[data-mode=light]{--scene").nth(1).unwrap();
    assert!(light.contains("-f:#0A84FFFF;"), "a blue cat: {}", p.tokens);
    // The cat theme: terracotta.
    let p = draw(asking(), "", "cat");
    assert!(p.tokens.contains("--scene-f:#D9733FFF;"), "{}", p.tokens);
    // app.md's scene, and this machine's local.md over it.
    let mut s = asking();
    s.scene = Some("abyss".into());
    assert_eq!(draw(s.clone(), "", "").scene, "abyss");
    let p = draw(s, "## Scene\n| scene | none |\n| motion | 0 |\n", "");
    assert!(p.scene.is_empty() && p.tokens.contains("--motion:0;") && !p.tokens.contains("--scene-"));
}

/// translator at the top with two children, a post from you, and the
/// child's turn on the post addressed to it.
fn family() -> State {
    let mut state = State::default();
    for (name, parent) in [("translator", None), ("helper", Some("translator")), ("tester", Some("translator"))] {
        let i = state.add_engine();
        let e = &mut state.engines[i];
        e.id = name.into();
        e.name = name.into();
        e.parent = parent.map(String::from);
        e.response = Responses::from_md("format: toon\n\n| key | description |\n|---|---|\n| thought | why |\n| act | calls |\n").unwrap().0;
    }
    state.shared.insert("translator".into(), "- 1795356330 you → translator: make it green\n- 1795356400 translator → helper: find the bug\n- 1795356500 tester: no to\n".into());
    let h = &mut state.engines[1];
    h.messages.push(Message { role: Role::User, text: format!("{}\n[thread 2] translator → helper: find the bug", dry_core::desk::WAKE_HEAD) });
    h.messages.push(Message { role: Role::Assistant, text: "thought: Read it.\nact: read(name=\"test\")".into() });
    h.messages.push(Message { role: Role::Tool, text: "read(name=\"test\") → assert!(x)".into() });
    h.messages.push(Message { role: Role::Assistant, text: "thought: Found.\nact: reply(text=\"A missing trim.\")".into() });
    state
}

#[test]
fn the_desk_draws_the_tree_a_thread_as_a_stream_and_its_composer() {
    let mut w = World::from_files(&repo_files(), family(), "", None);
    w.access = Access::Host;
    let top = dry_html::page::parts(&w, "/desk", &dry_html::Query::default()).unwrap().pane;
    for part in [
        "class=\"desk\"",
        "translator&#39;s thread",
        "make it green",
        "href=\"/desk?thread=translator&amp;to=helper\"",
        "data-thread=\"translator\"",
        "<option value=\"translator\" selected>",
        "data-act=\"new-thread\"",
        "bounced: no to",
    ] {
        assert!(top.contains(part), "/desk lacks {part}");
    }
    assert!(!top.contains("New engine") && !top.contains("Model engine"), "no New engine left");
    // Any node opens its root's thread with the composer to it; `board` is the old word.
    for q in ["to=helper", "thread=translator&to=helper", "board=translator&to=helper"] {
        let kids = dry_html::page::parts(&w, "/desk", &dry_html::Query::parse(q)).unwrap().pane;
        for part in ["translator&#39;s thread", "find the bug", "class=\"step thought\">Read it.", "<code class=\"name\">read</code>", "→ assert!(x)", "A missing trim.", "data-thread=\"translator\"", "<option value=\"helper\" selected>"] {
            assert!(kids.contains(part), "{q}: the translator's thread lacks {part}");
        }
    }
    // The engine page is the same stream, its composer pinned under it.
    let engine = dry_html::page::parts(&w, "/desk/1", &dry_html::Query::default()).unwrap().pane;
    assert!(engine.contains("from the thread") && engine.contains("<code class=\"name\">read</code>") && engine.contains("A missing trim."));
    let (stream, composer) = (engine.find("class=\"glass panel chat session\"").unwrap(), engine.find("composer pinned").unwrap());
    let (side, timeline) = (engine.find("class=\"col-side\"").unwrap(), engine.find("Timeline").unwrap());
    assert!(stream < composer && composer < side && side < timeline, "the box ends the main column; the timeline is beside it");
}
