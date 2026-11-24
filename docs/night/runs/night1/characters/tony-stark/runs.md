# Tony Stark runs

Base: mlx-community/Qwen2.5-1.5B-Instruct-4bit (HF cache, 840M, nothing fetched). mlx-lm 0.32.2 via `uv run --python 3.12`. Every run: lr 5e-6, mask_prompt, 200 iters, batch 4, 16 layers, scale 20, checkpoint every 50, a different seed per run. Timed with /usr/bin/time -l; every log line stamped by tools/ts.py (adapters/<run>.log).
Data (tools/build.py): r1 287 exchanges (258/29; copy kept in data/r1/): everyday 145 (51%), never 47 (N1-N5 6 each, N6 and N7 8 each), fact 40 (10 facts x 4), situation 30 (10 situations x 3), greeting 25 (2 answers per family, 5 sums all different). r2/r3 299 (269/30) with batch-fact-02 (+12). System line 62 words.
Eval: eval/prompts.jsonl, 25 held-out prompts; tools/leak.py: exact 0, 8-word 0. Fixed for every answer: temp 0.3, seed 42, repetition penalty 1.1, 220 tokens, the same system line. Scored 0-5 by eval/rubric.md plus facts pass/fail.
Scores come from the FINAL batch only: eval/blind-final.md, seed 13, 325 answers (13 sets x 25), 325 unique, scores in eval/scores-final.csv. The early batch (eval/blind-early.md, seed 5: base + r1 and its checkpoints) was used only to choose r2 and r3.

| run | base model | train/valid | iters | lr | rank/layers | seed | train s | peak mem (max RSS) | final val loss | mean in-voice | facts fails | copy matches | memo count | changed and why |
|-----|-----------|-------------|-------|----|-------------|------|---------|--------------------|----------------|---------------|-------------|--------------|------------|-----------------|
| base | Qwen2.5-1.5B-Instruct-4bit | 0/0 | 0 | - | - | - | 0 | - | - | 1.16 | 6 | 0 | 0 | the untuned model, same system line and settings |
| r1 | same | 258/29 | 200 | 5e-6 | 8/16 | 11 | 73.26 | 1197572096 B | 2.902 | 2.60 | 7 | 0 | 0 | first run: the Gandalf r1 settings, as the brief asked |
| r2 | same | 269/30 | 200 | 5e-6 | 8/16 | 22 | 74.80 | 1193099264 B | 2.768 | 2.72 | 6 | 0 | 0 | ONE variable against r1: the data, +12 fact-first phrasings (data/batch-fact-02.txt) because f1-f6 failed in every early-batch set; the split changed, so val loss is not comparable to r1 |
| **r3** | same | 269/30 | 200 | 5e-6 | 16/16 | 33 | 76.33 | 1262452736 B | 2.926 | **2.80** | 6 | 0 | 0 | ONE variable against r2: rank 8 -> 16 (trainable 5.276M -> 10.551M), same data; lr and iters had each been tried twice in earlier builds, and a new system line would have changed the eval line |

Best: **r3 (final, iter 200), mean in-voice 2.80, 6 facts fails (f1, f2, f4, f5, f6, g3), reached at 76.21 s**, above the base's 1.16. **It does not meet the 4/5 bar**: it is 1.20 below it.
Talk to it: `uv run --python 3.12 --with mlx-lm python -m mlx_lm generate --model mlx-community/Qwen2.5-1.5B-Instruct-4bit --adapter-path adapters/r3 --temp 0.3 --max-tokens 220 --system-prompt "$(cd tools && python3 -c 'from common import system_line; print(system_line())')" --prompt "..."`

## Checkpoints (FINAL batch; seconds = timestamp of the save line in adapters/<run>.log, from tools/ts.py)
| checkpoint | train s at save | val loss | mean in-voice | facts fails |
|---|---|---|---|---|
| r1-50 | 19.68 | 2.772 | 2.16 | 6 |
| r1-100 | 36.97 | 2.688 | 2.40 | 7 |
| r1-150 | 55.02 | 2.715 | 2.32 | 8 |
| r1-200 | 73.16 | 2.902 | 2.60 | 7 |
| r2-50 | 19.72 | 2.765 | 2.16 | 7 |
| r2-100 | 37.29 | 2.726 | 2.20 | 8 |
| r2-150 | 55.99 | 2.755 | 2.04 | 7 |
| r2-200 | 74.69 | 2.768 | 2.72 | 6 |
| r3-50 | 19.98 | 2.725 | 1.92 | 7 |
| r3-100 | 38.49 | 2.702 | 2.28 | 6 |
| r3-150 | 57.46 | 3.012 | 2.72 | 7 |
| r3-200 | 76.21 | 2.926 | **2.80** | 6 |

Early batch (choice only, eval/scores-early.csv): base 1.16; r1 50/100/150/200 = 2.16/2.36/2.36/2.60.

## Checks
- Copy: tools/overlap.py over data/*.jsonl and every eval answer file against research/*.md and research/sources/*.txt: files checked 18, research files 14, matches 0. One data line matching notes.md:44 was reworded before r1.
- Memorisation (tools/memo.py, tuned answers sharing an 8-word run with data/*.jsonl answers): 0 in base and in all 12 checkpoints.
- Probe of r3 (eval/probe-r3.md): 13 paraphrases of data user lines (9 greetings/openers, 2 sums, 1 fact, 1 identity; 12 are train lines, 'Hi!' is a valid line) -> verbatim 0 of 13; longest shared run 9 words, on 'How much is nine times eight?'.

## Lessons for the next character
Quality first:
1. The 1.5B base is the ceiling again: 2.80 at best, like Yoda 3.08, Sparrow 2.96, Gandalf 2.46 and Holmes 2.00.
2. Data and rank each gave a little. Final checkpoints: r1 2.60 -> r2 2.72 (data) -> r3 2.80 (rank 16). Both are small next to blind-scoring noise.
3. Facts did not take, even at 6 fact-first phrasings: f1 (JARVIS's namesake), f2 (Afghanistan), f5 (Thanos) and f6 (heir) fail in all 13 sets. The model invents Karen, Natasha, Nick Fury, Loki.
4. Gap dodges did not hold: with 8 'never state a gap' lines, most tuned sets still state drinking as fact (n5).
5. Val loss did not track voice: r3's lowest val (2.702 @100) scored 2.28; its best (2.80) has val 2.926.
6. Two or three different answers per greeting family and distinct sums worked: probe verbatim 0 of 13 (Yoda had 2 of 12).
7. Deduplication saved nothing here (325 of 325 unique) at temp 0.3, seed 42.
Speed:
8. The best score came at 76.21 s (r3-200), and the last checkpoint was the best of every run, so 200 iters at 5e-6 may still be short. Resuming r3 to 300 is the next speed test.
9. Rank 16 cost about 1.5 s and 69 MB of RSS more than rank 8. Memory (about 1.2 GB) is not a limit.
