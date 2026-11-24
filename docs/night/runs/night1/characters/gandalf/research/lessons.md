# Lessons

## Worked
- Direct reads of tolkiengateway.net/wiki/<Title> pages: Gandalf (85k, the main source), Letter_155, Letter_156, Letter_181, Letter_246, Narya, Pipe-weed, Shadowfax, Saruman, Denethor_II, Ian_McKellen.
- glyphweb.com/arda (Encyclopedia of Arda): g/gandalf.html, w/wizards.html, s/saruman.html. These served as the independent second source.
- grep and sed on the saved 85k Gateway page with keywords (firework, bilbo, forgiv, odin, portray) found facts faster than reading it whole.

## Failed
- web.wiki (Gandalf, Wizards (Middle-earth)): Wikipedia HTTP 429 every time.
- web.search, all queries: at first DuckDuckGo and DDG lite gave no results and Wikipedia search returned 429; later DuckDuckGo returned 403.
- britannica.com/topic/Gandalf: 403. lotr.fandom.com/wiki/Gandalf: 403.
- mckellen.com/cinema/lotr/qa.htm and gandalf.htm: 404. journal.htm and index.htm: index text only.
- tolkiengateway.net/wiki/Fireworks: 404. The Olórin and Gandalf_the_White pages redirect to the Gandalf page, so they are not independent.
- Off-topic for this question: Letters 107, 131, 144, 202.
- A grep with \| inside double-quoted run commands came back empty. Use grep -E with a plain | instead.

## Next time
- Try one web.search first. If it is blocked, go straight to known Tolkien Gateway and Encyclopedia of Arda addresses rather than retrying.
- For voice and diction, look for scholarly or essay pages at known addresses (e.g. tolkienestate.com, the Tolkien Society, journal archives) and McKellen interviews on newspaper sites, as soon as search works again.
- Run greps one command per call so the output is not cut off.
