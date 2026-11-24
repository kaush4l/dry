Asked: "Two small things, both under the folder /Volumes/Backup/dry-build/night-run/smoke3/proj. Nobody will answer questions: decide yourself and say what you decided. (1) From the web: who created the character Sherlock Holmes, the title and year of the first story he appears in, and three traits of how he speaks. A short research with its sources, in the folder …/proj/holmes. (2) A Python script tree.py in the folder …/proj/tree that prints a centred tree of height n, each level 2 stars wider. The two do not need each other. When both are done, summarise."
Done meant: a sourced research in proj/holmes (notes.md, summary.md, a url for every fact) and a tree.py in proj/tree whose runs show the expected output.

Direction:
- two children started at the same time — because the person said the two do not need each other — instead of one after the other
- tree readings decided by me — because nobody would answer questions — n is the first command-line argument with a default of 5; level k has 2k-1 stars; lines are centred with leading spaces only; there is no trunk; n must be an integer >= 1, and bad input prints usage on stderr and exits 1
- no reviewer for tree.py — because it is a small new script (one file) — instead of a review pass
- research-holmes sent back once — because its first reply, as I could read it, was cut off partway through trait 1, with no file listing and no urls visible for traits 2 and 3 — instead of accepting it unseen; it restated them in a second reply

Work:
research-holmes (deep-research, model not set, so my own): a short sourced answer on Holmes's creator, his first story with its year, and three speech traits
  Result: done
  Answer: created by Arthur Conan Doyle; first appears in the novel A Study in Scarlet, published in 1887 in Beeton's Christmas Annual (Ward, Lock & Co.)
  Traits:
    (1) he explains his reasoning aloud, step by step, as a lesson — shmoop.com/study-guides/sherlock-holmes/writing-style.html; en.wikipedia.org/wiki/Sherlock_Holmes; arthur-conan-doyle.com/wiki/Sherlock_Holmes
    (2) he states his method as maxims and calls his conclusions "elementary" — en.wikipedia.org/wiki/Sherlock_Holmes; arthur-conan-doyle.com/wiki/Sherlock_Holmes
    (3) he uses dry irony toward Watson and addresses him as "my dear Watson" — arthur-conan-doyle.com/wiki/How_Watson_Learned_the_Trick; arthur-conan-doyle.com/wiki/Sherlock_Holmes; en.wikipedia.org/wiki/Sherlock_Holmes
  Numbers: 10 pages saved in sources/; notes.md 6597 bytes, summary.md 5080 bytes, lessons.md 1496 bytes (ls -la . sources && wc -c, exit 0); 2 replies
  Files: notes.md, summary.md, lessons.md, sources/ (10 .txt)

dev-tree (developer, model not set, so my own): tree.py prints a centred tree of height n, each level 2 stars wider
  Result: done
  Numbers (developer's runs, from cat -e):
    python3 tree.py 3: exit 0, `  *$` / ` ***$` / `*****$`
    python3 tree.py 1: exit 0, `*$`
    python3 tree.py: exit 0, 5 lines, the last is `*********$`
    0, -2, abc, `3 1 2`: each exits 1, 0 bytes on stdout, `usage: python3 tree.py [n]  (n integer >= 1)` on stderr
  Files: tree.py (new)

Learned:
- a child's long reply can reach me cut off; asking for a short restatement under 1500 characters got the missing parts in one round (research-holmes, reply 2)
- the developer also rejects extra arguments as bad input; I had not specified that case (dev-tree reply)

Not done or not verified:
- day of first publication: 21 Nov 1887 (Doyle Encyclopedia) or 1 Dec 1887 (EBSCO); the year 1887 is the same in both
- that Gillette's 1899 play originated "Elementary, my dear Watson" rests on Wikipedia alone, which calls it alleged
- trait 1 is described most directly by a study guide (Shmoop)
- tree.py at height 7 was run, but its output was cut off and not checked; there is no test file (dev-tree)
- I hold no files: the file contents above are as the children reported them, not reread by me
- nothing was committed, pushed or published

Files:
/Volumes/Backup/dry-build/night-run/smoke3/proj/holmes: notes.md, summary.md, lessons.md, sources/ (10 pages)
/Volumes/Backup/dry-build/night-run/smoke3/proj/tree: tree.py