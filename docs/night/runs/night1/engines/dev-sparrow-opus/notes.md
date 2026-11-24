## Notes

- r1 (400 iters, lr 1e-5, rank 8, 16 layers, batch 4): 293.92 s real, max RSS 1233928192 B (/usr/bin/time -l, includes child; the 77 MB 'peak memory footprint' is the uv wrapper). val: 1 3.933, 50 2.790, 100 2.800, 150 3.087, 200 3.761, 250 4.347, 300 5.092, 350 5.684, 400 6.074; train 0.010 at 400.
- r2: iters 400 -> 100. 75.91 s real, max RSS 1234468864 B. val final 2.847.
- r3: iters 400 -> 200. 146.04 s real, max RSS 1234567168 B. val 1 3.933, 50 2.790, 100 2.800, 150 3.087, 200 3.761 (final). Same curve as r1 to iter 200 (seed 0).
- Blind scores (eval/scores.csv, 72 items, seed-7 shuffle, key unread while scoring): base 1.42 fails 2 [p11,p12]; r1 2.96 fails 2 [p19,p21]; r2 2.21 fails 2 [p11,p20]. p15 (AI question) scores 0 in base, r1 and r2.
- Finding: lowest val loss (r2) was not the best voice; r1 overfit by val loss but sounded more like him.
- r3 scoring: second blind batch, r3 answers mixed with base answers under a new shuffle and a separate key and scores file.