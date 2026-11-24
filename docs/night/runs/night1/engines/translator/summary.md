Asked: overnight and unattended, build 5 characters under /Volumes/Backup/dry-build/night-run/night1/proj/characters, one folder each. Jack Sparrow comes first, from public facts and manner only, with no copied film lines. You chose nothing else, so I picked the other four. Done meant five folders, each with research/notes.md and summary.md, card.md, data/, an adapter, a scored eval/ and runs.md. It also meant one combined characters/runs.md and this summary. Nothing was to be committed, pushed or published, and nothing was.

Direction:
- Holmes, Yoda, Gandalf and Tony Stark — because they are well known and their voices differ (Victorian deduction, inverted syntax, archaic grave, fast sarcasm) — instead of lesser-known characters.
- One base, mlx-community/Qwen2.5-1.5B-Instruct-4bit (du -shL 840M; model.safetensors 868,628,559 B), fetched once and shared — because you asked for one small shared base — instead of the 0.5B sibling, which was kept as a fallback and not used.
- claude/opus for every child — because claude/sonnet failed safeguards ([reasoning_extraction]) on all 8 children it was given, each before any reply (5 research, dev-sparrow, review-sparrow) — instead of sonnet, the cheaper row.
- 8-word copy check of research against saved sources, required to be 0 — because the Yoda check found near-verbatim Wikipedia phrases — instead of trusting the rewording.
- Plan first for every developer, and a reviewer before each tick — because each build is many files plus training — instead of accepting the developer's own report.
- Holmes lesson 1 (a 3B-7B base) not applied — because it breaks the one-small-base rule — instead of switching bases mid-night.
- Holmes r1 used the Sparrow r1 settings, and r3 used lr 5e-6 with mask_prompt and 200 iters. Yoda and Gandalf r1 started from that.
- A fresh final blind batch decides the best run, after an early batch used only to choose r2/r3 — because the Yoda plan used r1's blind scores to pick runs.
- A different seed per run — because Yoda r3 checkpoints 50-200 were byte-identical to r2's.
- Stark r2/r3 spent their one variable on data and rank — because more iters and lr 1e-5 did not beat r1-150 on Gandalf.
- Peak memory from /usr/bin/time -l on every Stark run — because Gandalf r1's was never logged.
- Stray scratchpad files left in place — because they sit in other sessions' directories, outside every child's scope.

Work (bar for every build: mean in-voice >= 4/5 and above the base; none met it):
- Research, 5 deep-research children on opus (sonnet attempts failed). All files exist with final copy 0/0/0. Sparrow: 98/102/22 lines, 7 lines rewritten. Holmes: 89+83/152/21, 9 rewritten, 3 claims tagged after a send-back. Yoda: 84/106/22, 5 rewritten. Gandalf: 102/146/20, 16 rewritten. Stark: 91/104/11, 1 rewritten; it first stopped on a 600 s timeout and was told to go on.
- dev-sparrow-opus: base 1.417; r1 2.958 (lr 1e-5, 400 iters, 293.92 s, best); r2 2.208; r3 2.833. Peak RSS ~1.23 GB; 239 exchanges, 215/24. Accepted by review-sparrow-opus after two rounds. One round fixed a misread grep. The other found that the prompt 'Hello.' recites train.jsonl:106 word for word.
- dev-holmes: base 1.00; r1 1.62 (167.7 s); r2 1.67 (177.8 s); r3 2.00 (85.9 s, 1.23 GB, val 2.804, 4 facts fails). 248 exchanges, 223/25. Accepted by review-holmes.
- dev-yoda: base 1.08; r1 73.6 s (r1-150 3.04 at 55.1 s); r2 74.9 s; r3 at lr 1e-5 and 400 iters 153.0 s, best ckpt 300 at 3.08 (115.0 s, 1.19 GB). 280/31; probe 2 of 12 verbatim. Accepted by review-yoda; 4 runs.md doc fixes made after.
- dev-gandalf: base 1.29 (6 fails); r1-150 2.46 at 63.30 s (best); r2-400 2.33 (86.22 s); r3 at lr 1e-5 best 2.29 (88.87 s). 292/32; probe 0 of 13. review-gandalf sent it back once on runs.md:41/:48/:13, then accepted.
- dev-stark: base 1.16; r1 2.60 (73.26 s); r2 +12 fact lines 2.72 (74.80 s); r3 rank 16 2.80 (76.33 s, 1262452736 B, 6 fails). 269/30; memo 0 in 13 sets; probe 0 of 13. Accepted by review-stark; runs.md:37 fixed (17 -> 18).
- characters/runs.md (dev-stark): 71 rows, all checked cell by cell by review-stark against the cited lines, 0 mismatches.

Learned:
- Val loss did not track voice in any build. Examples: Holmes val minimum at iter 100 scored 1.58; Stark minimum at iter 100 scored 2.28.
- The best scores came from short training. Yoda r1-150 got 3.04 in 55.1 s. Gandalf r1-150 got 2.46, and no later checkpoint beat it.
- The 1.5B base invents facts in every build. Examples: Holmes Mycroft as Watson and 9x8=64; Gandalf's horse wrong in 11 of 12 checkpoints; Stark f1, f2, f5 and f6 failing in all 13 sets.
- Near-duplicate prompts recite training answers: Sparrow 'Hello.', and Yoda 'Hi!' and 9x8.

Not done or not verified:
- No build reached 4/5. The best was Yoda r3-300 at 3.08.
- Every score comes from one self-scorer per build, with no judge model. Each developer had seen r1's answers before its final batch.
- Holmes data split cannot be counted, because the batches have no headers. Gandalf's fact batch was never written, and the cause is unknown. Gandalf r1 peak memory is not logged.
- Research gaps are marked open in the research files. Examples: no linguist source for Yoda's word order, McKellen's account not reached, Stark jargon and deflection inferred.
- 12 stray Stark eval dumps (03:21:18-03:30:10). Whether they overwrote anything cannot be proved. Delete them if you like:
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/d9d09d42-9849-4c82-90af-d5436160a841/scratchpad/early.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e2.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e3.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e4.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e5.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e6.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e7.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/e8.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/6f48c28d-2f4b-4b75-ae41-ecf70c816edd/scratchpad/early.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/3a541b1f-d789-4022-9002-e0c8d9852b88/scratchpad/early.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/de36572c-5f96-4532-818f-55be050c4472/scratchpad/final.txt
/private/tmp/claude-501/-private-var-folders-n3-l6pr1nds4bs00x624-0-qqdr0000gn-T-dry-cli/4d441725-0c77-4188-bc82-cd68f3074a0c/scratchpad/miss.txt

Files (each under /Volumes/Backup/dry-build/night-run/night1/proj/characters/):
- runs.md (combined table)
- In each of jack-sparrow, sherlock-holmes, yoda, gandalf and tony-stark: research/, card.md, data/, adapters/, eval/, tools/ and runs.md