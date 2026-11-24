## Ask
Tony Stark persona on Qwen2.5-1.5B-Instruct-4bit (cached): card, original data, at most 3 LoRA runs, a blind eval against the base, copy and memo checks, tony-stark/runs.md and the combined characters/runs.md.

## Steps
- [x] tools/: copied the yoda tools; common.py HEAD set to Stark (system line 62 words). The lora --help flags were checked. probe.py pairs set to Stark data lines.
- [x] card.md: every point cited to file:line, with gaps marked (inferred) or (open).
- [x] data/batch-<cat>-NN.txt: 287 exchanges (258/29), then 299 (269/30) with batch-fact-02. 0 duplicates; overlap 0.
- [x] eval/prompts.jsonl: 25 prompts, leak.py 0. eval/rubric.md. Fixed settings: temp 0.3, seed 42, rep 1.1, 220 tokens.
- [x] r1 (seed 11), r2 (data, seed 22), r3 (rank 16, seed 33): all timed with /usr/bin/time -l and ts.py logs.
- [x] Early blind batch (choice only); final fresh batch of 325 unique answers decides. Best r3 2.80 against base 1.16; the 4/5 bar is not met.
- [x] Copy 0 matches (17 files), memo 0 in all sets, probe r3 0 of 13 verbatim.
- [x] tony-stark/runs.md; characters/runs.md (71 rows, file:line, 'not logged').
- [x] find -newer: gandalf 0, yoda 0, jack-sparrow 0, sherlock-holmes 2. Those 2 are its data/train.jsonl and valid.jsonl at 01:27:54, 15 s after its runs.md (01:27:39). That is before this build began (r1 03:17, data/ 03:24), so not written by me.

## Proof
- overlap.py: 0 matches and the number of files checked.
- leak.py: 0.
- Best mean in-voice of 4 or more and above the base, or I say so plainly.
- find -newer prints 0 for each of the four folders (sherlock-holmes 2, explained above).
- characters/runs.md row count shown.