## Ask
Yoda persona on Qwen2.5-1.5B-Instruct-4bit (cached): card, labelled original data, at most 3 LoRA runs, blind evals against the base, and runs.md with lessons.

## Steps
- [x] tools, card, data (311), split 280/31, prompts 24 (leak 0), rubric.
- [x] r1 73.6 s, r2 74.9 s, r3 153.0 s; selection batches; FINAL batch 408: best r3-300 3.08 vs base 1.08, bar 4.0 NOT met.
- [x] Copy check 0 matches (26 files); memo 0 in all; probe r3-300 verbatim 2/12.
- [x] runs.md: rows, checkpoint table, lessons.
- [ ] End check: sherlock-holmes find shows 2 newer files; identify them (mine or not) before reporting. jack-sparrow 0.

## Proof
overlap.py 0; leak.py 0/24; blind.py unblind -final: best 3.08 < 4.0, above base 1.08 (said plainly).