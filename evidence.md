# Evidence: dry's web build

What dry shows in a browser, shot the same way every build and compared
with the committed `evidence/` folder (docs/evidence.md). Run it with
`cargo run -q -p dry-tools -- evidence`; `--update` takes the shots as the
new baseline. The build goes to a scratch folder (`{out}`, default
`target/evidence/`), never `dist/`, and is served on a free port.

| scenario  | route              | viewports | steps |
|-----------|--------------------|-----------|-------|
| landing   | /                  |           |       |
| home      | /app/#/            |           | click "#welcome [data-act=load-demo]"; wait-for "Trip planner"; goto "/app/#/" |
| engine    | /app/#/            |           | click "#welcome [data-act=load-demo]"; wait-for "Trip costs"; goto "/app/#/desk/4"; wait-for "Here is what it costs" |
| settings  | /app/#/settings    |           |       |
| container | /app/#/container   |           |       |

## build
command: cd crates/web && trunk build --release --public-url /app/ --dist {out}/site/app --filehash false index.html && cp landing/index.html {out}/site/index.html

## run
command: exec python3 -m http.server {port} --bind 127.0.0.1 --directory {out}/site
ready: http://127.0.0.1:{port}/app/
url: http://127.0.0.1:{port}
timeout: 30

## viewports
1280x800
390x844
scale: 1

## capture
time: 2026-11-23T12:00:00Z
seed: 42
timezone: UTC
locale: en-US
scheme: dark
settle: 300

## masks

## threshold
pixel: 8
percent: 0
