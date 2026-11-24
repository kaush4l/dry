# Yoda: runs

Base model: mlx-community/Qwen2.5-1.5B-Instruct-4bit (HF cache, 840M, nothing fetched). Data: 311 exchanges in 7 labelled batches (everyday 146, never 64, fact 59, situation 22, greeting 20), split 280 train / 31 valid (seed 42). Eval: 24 held-out prompts (eval/prompts.jsonl, tools/leak.py: 0 exact, 0 8-word overlaps). Generation, the same for every answer: temp 0.3, seed 42, repetition penalty 1.1, 220 tokens, system line 76 words.

Scores come from the FINAL blind batch only: eval/blind-final.md, 408 answers (base + 16 checkpoints x 24 prompts), shuffled with seed 17, scored into eval/scores-final.csv before `tools/blind.py unblind -final`. Two earlier blind batches (eval/blind-sel.md, seed 11, 120 items; eval/blind-sel2.md, seed 13, 96 items) were used only to choose r2 and r3.

Peak memory is the maximum resident set size from `/usr/bin/time -l` in adapters/<run>.log. mlx-lm printed no peak-memory line of its own. Train seconds are wall seconds from the same log.

| run | base model | train/valid | iters | lr | rank/layers | train s | peak mem | final val loss | mean in-voice | facts fails | copy matches | memo count | changed and why |
|-----|-----------|-------------|-------|----|-------------|---------|----------|----------------|---------------|-------------|--------------|------------|-----------------|
| base | Qwen2.5-1.5B-Instruct-4bit | 0/0 | 0 | - | - | 0 | - | - | 1.08 | 2 | 0 | 0 | the untuned model with the same system line |
| r1 | Qwen2.5-1.5B-Instruct-4bit | 280/31 | 200 | 5e-6 | 8/16, mask_prompt | 73.6 | 1.22 GB | 2.297 | 2.88 (best ckpt 150: 3.04) | 4 | 0 | 0 | Holmes r3 settings, the best of the earlier builds |
| r2 | Qwen2.5-1.5B-Instruct-4bit | 280/31 | 200 | 1e-5 | 8/16, mask_prompt | 74.9 | 1.22 GB | 2.499 | 2.96 | 4 | 0 | 0 | lr doubled: in the selection batch about a third of r1's answers fell back to long assistant prose or lists |
| r3 | Qwen2.5-1.5B-Instruct-4bit | 280/31 | 400 | 1e-5 | 8/16, mask_prompt | 153.0 | 1.19 GB | 4.154 | 3.04 (best ckpt 300: **3.08**) | 3 (ckpt 300: 2) | 0 | 0 | against r2 only iters changed (200 -> 400), because r2's voice was still rising at 200 (2.54 -> 3.04 in selection-2); against r1 both lr (5e-6 -> 1e-5) and iters (200 -> 400) changed |

**Best: r3 checkpoint 300 (adapters/r3/0000300_adapters.safetensors, also adapters/r3/ckpt-300/), mean in-voice 3.08, facts fails 2 (p15 Dooku, p23 insect legs), reached at 115.0 s of training.** It ties r3-350 (3.08, fails 2) and was picked because it got there sooner. The bar of 4.0 is NOT met. The best run is above the base (3.08 vs 1.08).

Copy check: `tools/overlap.py` (data/*.jsonl and the eval answer files) against research/*.md and research/sources/*.txt: files checked: 32, research files: 9, matches: 0. Base answers: 0 matches.
Memorisation: `tools/memo.py`, tuned answers sharing an 8-word run with data/*.jsonl answers: 0 in every file (f-base and all 16 f- checkpoints).
Probe (eval/probe-r3-300.md, 12 paraphrases of training user lines): verbatim 2 of 12. 'Hi!' reproduced the whole 'Hi there!' answer (11 words). 'How much is nine times eight?' began with the whole 'What is 9 times 8?' answer (6 words), then added wrong arithmetic. 'Who made you?' shares 13 words with a training answer but is not a whole copy. The other 9 share at most 4 words.

## Checkpoints (FINAL batch; seconds = timestamp of the save line in adapters/<run>.log, from tools/ts.py)

| run | iter | saved at s | val loss | mean in-voice | facts fails |
|-----|------|-----------|----------|---------------|-------------|
| r1 | 50 | 19.8 | 2.440 | 2.62 | 4 |
| r1 | 100 | 37.1 | 2.337 | 2.83 | 4 |
| r1 | 150 | 55.1 | 2.263 | 3.04 | 4 |
| r1 | 200 | 73.5 | 2.297 | 2.88 | 4 |
| r2 | 50 | 19.5 | 2.330 | 2.54 | 4 |
| r2 | 100 | 37.3 | 2.257 | 2.75 | 3 |
| r2 | 150 | 56.0 | 2.392 | 2.92 | 3 |
| r2 | 200 | 74.8 | 2.499 | 2.96 | 4 |
| r3 | 50 | 19.7 | 2.330 | 2.54 | 4 |
| r3 | 100 | 38.2 | 2.257 | 2.75 | 3 |
| r3 | 150 | 57.5 | 2.392 | 2.92 | 3 |
| r3 | 200 | 76.8 | 2.499 | 2.96 | 4 |
| r3 | 250 | 96.0 | 2.999 | 2.88 | 4 |
| r3 | 300 | 115.0 | 3.865 | 3.08 | 2 |
| r3 | 350 | 133.9 | 3.712 | 3.08 | 2 |
| r3 | 400 | 152.9 | 4.154 | 3.04 | 3 |

r2 and r3 checkpoints 50-200 are byte-identical (cmp; same seed 42, lr and data). r3 is r2 continued, so their scores at 50-200 match.

Selection batches (choice only): base 1.12; r1 50/100/150/200 = 2.67/2.83/2.96/2.92; r2 50/100/150/200 = 2.54/2.71/2.92/3.04.

## What the answers show
- Tuning removes most of the assistant manner. The base gives long 'You must be patient...' paragraphs and markdown bullets. The tuned checkpoints give short fronted clauses, tags ('hmm', 'heh'), and plain firm orders on the reactor prompt.
- Points are lost to garbled inversion ('Ready for what comes today, that day make you are'), non sequiturs, and literal readings of the bullet prompt: even the best checkpoints still number five points.
- Facts: 'over eight hundred years' is right in some checkpoints, but many still say thirty. The maker prompt still invents Lucas as a businessman, the Council or Lothal. Dooku gets reversed (Yoda as his apprentice), and insects get 'many' legs.

## Lessons for the next character
Quality first:
1. The 1.5B base is the ceiling. Three Yoda runs, like the Holmes and Sparrow runs, plateau near 3/5. A costume-heavy voice (word order) is easier than Holmes' prose, but coherence and facts limit it. A 3B-7B instruct base is the next variable to test, if the person allows it.
2. Val loss ran opposite to voice again. r3's val loss rose from 2.26 to 4.15 while voice rose from 2.75 to 3.08. Choose by blind checkpoint scores.
3. Five or six phrasings per fact were not enough for 'over 800 years' and the maker. Next time, put the exact fact in the first words of every phrasing, and add more paraphrased user lines rather than more answers.
4. Short training answers still leak. 'Hi!' recited the whole 'Hi there!' answer (11 words), and 'How much is nine times eight?' recited the whole 'What is 9 times 8?' answer, then added wrong arithmetic (eval/probe-r3-300.md:49-52). Give each greeting family and each sum two or three different answers.
5. A 'never list' rule needs examples where the user explicitly asks for N bullet points. Mine asked for lists in general, and the model still counts to five.
Speed:
6. The best score, r3-300 at 3.08, took 300 iters at lr 1e-5 and 115.0 s of training. r1-150 got 3.04 at lr 5e-6 in 55.1 s, within 0.04 of the best. At lr 1e-5 the same 150 iters (r3-150) scored only 2.92, so lr and iters are separate variables: 150 iters at 5e-6 came close to 300 iters at 1e-5 in half the time.
7. A run that only adds iterations with the same seed repeats the earlier run byte for byte. Resume from the earlier adapter instead (--resume-adapter-file) to save about 75 s and avoid identical answers in the blind batch.
8. Scoring 408 blind answers was the slowest step again, and many answers repeated across checkpoints. Next time, de-duplicate identical answers before shuffling (score once, copy to each run), and score only 3 checkpoints per run.
