---
name: web
description: Search the web and read pages into files under sources/ with tools made for it - load it before any search.
---
# Web: search, read, keep the source

This skill brings three tools. Each runs in your terminal, in the root,
and comes back with the exit code and the last lines, as `run` does. A
result shows at most 15 lines: the page itself is in the file.

- `web.search(query="jack sparrow speech mannerisms")` prints which
  source answered, then up to 8 results: `n. title — url — snippet`.
  It asks DuckDuckGo, then DuckDuckGo's lite page, then Wikipedia's
  search. When the first line says "wikipedia search", the web search
  gave nothing and the results are Wikipedia articles only: say so in
  your notes.
- `web.wiki(title="Jack Sparrow")` saves one English Wikipedia article as
  plain text in `sources/wiki-<title>.txt` and prints its section names.
  Use it for any Wikipedia page (`web.read` on a Wikipedia address gives
  a noisier text). A title that is not an article prints the close ones.
- `web.read(url="https://...")` fetches one page, removes tags, scripts
  and styles, and saves the text in `sources/<site-and-path>.txt`. Each
  file starts with `url:`, `read:` (the date) and `title:` lines.

How to work:

1. **Several calls in one act.** Three searches, or a search and two
   reads, start together and their results come back in the order
   written:
   `web.search(query="..."); web.search(query="..."); web.wiki(title="...")`
2. **A snippet is not a source.** A fact goes in the notes only from a
   page you read into `sources/`. Read the two or three best results of
   a search, not all eight.
3. **Read the file, not the result.** The result shows the first lines.
   `open(path="sources/<name>.txt")` shows the text; a long one is cut,
   so `focus(path="...", from="40", to="120")` on the part you need, or
   `run(command="grep -n -i 'word' sources/<name>.txt")` to find it.
   Close what you have read by opening the next: at most 8 files are
   open at once.
4. **When a read fails.** `error:` with 403, 429 or a timeout means the
   site refuses programs: take the next result, do not retry it. "very
   little text came back" means the page is drawn by script or sits
   behind a consent wall: the same. Never work around a block.
5. **Search words.** Short and specific beats a sentence. Change the
   words, not the order, when a search gives nothing new: a name with a
   role ("actor interview", "screenwriter", "analysis"), a quoted exact
   phrase, a year.
6. **Which pages to trust** (first is best): the maker's own words
   (interviews, official pages), an encyclopedia or a reference work, a
   named journalist or scholar, then fan wikis and blogs. A forum post
   or an unsigned list is a lead to check, not a source.
7. **Copyright.** The files in `sources/` are your working copies and
   stay there. What you write from them is in your own words: no script
   lines, lyrics or passages longer than a short phrase.
