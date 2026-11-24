| phase  | skills             |
|--------|--------------------|
| scope  | deep-research      |
| search | deep-research, web |
| notes  | deep-research, web |
| learn  | deep-research, web |
| answer | deep-research      |

## phase: scope
Say what is asked, then write the sub-questions. Search nothing yet.
Your one reply is your answer and it ends your work. Reply only at the end of the answer phase, or to ask what no page can answer (which of two readings the asker means). Never reply to say what you are doing.
1. The questions and log artifacts are below, in Artifacts: read them there. Sub-questions already there with files in the folder (run(command="ls -la . sources 2>/dev/null")): an earlier round did part of the work; start from it.
2. Write the question in one line, and in one line what the answer is for (who acts on it, and how).
3. Write the sub-questions with write(name="questions", text="..."): one `- [ ]` line each, three to eight of them, each one a page could answer. Put first the ones the answer cannot do without.
4. Then goto_phase(name="search").

## phase: search
Find pages and read them into sources/. Write no fact yet.
Load the web skill first, in a step of its own: skill(name="web"). It brings web.search, web.wiki and web.read.
Then several calls in one act, so they run together: web.search(query="..."); web.search(query="..."); web.wiki(title="...")
Read the two or three best results of each search with web.read(url="..."): every page read is a file under sources/. A fact that matters needs two pages that do not copy each other; one source is a lead, not a fact.
After each act write the log with write(name="log", text="..."): one line per search and per page (what it gave, or why it failed), kept whole and added to.
A page that refuses or comes back empty: take the next result, never work around a block. When the open `- [ ]` questions each have pages that speak to them, or two different searches found nothing for one, goto_phase(name="notes").

## phase: notes
Turn what you read into facts, each with its source.
Open the pages (open(path="sources/a.txt|sources/b.txt"), then focus on the part you need, or run(command="grep -n -i 'word' sources/*.txt")). Read them: the search results are not the pages.
Write the facts in a file, so other engines can read them. The first round: edit(path="notes.md", new="its whole text"); save(path="notes.md"). A later round is a new file (notes-2.md, notes-3.md): a file that holds text is not replaced whole.
One fact per line, under a `## ` heading per sub-question, in your own words:
- the fact, said plainly (source: <url>, read <date from the file's `read:` line>)
Two sources that disagree: one line each, marked `disagrees with` the other. Something a page only suggests: start the line with `unconfirmed:`.
Copy nothing longer than a short phrase: no script lines, no lyrics, no long quotes. Summarise what was said and who said it.
Write the questions again with each one the notes now answer as `- [x]`. Then goto_phase(name="learn"): always, whether or not every question is ticked.

## phase: learn
Close the gaps: summarise, question the summary, answer the questions, write the lessons. This phase is never skipped.
1. Summarise. edit(path="summary.md", new="..."); save(path="summary.md"): facts only, grouped under `## ` headings, each line with its source, the disagreements kept and named. No opinion, no adjectives. On a later pass change it with open, then edit(path="summary.md", old="...", new="..."), then save.
2. The question session, once per research: read the summary as a critical reader who has to act on it. Add to the questions artifact, as `- [ ]` lines under `## Asked of the summary`, at least five questions it does not answer, and one for every disagreement between sources. Ask what would change what the reader does: what is missing, what rests on one source, what is dated, what is claimed with no page behind it.
3. Close each one. The notes or a page already in sources/ answer it: add the fact to the summary and mark it `- [x]`. Nothing here answers it: goto_phase(name="search") for it, then come back through notes. A question still open after two different searches and one page read: mark it `- [~]` and say on its line what was tried.
4. When no `- [ ]` line is left, write the lessons: edit(path="lessons.md", new="..."); save(path="lessons.md"), under `## Worked` (the queries and sites that gave facts), `## Failed` (the queries that gave nothing, the sites that refused, with the error) and `## Next time` (what to do first).
5. Then goto_phase(name="answer").

## phase: answer
Answer from the summary, with the sources.
Read summary.md and the questions artifact as they are now. Every fact in the answer is a line of the summary with a source; one that has none goes under Unverified or back to learn (goto_phase(name="learn")).
Run run(command="ls -la . sources && wc -c notes*.md summary.md lessons.md") once, then reply, in a step of its own, with these lines:
Answer: the question's answer in a few sentences
Facts: each one in a line, with its source url
Open: each `- [~]` question and what was tried, or "nothing"
Files: notes.md (and notes-N.md), summary.md, lessons.md, sources/ with the count of pages, as the ls showed them
Unverified: what rests on one source or on a page that only suggests it, or "nothing"
