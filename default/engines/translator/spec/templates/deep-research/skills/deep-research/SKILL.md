---
name: deep-research
description: Research a question on the web so that someone else can act on it - sub-questions, pages read into files, facts with sources, then a summary, a question session and lessons that close the gaps. Load it when a step of the method is unclear.
---
# Deep research: facts someone else can act on

You are asked so that another engine, or the person, can work from your
files without searching again. What they can trust is a fact with a page
behind it, and a plain list of what could not be found.

## The files you leave

| file | what it holds |
|---|---|
| `sources/*.txt` | every page read, as text, with its `url:`, `read:` date and `title:` |
| `notes.md` (`notes-2.md`, ...) | each fact, one line, in your words, with its source |
| `summary.md` | the facts grouped and checked: what the reader starts from |
| `lessons.md` | which searches and sites worked, which failed, what to do first next time |

The `questions` artifact holds the sub-questions and their marks; the
`log` artifact holds each search and read in one line. Both are yours
alone: other engines read the files.

## The method

1. **Scope.** The question in one line, what the answer is for, then
   three to eight sub-questions a page could answer. "What is he like"
   is not one; "how do critics and the actor describe his way of
   speaking" is.
2. **Search wide, read narrow.** Several searches in one act, with
   different words. Read the best two or three pages of each, whole
   enough to know what they say. Prefer the maker's own words, a
   reference work, a named writer; treat fan pages and lists as leads.
3. **Two pages for a fact that matters.** Pages that copy each other
   count as one. A fact with one source is written as such.
4. **Notes are facts, not excerpts.** One line each, in your own words,
   with the url and the date read. Keep what disagrees, and say who says
   what. Mark what a page only suggests as `unconfirmed:`.
5. **Nothing copied.** Sources are read, not reproduced: no script
   lines, lyrics or passages longer than a short phrase, in the notes,
   the summary or the answer. Describe how a character speaks; do not
   paste what the character said.
6. **Summarise.** Group the facts under headings that follow the
   sub-questions. Facts only: no praise, no "interestingly", no guess
   written as a fact.
7. **The question session.** Read the summary as the one who must act
   on it and has no other page. Write at least five questions it does
   not answer, and one per disagreement. Good ones:
   - What does the reader need to do the work that is not here?
   - Which line rests on one source, or on a page that is a copy?
   - Which fact is old, and may have changed?
   - Where do two sources disagree, and which is closer to the origin?
   - What would an expert on this say is missing or wrong?
8. **Close each question.** From the notes or a page already read, or by
   a new search made for that question alone. Two different searches and
   one page read without an answer: mark it `- [~]` with what was tried,
   and stop. An honest open question is a result.
9. **Lessons.** The queries that gave facts, word for word; the sites
   that refused, with the error; what you would do first next time. The
   next research on a near topic starts by reading this file.
10. **Answer** from the summary, each fact with its url, and say what is
    open and what is unverified.

## Marks in the questions artifact

```text
## Sub-questions
- [x] who created the character, and from what models
- [ ] how the actor describes the voice
- [~] the exact year of the first draft: searched "<query 1>", "<query 2>", read <url>; the pages give 2001 and 2002

## Asked of the summary
- [ ] the summary says "critics": which critics, where
```

Write the artifact whole each time, keeping every line that was there.

## When it goes wrong

- **A search gives nothing new**: change the words (a role, a year, a
  quoted phrase), not their order. The first line of a result names the
  source that answered; "wikipedia search" means the web search failed.
- **A site refuses** (403, 429, a timeout, almost no text): take the
  next result. Write it in the log and in lessons.md. Never retry in a
  loop and never work around a block.
- **Everything refuses**: say so. Reply with what was read, the errors,
  and the questions as `- [~]`. Do not fill the gap from memory.
- **What you remember** is a lead to check, never a fact: a line in the
  notes has a page under sources/ behind it.

Ideas from Anthropic's research-agent write-up (search wide then
narrow, parallel calls) and the Feynman habit of asking what you cannot
yet explain; written for dry.
