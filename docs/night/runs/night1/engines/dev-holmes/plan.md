## Ask
Sherlock Holmes persona on Qwen2.5-1.5B-Instruct-4bit: card, data, 3 runs with checkpoints, one blind eval, runs.md.

## Steps
- [x] card.md (0 copy hits)
- [x] data 248 (223/25), 0 copy matches
- [x] tools adapted + memo.py, probe.py
- [x] prompts 24 (leaks 0), rubric, base.md
- [x] r1 167.7 s, r2 177.8 s, r3 85.9 s; checkpoints at 50/100/200
- [x] ONE blind batch of 288 scored and unblinded: base 1.00, best r3 2.00 (bar NOT met)
- [x] copy 0 in tuned/data; memorisation r2 1 (p21); probe r3 0/10 verbatim
- [x] runs.md with rows, checkpoint seconds, lessons

## Proof
build counts 223/25; leak 0; overlap 0 in data and tuned answers; best 2.00 > base 1.00 but < 4.0, said plainly.