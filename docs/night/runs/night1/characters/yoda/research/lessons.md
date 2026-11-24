# Lessons

## Worked
- web.wiki 'Yoda' (Wikipedia): creation, Lucas/Kasdan intent, performers, film roles. It worked on the first call, before rate limiting began.
- Direct web.read of https://starwars.fandom.com/wiki/Yoda (Wookieepedia): Personality and traits (l.1169), Behind the scenes (l.1351), exile and Rebels lines (l.976-1046), younglings (l.597).
- Direct web.read of https://www.starwars.com/databank/yoda: official biography, fear and Anakin, 800 years of teaching.
- Direct web.read of https://muppet.fandom.com/wiki/Yoda: Henson suggested Oz, the Grover likeness, Kershner's 1980 People quote.
- Direct web.read of https://tvtropes.org/pmwiki/pmwiki.php/Characters/StarWarsYoda: Verbal Tic, Flanderization of the word order, OOC Is Serious Business, Training from Hell, My Greatest Failure. A rich lead, but a second source only.
- grep with trope names and keywords (arrogan, youngling, laugh, hmm) on the saved files closed most questions without new reads.

## Failed
- web.search: every query returned empty from DuckDuckGo and lite, later HTTP 403. The Wikipedia fallback gave irrelevant hits ('Yoda syntax object subject verb linguist analysis', 'Yoda speech pattern grammar'), then HTTP 429. Failed queries: 'Frank Oz interview Yoda voice performance', 'Irvin Kershner Yoda Empire Strikes Back interview character', 'Yoda inverted syntax Frank Oz Kasdan dialogue', '"Yoda" word order linguistics', 'Frank Oz Yoda character interview', 'starwars.com Yoda Frank Oz', 'Yodish word order XSV'.
- web.wiki 'Object–subject–verb word order', 'Frank Oz', 'The Empire Strikes Back': HTTP 429 Too Many Requests.
- web.read https://starwars.fandom.com/wiki/Frank_Oz and https://muppet.fandom.com/wiki/Frank_Oz: HTTP 403 Forbidden.
- The saved text files keep no links, so the cited interview addresses (IGN FilmForce 2000, Tom Kane 'Mastering Yoda') could not be followed.
- The TV Tropes Strange-Syntax Speaker page had almost nothing on Yoda himself.

## Next time
- Make all needed Wikipedia calls first and space them out ('Languages in Star Wars', 'Frank Oz', 'The Empire Strikes Back', 'Object–subject–verb word order') before the 429 limit hits.
- Go straight to direct reads of known pages (Wookieepedia, StarWars.com databank, Muppet Wiki, TV Tropes) when search returns empty twice.
- For linguistics, try known addresses directly: Language Log (languagelog.ldc.upenn.edu) posts on Yoda, or a Wayback Machine copy of the IGN FilmForce Oz interview.
- Grep the saved fan pages by trope or section names early: they answer sounds, younglings and regrets quickly.
