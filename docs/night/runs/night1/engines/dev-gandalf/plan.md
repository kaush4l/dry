## Ask
Gandalf persona: card, original data, LoRA on cached Qwen2.5-1.5B-Instruct-4bit (at most 3 runs, one variable each, new seed each), blind eval against the base, gandalf/runs.md with lessons.

## Approval notes (must hold)
- r2 resumes r1 to 400 iters. Its checkpoints 50-200 are r1's own: score them once, under r1. The final batch holds only r2-250..400 as new r2 entries. runs.md says r2 changed iters only (200 -> 400) against r1, and that seed 22 applies to the new steps only.
- r3 changes exactly one variable (lr) against r1. runs.md says why: the planned data change was dropped (see notes).
- Unverified must say I saw r1's answers before the final batch.
- Write only gandalf/runs.md, not characters/runs.md. Never commit, push or publish. Final reply under 4000 characters.

## Early batch (choice only)
base 1.33 (7 facts fails), r1-50 2.21, r1-100 2.17, r1-150 2.50, r1-200 2.38. Facts fails on p04, p05 and p06 in every r1 checkpoint.

## Steps
- [x] tools, card, data (324: 292/32), eval prompts (0 of 24), copy check 0, r1 (84.12 s)
- [x] early blind batch scored and unblinded (119 items)
- [x] r2: resumed r1, 200 new steps, seed 22, 86.33 s, peak RSS 1231880192 B, val 2.625/2.848/3.129/3.431; eval r2-250..400 written (24 each)
- [x] r3: r1 settings, lr 1e-5, seed 33, 88.85 s, peak RSS 1228390400 B, val 2.535/2.546/2.520/2.850; eval r3-50..200 written (24 each)
- [ ] Final fresh shuffled blind batch (tag final, seed 202: base, r1-50..200, r2-250..400, r3-50..200), score into eval/scoresfinal.csv, unblind
- [ ] Probe of the best, memo count, copy check on the answers, runs.md, find -newer check.

## Proof
- overlap.py '0 matches, N files checked'; heldout.py '0 of 24'; best mean >= 4.0 and above base or said plainly; find -newer runs.md | wc -l = 0 in yoda, sherlock-holmes, jack-sparrow.