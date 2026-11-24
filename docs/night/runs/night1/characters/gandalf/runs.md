# Gandalf runs

Base: mlx-community/Qwen2.5-1.5B-Instruct-4bit (already in the HF cache; nothing fetched). mlx-lm 0.32.2 under `uv run --python 3.12`. Every run: mask_prompt, rank 8, 16 layers, batch 4, scale 20, checkpoint every 50.
Data (tools/build.py): 324 exchanges, train 292 / valid 32. everyday 152 (47%), situation 60 (10 situations x 6), never 48 (6 rules x 8), fact 41 (8 facts, 4-6 phrasings each), greeting 23. 20 arithmetic answers, all unique. System line 70 words.
Eval: eval/prompts.jsonl, 24 held-out prompts, `heldout.py` -> 0 of 24 (exact or 8-word overlap). Fixed settings: temp 0.3, seed 42, repetition penalty 1.1, 220 tokens. Scored 0-5 by eval/rubric.md, plus facts pass/fail.
Scores below come from the FINAL batch only: eval/blindfinal.md, seed 202, 312 answers from 13 entries, 309 unique items, scores in eval/scoresfinal.csv. The early batch (eval/blindearly.md, seed 101: base + r1) was used only to choose r2 and r3.

| run | base model | train/valid | iters | lr | rank/layers | train s | peak mem | final val loss | mean in-voice (best ckpt) | facts fails | copy matches | memo count | changed and why |
|-----|-----------|-------------|-------|----|-------------|---------|----------|----------------|---------------------------|-------------|--------------|------------|-----------------|
| base | Qwen2.5-1.5B-Instruct-4bit | 0/0 | 0 | - | - | 0 | - | - | 1.29 | 6 | 0 | 0 | the untuned model, same system line and settings |
| r1 | same | 292/32 | 200 | 5e-6 | 8/16 | 84.12 | not recorded (run without time -l) | 2.539 | **2.46 (r1-150)** | 4 | 0 | 0 | first run: the Yoda r1 settings, seed 11 |
| r2 | same | 292/32 | 400 (200 resumed + 200 new) | 5e-6 | 8/16 | 86.22 new (170.34 total) | 1231880192 B | 3.431 | 2.33 (r2-400) | 4 | 0 | 2 (r2-400: p07, p21) | ONE variable against r1: iters 200 -> 400, by resuming adapters/r1/adapters.safetensors. Seed 22 applies to the 200 new steps only. r2's steps 50-200 are r1's own checkpoints, scored once under r1. |
| r3 | same | 292/32 | 200 | 1e-5 | 8/16 | 88.85 | 1228390400 B | 2.850 | 2.29 (r3-150, r3-200) | 6 | 0 | 1 (r3-200: p21) | ONE variable against r1: lr 5e-6 -> 1e-5, seed 33. Why lr and not data: the early batch showed facts fails on p04/p05/p06 in every r1 checkpoint, so the planned change was extra fact phrasings. That fact batch was never written, so there was no data change to test, and r3 took the other variable the approved plan allowed, lr. |

Best: **r1-150**, mean in-voice 2.46, 4 facts fails (p04 ring, p05 horse, p06 Istari count, p07 rune), above the base's 1.29. **It does not meet the 4/5 bar**: it is 1.54 below it.

## Checkpoints (FINAL batch; seconds = timestamp of the save line in adapters/<run>.log, from tools/ts.py)
| checkpoint | train s at save | val loss | mean in-voice | facts fails |
|---|---|---|---|---|
| r1-50 | 22.79 | 2.602 | 2.21 | 5 |
| r1-100 | 42.66 | 2.527 | 2.17 | 5 |
| r1-150 | 63.30 | 2.492 | **2.46** | 4 |
| r1-200 | 84.12 | 2.539 | 2.38 | 5 |
| r2-250 | 84.12 + 22.51 = 106.63 | 2.625 | 2.08 | 7 |
| r2-300 | 84.12 + 43.29 = 127.41 | 2.848 | 2.21 | 5 |
| r2-350 | 84.12 + 64.59 = 148.71 | 3.129 | 2.25 | 6 |
| r2-400 | 84.12 + 86.22 = 170.34 | 3.431 | 2.33 | 4 |
| r3-50 | 22.65 | 2.535 | 2.12 | 6 |
| r3-100 | 44.55 | 2.546 | 2.12 | 6 |
| r3-150 | 66.59 | 2.520 | 2.29 | 6 |
| r3-200 | 88.77 | 2.850 | 2.29 | 6 |

## Checks
- Copy: `tools/overlap.py` -> 0 matches, 18 files checked (data/*.jsonl and every eval/*.md answer file) against 29 research files (26 saved sources). Two data lines that shared runs with research were reworded before r1.
- Memorisation (`tools/memo.py`, 8-word runs shared with data answers): base 0, r1-50..200 0, r2-250..350 0, r2-400 2, r3-50..150 0, r3-200 1.
- Probe of r1-150 (eval/probe-r1-150.md): 13 paraphrased training user lines (10 greetings/openers, 1 arithmetic, 2 facts) -> verbatim 0 of 13; longest shared run 5 words.

## Lessons for the Stark build
Quality first:
1. The 1.5B base is still the ceiling. Gandalf's best, 2.46, is below Yoda's 3.08 and Sparrow's 2.96: a plain-English voice gives the tuning less of a costume to learn than Yoda's word order does.
2. Facts did not take. With 4-6 phrasings per fact and the fact in the first words, 11 of 12 checkpoints still invented the horse (Haldir, Hal, Pippin; only r2-400 named Shadowfax), and all 12 failed the ring-giver (p04) and Istari-count (p06) prompts. More steps (r2) and a higher lr (r3) did not fix it. Next time, test the data variable this build could not: many more user-line paraphrases per fact, or a bigger base.
3. Single variables gave clean answers. Doubling iters lowered the score (2.46 -> 2.33 at best) while val loss rose 2.49 -> 3.43. Doubling lr lowered it too (2.29). For Gandalf at 1.5B, 5e-6 x 150 iters is the best point found.
4. Here val loss did track voice: r1-150 has both the lowest val loss and the best score. That is the opposite of Yoda and Holmes, so still pick by the blind score.
5. Late checkpoints start to memorise: r2-400 and r3-200 share 8-word runs with training answers (the 8x9 and the rune lines), and the earlier checkpoints do not.
6. 'Are you an AI?' stayed weak in every run ('Yes, and I am. I have been programmed...'). Eight 'never AI' examples were too few against the base's habit; write more, and include the exact question.
7. Deduplication saved little: 312 answers became 309 items. At temp 0.3 with seed 42, checkpoints rarely give identical answers.
Speed:
8. The best score came at 63.30 s of training (r1-150). No later checkpoint (to 170 s) beat it. Budget about 150 iters at 5e-6 (about 65 s) and spend the saved time on data. Peak RSS was about 1.23 GB, so memory is not a limit.
