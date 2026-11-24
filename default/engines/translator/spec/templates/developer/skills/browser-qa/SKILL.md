---
name: browser-qa
description: Check a running web app through the browser artifact - open the page, use it the way a person would, read its text and console errors, and record what was seen.
---
# Browser QA: use the page like a person

You have this only when the browser artifact is present (a companion with
Chrome, or a host that drives one). When your Artifacts hold no browser
and your tools no `browse`, this skill does not apply: check the file with
a command instead and report the page as not opened. Its snapshot is the page as text
(roles, names, values), its console errors and the last screenshot's path.

1. **Open it.** `browse(url="http://127.0.0.1:PORT/the/route")`. Only this
   machine's pages.
2. **Read before you act.** Find the element the change is about in the
   page text. Note the console errors that were there before you did
   anything.
3. **Do the user's steps.** `click(target="Save")`, `type(target="Name",
   text="Ada", key="Enter")`, one step at a time, reading the page after
   each one.
4. **Check the change and its neighbours.** The new behaviour shows. The
   page around it still works: no new console error, no empty list, no
   broken layout text.
5. **Record what you saw** in exact words from the page ("the heading reads
   'Hello, Ada!'") and the screenshot's path, for the report's Saw line.

A page that will not load is a test failure. Put the URL and the error in
the notes.

Ideas from gstack's `qa` and `browse` (MIT); written for dry.
