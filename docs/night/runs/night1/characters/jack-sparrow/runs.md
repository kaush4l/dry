# runs: Captain Jack Sparrow

Base model for every run: mlx-community/Qwen2.5-1.5B-Instruct-4bit, fetched once. Its size is 840M (`du -shL`; model.safetensors is 868,628,559 B). It lives at ~/.cache/huggingface/hub/models--mlx-community--Qwen2.5-1.5B-Instruct-4bit/snapshots/8b403126fc14f14cfc99bb4cfa72ecbc129ea677.
Training used mlx-lm 0.32.2 through `uv run --python 3.12 --with mlx-lm python -m mlx_lm lora -c adapters/<run>.yaml`, with seed 0, batch 4, mask_prompt, max_seq_length 1024 and LoRA scale 20, dropout 0.
The eval in tools/eval.py uses the same system line as training (from card.md Who + Voice, 167 words), temp 0 and max-tokens 220, on 24 held-out prompts (eval/prompts.jsonl).
Train s and peak mem come from `/usr/bin/time -l` around the lora command, stored at the end of adapters/<run>.log. Peak mem is the max resident set size; this mlx-lm log prints no peak-memory line of its own.
In-voice is scored 0-5 and facts pass/fail, per eval/rubric.md. Both were scored blind by me from shuffled files with hidden ids: batch 1 is eval/blind.md + eval/scores.csv (base, r1, r2; seed 7), and batch 2 is eval/blind2.md + eval/scores2.csv (base, r3; seed 11).
Copy matches is the number of 8-word runs shared with research/*.md and research/sources/*.txt, from tools/overlap.py over data/*.jsonl and eval/*.md. Prompt leaks into the training data (tools/leak.py, exact match + 8-word): 0.
Memorisation (tuned answers vs data/*.jsonl, 8-word runs, on the 24 held-out eval prompts only): r1 0 of 24, r3 1 of 24 (p12). No r1 eval-set answer contains 'Hello'. Off the eval set, r1 given the prompt 'Hello.' (temp 0, training system line) returned the answer at data/train.jsonl:106 (data/batch-02.txt:91-92, the reply to 'Hi there.') word for word.
data/train.jsonl and data/valid.jsonl were rewritten after the r3 training, by the test-phase reruns of tools/build.py. Their sha256 matches an in-memory rebuild from data/batch-*.txt (seed 0), so the content is unchanged from what r1-r3 trained on.

| run | base model | train/valid | iters | lr | rank/layers | train s | peak mem | final val loss | mean in-voice | facts fails | copy matches | changed, and why |
|-----|------------|-------------|-------|----|-------------|---------|----------|----------------|---------------|-------------|--------------|------------------|
| base | Qwen2.5-1.5B-Instruct-4bit | 0/0 | 0 | - | - | 0 | - | - | 1.42 | 2 (p11, p12) | 0 | Untuned model with the same system line. It is a plain assistant with costume words, and it calls itself a language model on p15. Batch-2 rescore of the same answers: 1.33. |
| r1 | Qwen2.5-1.5B-Instruct-4bit | 215/24 | 400 | 1e-5 | 8/16 | 293.92 | 1.23 GB (1233928192 B) | 6.074 (min 2.790 at iter 50) | **2.96** | 2 (p19, p21) | 0 | First run. Train loss fell to 0.010 while val loss rose after iter 100, a clear overfit by val loss, yet it is the best voice. |
| r2 | Qwen2.5-1.5B-Instruct-4bit | 215/24 | 100 | 1e-5 | 8/16 | 75.91 | 1.23 GB (1234468864 B) | 2.847 | 2.21 | 2 (p11, p20) | 0 | iters 400 -> 100, to stop near the val minimum. Val loss was best, but the voice was worse: the model falls back to assistant answers. |
| r3 | Qwen2.5-1.5B-Instruct-4bit | 215/24 | 200 | 1e-5 | 8/16 | 146.04 | 1.23 GB (1234567168 B) | 3.761 | 2.83 | 4 (p11, p12, p20, p21) | 0 | iters 400 -> 200, the midpoint, to test whether r1's voice can be kept with less overfit. The voice was nearly r1's and this was the only run that passed p15 (AI question, 3 vs 0), but it had more facts fails: an invented marriage to Barbossa, a compass described as an ordinary instrument, a wrong boiling point, and hinge nonsense. |

Best run: r1 (adapters/r1/adapters.safetensors). **It does not meet the bar.** Its mean in-voice is 2.96, short of the 4.0 target, though it is +1.54 over the base, and it has 2 facts fails where the target was 0. Talk to it with:
`uv run --python 3.12 --with mlx-lm python -m mlx_lm generate --model mlx-community/Qwen2.5-1.5B-Instruct-4bit --adapter-path adapters/r1 --temp 0 --max-tokens 220 --system-prompt "$(cd tools && uv run --python 3.12 python -c 'from common import system_line; print(system_line())')" --prompt "..."`

## Lessons for the next character

Quality first:
1. Do not pick a run or checkpoint by val loss. Here the lowest val loss (r2, 2.85) was the worst tuned voice, and the highest (r1, 6.07) was the best. With about 240 short hand-written exchanges, val loss rises after about 1 epoch (54 iters) while the style keeps sinking in. Save checkpoints every 50-100 iters and choose by a blind voice score on a few prompts.
2. Everyday questions are where the voice leaks out: lists and "Here are a few steps". The data had about 80 everyday exchanges out of 239. Next time aim for half the data, with short in-voice answers, and include several list-shaped questions answered without lists.
3. Never-tests need many examples, not one or two. With 2 AI and 1 actor example, "are you a language model" still scored 0 in base, r1 and r2. Give each Never 6-10 varied examples, with paraphrases of the trigger.
4. Facts drift both ways. Tuning taught the manner but also invented canon (Barbossa "married", a reversed marooning) and broke everyday facts (90 C boiling point, hinge advice). Add more exchanges that state the card's facts plainly in voice, and more correct everyday facts with a light touch.
5. The 1.5B 4-bit base repeats itself at temp 0 (loops on p01, p08, p16) and sometimes recites the system line. In the next eval try a small repetition penalty or temp 0.3 with a fixed seed, and a shorter system line, under 80 words.
6. Memorisation, measured: on the 24 held-out eval prompts, 0 of 24 r1 answers share an 8-word run with data/*.jsonl, and r3 has 1 of 24 (p12). The only whole-sentence echo in r1 there is the 3-word "I'm only asking." on p14, which also appears at data/train.jsonl:94. Off the eval set it does recite: the prompt 'Hello.' returned data/train.jsonl:106 (the 'Hi there.' answer) word for word, so a prompt close to a training line gets the training answer back. overlap.py only checks against research/, so the next build should run the same 8-word check of the tuned answers against data/*.jsonl and put the count in each row. It should also probe the tuned model with paraphrases of training user lines (greetings, short openers), not only the eval set.
7. Blind scoring is only partly blind when the base answers are identical across batches (temp 0). Shuffle every run into one batch at the end, or have a second scorer.

Speed:
1. Training costs about 0.73 s per iter at batch 4, rank 8 and 16 layers on this machine: 100 iters in 76 s, 400 in 294 s. Peak RSS is about 1.23 GB in every run, so memory is not the limit; there is room for a larger batch or rank.
2. The eval runs at about 250 tok/s for the base, so 24 prompts take well under a minute. One training run plus checkpoint evals fits in about 5 minutes.
3. The next builds should do one run of about 300 iters saving every 50, then eval 3-4 checkpoints instead of separate runs. That is cheaper than retraining shorter runs, and r3 shows the curve is the same at seed 0.
