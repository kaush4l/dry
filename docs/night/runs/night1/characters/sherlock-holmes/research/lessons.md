# Lessons: Sherlock Holmes research

## Worked
- web.wiki on character and story titles: Sherlock Holmes, Joseph Bell, Dr. Watson (3rd try), Inspector Lestrade, C. Auguste Dupin, Monsieur Lecoq, Sherlock (TV series), Sherlock Holmes (1939 film series), Sherlock Holmes (1984 TV series), A Scandal in Bohemia, Blue Carbuncle, Abbey Grange, Three Garridebs, Red-Headed League.
- arthur-conan-doyle.com/index.php/<Name>: a canon-only encyclopedia with story codes. Sherlock_Holmes, Dr._Watson, Mycroft_Holmes, Professor_Moriarty, Irene_Adler, Jeremy_Brett and the story pages (which hold the story text) all answered. It is the best independent second source.
- Wikisource chapter pages (Study in Scarlet, first part, second chapter; Sign of the Four, first chapter) gave primary text (with JSON noise at the top).
- grep -n -i with a list of terms, then focus on the hit lines: fast for checking a single fact.

## Failed
- DuckDuckGo and its lite page: no results, later HTTP 403. Every web.search fell back to Wikipedia search, which gave irrelevant hits for 'manner of speaking Watson narration'.
- Wikipedia: HTTP 429 after about six quick calls; it recovered after a pause.
- Gutenberg cache/epub/244, 2097, 1661: HTTP 503. Britannica topic page: 403. sherlockian.net/investigating: 403.
- Wikisource story titles (Adventures/A Scandal in Bohemia, Five Orange Pips): versions pages only. Guessed paths '_(1892)/...' and '_(1894)/Silver_Blaze': 404.
- arthur-conan-doyle.com Inspector_Lestrade and G._Lestrade: 404.
- No critic describing his speech was found by any query.

## Next time
- Start with the arthur-conan-doyle.com character and story pages, plus four or five Wikipedia calls spaced out, to avoid 429.
- Pick the story articles for the moral and emotional scenes (Abbey Grange, Blue Carbuncle, Three Garridebs) at the start, not after the summary.
- For speech criticism, try Google Scholar, JSTOR or the Baker Street Journal directly, or a named book (Klinger's annotated edition, Konnikova).
- Verify every grep hit before it goes into the summary; cut output hid two endings.
