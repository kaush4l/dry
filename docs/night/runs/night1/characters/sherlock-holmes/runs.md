# Runs: Sherlock Holmes on mlx-community/Qwen2.5-1.5B-Instruct-4bit

Data: 248 exchanges written new (data/batch-01..06.txt), built to train 223 / valid 25 by tools/build.py; system line 61 words. Eval: 24 held-out prompts (tools/leak.py: 0 exact, 0 eight-word overlaps). Fixed settings for every answer: temp 0.3, seed 42 before each prompt, repetition penalty 1.1, 220 tokens. Scoring: all 288 answers (base, 3 runs, 8 checkpoints) in ONE shuffled blind batch (eval/blind.md, seed 7), scored by me against eval/rubric.md into eval/scores.csv, then unblinded by tools/blind.py. No judge model.

## One row per run

| run | base model | train/valid | iters | lr | rank/layers | train s | peak RSS | final val loss | mean in-voice | facts fails | copy matches | memorisation | changed and why |
|-----|-----------|-------------|-------|----|-------------|---------|----------|----------------|---------------|-------------|--------------|--------------|-----------------|
| base | Qwen2.5-1.5B-Instruct-4bit | 0/0 | 0 | - | - | 0 | - | 4.249 (iter 1, r1 log) | 1.00 | 3 | 7 (one answer quoting the impossible/improbable maxim, base.md:110) | 0 | the untuned model, same system line |
| r1 | same | 223/25 | 400 | 1e-5 | 8 / 16 | 167.7 | 1.21 GB | 2.096 (min 1.060 @100) | 1.62 (best ckpt r1-200: 1.83) | 6 | 0 | 0 (r1-100: 1 echo of the system line, p21) | Jack r1 settings as the first run |
| r2 | same | 223/25 | 400 | 1e-5 | 8 / 16 | 177.8 | 1.23 GB | 5.522 (min 2.637 @100) | 1.67 (best ckpt r2-200: 1.92) | 5 | 0 | 1 (p21 'Good afternoon, sir.' gave the training answer to 'Good evening.' word for word) | mask_prompt: the 61-word system line was most of each example's loss |
| **r3** | same | 223/25 | 200 | 5e-6 | 8 / 16 | 85.9 | 1.23 GB | 2.804 (min 2.630 @100) | **2.00** | 4 | 0 | 0; probe 0 of 10 verbatim (eval/probe-r3.md, longest shared run 6 words) | r2 with half the lr and half the iters: r1 and r2 overfit after about one epoch (56 iters) |

Best run by blind voice: **r3 (iter 200), 2.00**, against base 1.00. The bar (mean in-voice of 4/5 or more) is **NOT met**. The lowest val loss (r3 @100, 2.630) scored 1.58, and r2's worst val loss (5.522) scored 1.67, so val loss does not track voice, as with Jack.

## Scored checkpoints (seconds to reach, from log timestamps)

| checkpoint | s to reach | val loss | mean in-voice | facts fails |
|------------|-----------|----------|---------------|-------------|
| r1-50 | 18.8 (est.) | 1.081 | 1.33 | 8 |
| r1-100 | 37.7 (est.) | 1.060 | 1.42 | 7 |
| r1-200 | 77.2 (est.) | 1.217 | 1.83 | 8 |
| r1-400 | 153.7 (est.) | 2.096 | 1.62 | 6 |
| r2-50 | 22.9 | 2.639 | 1.71 | 6 |
| r2-100 | 44.4 | 2.637 | 1.79 | 6 |
| r2-200 | 89.5 | 3.438 | 1.92 | 7 |
| r2-400 | 177.7 | 5.522 | 1.67 | 5 |
| r3-50 | 23.4 | 2.680 | 1.38 | 6 |
| r3-100 | 43.9 | 2.630 | 1.58 | 4 |
| r3-200 | 85.8 | 2.804 | 2.00 | 4 |

r1 seconds are estimated from the tokens and tok/s columns: grep buffered the pipe, so every r1 line carries the same stamp (167.7). r2 and r3 used grep --line-buffered and have real stamps. Training runs at about 0.43 s/iter (r3), and peak RSS is 1.23 GB on an M4 Max, so memory is not the limit.

## What the eval shows
- The tuned runs drop the assistant manner: no lists, no 'As Sherlock Holmes...', shorter answers. They pick up his moves: an apology on p06, asking for data on p03, deflecting praise on p09, reading the caller on p21.
- Most remaining points are lost to coherence and facts, not costume. The 1.5B base invents canon (an older brother who is a doctor or Watson, Watson wounded in the leg in 1892), gets everyday facts wrong (9 x 8 = 64 or 720, a spider with four legs) and drifts into non sequiturs. p14 ('Which company built you?') stays a facts fail in every run.
- Copy check: tools/overlap.py finds 0 eight-word runs shared with research/ or its sources in data/*.jsonl and every tuned answer. The 7 hits are all one base answer (and its copy in blind.md).

## Lessons for the next character
Quality first:
1. The base model is the ceiling. Three runs on 1.5B-4bit moved voice from 1.0 to 2.0 and could not fix facts. Before writing data, check the HF cache for a 3B-7B instruct model and use the largest that trains in under five minutes. Memory headroom here is roughly 100x.
2. Fact lines need repetition, not breadth. One line about Mycroft and one about Watson's wound did not stick; the model invented both. Write 4-6 phrasings of each canon fact the eval asks about, and short exact answers for sums and counts.
3. Score checkpoints blind and choose by voice. In all three runs the val-loss minimum was not the best checkpoint.
4. Probe greetings early. r2 recited a training greeting word for word on a held-out 'Good afternoon'. Give greetings several different answers, or vary the greeting lines more.
5. Keep the 'Which company built you?' style of prompt in the data: 0 of 3 runs answered it in character without inventing a maker.
Speed:
6. The best score (r3, 2.00) took 85.8 s of training, half of r1's 167.7 s. A low lr (5e-6) with mask_prompt for about 3.5 epochs (200 iters at batch 4) beat 400 iters at 1e-5. Start the next character there, and save every 50 iterations.
7. Pipe training through grep --line-buffered and a timestamp from the first run, so checkpoint seconds are measured, not estimated.
8. Reading 288 blind answers was the slowest step, not training. Next time, generate with fewer checkpoints (50, 100 and the end), or add a judge model scored apart from my own scores.
