## Notes (approval, folded in)
1) Every training line timestamped (yoda ts.py); checkpoint seconds measured from the log.
2) r3: rank 16, seed 33, identical eval settings, compared against r2.
3) Early blind batch only to choose r2/r3; final fresh batch scores each checkpoint once; Unverified says I saw r1's answers before (early batch).
4) characters/runs.md: every run and scored checkpoint of all five, each citing <folder>/runs.md:line, values as written, gaps 'not logged', flag values that differ from the brief.
Reply under 4000 chars. Never commit, push or publish.

## Logs
r1: 73.26 s real, max RSS 1197572096; val 4.246@1, 2.772@50, 2.688@100, 2.715@150, 2.902@200; saves 19.68/36.97/55.02/73.16 s. data 287 (258/29), seed 11, rank 8.
r2 = data only vs r1 (+12 fact-first lines, batch-fact-02): 299 (269/30), seed 22. 74.80 s, RSS 1193099264; val 4.135@1, 2.765@50, 2.726@100, 2.755@150, 2.768@200; saves 19.72/37.29/55.99/74.69 s.
r3 = rank 16 vs r2, same data, seed 33. 76.33 s, RSS 1262452736, trainable 10.551M; val 4.135@1, 2.725@50, 2.702@100, 3.012@150, 2.926@200; saves 19.98/38.49/57.46/76.21 s.

## Early batch (choice only): base 1.16, r1-50 2.16, r1-100 2.36, r1-150 2.36, r1 2.60.

## FINAL batch (eval/scores-final.csv, 325 unique of 325, seed 13), decides best:
base 1.16 (6 fails); r1-50 2.16 (6), r1-100 2.40 (7), r1-150 2.32 (8), r1 2.60 (7); r2-50 2.16 (7), r2-100 2.20 (8), r2-150 2.04 (7), r2 2.72 (6); r3-50 1.92 (7), r3-100 2.28 (6), r3-150 2.72 (7), r3 2.80 (6). BEST r3 2.80 at 76.21 s. Bar 4/5 NOT met. f1,f2,f5,f6 fail in every set; n5 drinking stated as fact in most tuned sets.
Next: overlap (copy), memo count, probe r3 (12+ paraphrases), tony-stark/runs.md, characters/runs.md, find -newer.