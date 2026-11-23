# dry-server as a service (code-server mode)

The owner's most important mode: the application runs as a service, and
its UI streams straight to a browser on any machine. One server per
hosting. `dry-server` is that service. It is the host (the one writer of
the store, running every engine's turn) and it draws every page
(docs/web.md). A browser elsewhere gets the pages, the live updates over
`/events`, and every control as an intent the host carries out.

## Run it by hand first

```sh
cargo build --release -p dry-server
openssl rand -hex 16 > ~/.config/dry/server-token && chmod 600 ~/.config/dry/server-token
DRY_ROOT=$PWD DRY_SERVER_TOKEN="$(cat ~/.config/dry/server-token)" \
  target/release/dry-server --bind 0.0.0.0 --port 8091
```

On another machine, open `http://<this machine>:8091/?token=<token>` once.
The server sets an HttpOnly cookie and redirects to the plain address.
After that it is the app, live. For a script, send
`Authorization: Bearer <token>`.

| Flag | Environment | What it does |
|---|---|---|
| `--bind ADDR` | `DRY_SERVER_BIND` | Where it listens (default `127.0.0.1`). Anything wider refuses to start without a token. |
| `--port N` | `DRY_SERVER_PORT` | The port (default 8090; 8080 is `dx serve`'s). |
| `--token T` | `DRY_SERVER_TOKEN` | What every client from another machine must carry. |
| `--strict` | `DRY_SERVER_STRICT=1` | Ask the token of this machine too. Use it when a proxy on this machine fronts the server (TLS, a relay), since every request then looks local. The GPUI window and session workers on this machine then need the token as well (not wired yet), so use `--strict` only on a host they do not use. |
| `--read-only` | | Pages only: no turns, no writes. |

`DRY_ROOT` is the folder with `routes/`, `themes/`, `scenes/`,
`models.json` and the store (`state/`). `DRY_STORE` picks another store.

## As a service

The files are in `scripts/service/`. Nothing installs them for you: copy,
edit the paths, and load them yourself.

### macOS: a launchd agent

`scripts/service/dev.dry.server.plist`. It runs as you, starts at login,
and restarts if it dies. It reads the token from
`~/.config/dry/server-token`, so the token never sits in the plist.

```sh
cp scripts/service/dev.dry.server.plist ~/Library/LaunchAgents/
# edit /Users/you/dry (the root and the binary) in the copy
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.dry.server.plist
launchctl print gui/$(id -u)/dev.dry.server | head     # running?
launchctl bootout gui/$(id -u)/dev.dry.server          # stop and unload
```

**The application firewall.** With macOS's firewall on, a freshly built
binary is added as "Block incoming connections" the first time it
listens. That happened on 2026-11-23 with a scratch build: requests from
the LAN address got an empty reply, while loopback worked. Allow it once
in System Settings → Network → Firewall → Options, or sign the binary.
Rebuilding can change its identity, so allow the release binary the
service runs, not `target/debug`. Without that, keep it on loopback and
reach it with `ssh -L 8091:127.0.0.1:8091 host`, which needs no token
and no firewall change.

### Linux: a systemd user service

`scripts/service/dry-server.service`, with the token in
`~/.config/dry/server.env` (`scripts/service/server.env.example`,
`chmod 600`).

```sh
mkdir -p ~/.config/systemd/user ~/.config/dry
cp scripts/service/dry-server.service ~/.config/systemd/user/
cp scripts/service/server.env.example ~/.config/dry/server.env && chmod 600 ~/.config/dry/server.env
systemctl --user daemon-reload && systemctl --user enable --now dry-server
loginctl enable-linger "$USER"        # keep running with nobody logged in
journalctl --user -u dry-server -f
```

A stop takes at most 3 s. The `/events` streams never end, so the server
gives a graceful stop 3 s and then exits. It also removes its lock file
(`<store>.host.json`) and its terminal logs.

How the files were checked: `plutil -lint` passes on the plist.
`systemd-analyze verify` was not available on the Mac it was written on.
The unit was checked only for its sections and known keys.

## Behind TLS

Put a reverse proxy (Caddy, nginx) on the same machine in front of
`127.0.0.1:8091`, and run the server with `--strict --token …`. The proxy
must pass server-sent events through unbuffered. The server sends
`X-Accel-Buffering: no` for nginx.

## What a remote page can do

Everything the host takes as an intent (docs/web.md, "The send path"):

- send messages and answer asks;
- make and delete engines;
- set the format and the mode;
- Detect and Test a model, Make default;
- switch the theme, Reset all tokens;
- hide, show, turn off and delete routes, and create a new page;
- reload an engine's responses or spec;
- approvals and peers;
- type commands into the Code page's terminal.

That terminal runs commands on the host, in the root folder, as the
service's user. That is code-server's terminal, so the token is the key
to the machine. Keep it long, keep it secret, and bind wide only on
networks you trust.

Still off on a remote page: Voice ("not checked in this runtime") and
editing one token's value (the desktop app's for now).

## Proved end to end (2026-11-23)

These checks ran on a scratch root: a copy of the routes, themes and
scenes, a fake OpenAI-compatible model on loopback, and no MCP,
OpenAlice or Telegram. The scripts and shots are kept in the session's
scratch folder.

- **The service.** `dry-server --bind 127.0.0.1 --port 8299 --strict --token …`.
  A bun relay on `0.0.0.0:8297` (bun is allowed through the firewall)
  pipes to it, so the token is still asked of every client.
- **A second machine.** A Linux container (Apple `container`,
  `oven/bun`) with its own address on its virtual network, 192.168.64.x.
  It reached the server through the Mac's LAN address:
  - `401` without the token;
  - `?token=` gave a `303` with an HttpOnly cookie;
  - the page loaded with the cookie (the `host` chip shows);
  - `/assets/scene.wasm` was served;
  - `/events` streamed `hello`, then every `change`, `activity`,
    `message`, `reply` and `turn` of the browser's turn as it ran.

  Then it sent its own message (`202`). The turn ran on the host and
  asked a question. The container answered it (`202`), and a second
  answer was refused (`409`). The engine's page then showed the
  container's message.
- **A browser.** Headless Chrome with a fresh profile, two tabs:
  - without the token, the error;
  - with `?token=` once, the app, live;
  - tab A made an engine and sent a message. The host ran the turn, and
    tab B, never reloaded, showed the mood `working` while it ran, then
    the ask. Tab A answered it; tab B showed the answer and the reply.
  - from Settings, tab A switched the theme to `cat`. Tab B took the new
    tokens and the cat scene, and the cat reacted to tab B's pointer.
  - tab A typed a command into the Code page's terminal. Tab B, on
    `/code`, showed it run and print.
  - tab A hid a route and showed it again, and made another model the
    default. Tab B's navigation followed.
  - no control on Settings is drawn off. On an engine's page, only
    Voice is.
- **The firewall** is the only reason for the relay. Bound to the LAN
  address directly, the unsigned scratch binary was blocked by macOS
  (see above).
