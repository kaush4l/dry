## Ask
Jack Sparrow persona: card, original data, LoRA on Qwen2.5-1.5B-Instruct-4bit (mlx-lm), eval vs base, copy check, runs.md with rows and lessons.

## Steps
- [x] card.md: 44 citations, inferred marked
- [x] data/batch-01..06.txt: 239 exchanges written new
- [x] tools/build.py: train 215 / valid 24, seed 0, system line 167 words
- [x] tools/overlap.py: matches: 0 (data/*.jsonl incl. system line vs 17 research files)
- [x] tools/leak.py: exact 0, 8-word 0, leaks: 0
- [x] eval/prompts.jsonl: 24 held-out prompts (10 situations, 3 facts, 3 Never, 8 everyday)
- [x] eval/rubric.md: in-voice 0-5 anchors and facts pass/fail
- [x] tools/eval.py: mlx_lm library, same system line, temp 0, max-tokens 220, writes eval/<name>.md and .jsonl
- [x] tools/blind.py: batch 1 (base,r1,r2 seed 7) and batch 2 (base,r3 seed 11), scored blind, unblinded
- [x] eval/base.md: 24 answers, mean 250.3 tok/s
- [x] fetch base once: 840M (du -shL), ~/.cache/huggingface/hub/models--mlx-community--Qwen2.5-1.5B-Instruct-4bit/snapshots/8b403126fc14f14cfc99bb4cfa72ecbc129ea677
- [x] r1 (400 iters) 2.96, r2 (100) 2.21, r3 (200) 2.83; base 1.42; 3 runs used, no training failure
- [x] runs.md: base + 3 rows, every column, lessons (quality, then speed); overlap matches 0 over 8 files, leaks 0

## Proof
overlap.py prints 'matches: 0' (data + eval answers); leak.py 'leaks: 0'; best eval/<run>.md header shows mean in-voice >= 4.0, above base, 0 facts fails. RESULT: bar not met (best r1 2.96, 2 fails), above base 1.42.