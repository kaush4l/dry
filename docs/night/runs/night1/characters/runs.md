# All characters: every run and scored checkpoint

All five builds use mlx-community/Qwen2.5-1.5B-Instruct-4bit. Each value is copied as written from the cited `<folder>/runs.md:line`; nothing is recomputed. A value the source does not give is written 'not logged'. Checkpoint rows take the peak memory of their run. Settings are iters, lr and rank/layers. In-voice is the blind mean, 0-5.
Eval settings differ by build: jack-sparrow used temp 0 and a 167-word system line (jack-sparrow/runs.md:5). The others used temp 0.3, seed 42, rep 1.1 and 220 tokens (gandalf/runs.md:5, tony-stark/runs.md:5).

| character | run | settings | train s | peak mem | val loss | in-voice | facts fails | memo | best | source |
|---|---|---|---|---|---|---|---|---|---|---|
| jack-sparrow | base | - | 0 | - | - | 1.42 | 2 (p11, p12) | not logged | | jack-sparrow/runs.md:14 |
| jack-sparrow | r1 | 400, 1e-5, 8/16 | 293.92 | 1233928192 B | 6.074 (min 2.790 at iter 50) | 2.96 | 2 | 0 of 24 (jack-sparrow/runs.md:9) | **best** | jack-sparrow/runs.md:15 |
| jack-sparrow | r2 | 100, 1e-5, 8/16 | 75.91 | 1234468864 B | 2.847 | 2.21 | 2 | not logged | | jack-sparrow/runs.md:16 |
| jack-sparrow | r3 | 200, 1e-5, 8/16 | 146.04 | 1234567168 B | 3.761 | 2.83 | 4 | 1 of 24 (jack-sparrow/runs.md:9) | | jack-sparrow/runs.md:17 |
| sherlock-holmes | base | - | 0 | - | 4.249 (iter 1, r1 log) | 1.00 | 3 | 0 | | sherlock-holmes/runs.md:9 |
| sherlock-holmes | r1 | 400, 1e-5, 8/16 | 167.7 | 1.21 GB | 2.096 (min 1.060 @100) | 1.62 | 6 | 0 | | sherlock-holmes/runs.md:10 |
| sherlock-holmes | r2 | 400, 1e-5, 8/16, mask_prompt | 177.8 | 1.23 GB | 5.522 (min 2.637 @100) | 1.67 | 5 | 1 | | sherlock-holmes/runs.md:11 |
| sherlock-holmes | r3 | 200, 5e-6, 8/16 | 85.9 | 1.23 GB | 2.804 (min 2.630 @100) | 2.00 | 4 | 0 | **best** | sherlock-holmes/runs.md:12 |
| sherlock-holmes | r1-50 | r1 | 18.8 (est.) | see r1 | 1.081 | 1.33 | 8 | not logged | | sherlock-holmes/runs.md:20 |
| sherlock-holmes | r1-100 | r1 | 37.7 (est.) | see r1 | 1.060 | 1.42 | 7 | not logged | | sherlock-holmes/runs.md:21 |
| sherlock-holmes | r1-200 | r1 | 77.2 (est.) | see r1 | 1.217 | 1.83 | 8 | not logged | | sherlock-holmes/runs.md:22 |
| sherlock-holmes | r1-400 | r1 | 153.7 (est.) | see r1 | 2.096 | 1.62 | 6 | not logged | | sherlock-holmes/runs.md:23 |
| sherlock-holmes | r2-50 | r2 | 22.9 | see r2 | 2.639 | 1.71 | 6 | not logged | | sherlock-holmes/runs.md:24 |
| sherlock-holmes | r2-100 | r2 | 44.4 | see r2 | 2.637 | 1.79 | 6 | not logged | | sherlock-holmes/runs.md:25 |
| sherlock-holmes | r2-200 | r2 | 89.5 | see r2 | 3.438 | 1.92 | 7 | not logged | | sherlock-holmes/runs.md:26 |
| sherlock-holmes | r2-400 | r2 | 177.7 | see r2 | 5.522 | 1.67 | 5 | not logged | | sherlock-holmes/runs.md:27 |
| sherlock-holmes | r3-50 | r3 | 23.4 | see r3 | 2.680 | 1.38 | 6 | not logged | | sherlock-holmes/runs.md:28 |
| sherlock-holmes | r3-100 | r3 | 43.9 | see r3 | 2.630 | 1.58 | 4 | not logged | | sherlock-holmes/runs.md:29 |
| sherlock-holmes | r3-200 | r3 | 85.8 | see r3 | 2.804 | 2.00 | 4 | not logged | **best** | sherlock-holmes/runs.md:30 |
| yoda | base | - | 0 | - | - | 1.08 | 2 | 0 | | yoda/runs.md:11 |
| yoda | r1 | 200, 5e-6, 8/16, mask_prompt | 73.6 | 1.22 GB | 2.297 | 2.88 (best ckpt 150: 3.04) | 4 | 0 | | yoda/runs.md:12 |
| yoda | r2 | 200, 1e-5, 8/16, mask_prompt | 74.9 | 1.22 GB | 2.499 | 2.96 | 4 | 0 | | yoda/runs.md:13 |
| yoda | r3 | 400, 1e-5, 8/16, mask_prompt | 153.0 | 1.19 GB | 4.154 | 3.04 (best ckpt 300: 3.08) | 3 (ckpt 300: 2) | 0 | | yoda/runs.md:14 |
| yoda | r1-50 | r1 | 19.8 | see r1 | 2.440 | 2.62 | 4 | 0 (yoda/runs.md:19) | | yoda/runs.md:26 |
| yoda | r1-100 | r1 | 37.1 | see r1 | 2.337 | 2.83 | 4 | 0 | | yoda/runs.md:27 |
| yoda | r1-150 | r1 | 55.1 | see r1 | 2.263 | 3.04 | 4 | 0 | | yoda/runs.md:28 |
| yoda | r1-200 | r1 | 73.5 | see r1 | 2.297 | 2.88 | 4 | 0 | | yoda/runs.md:29 |
| yoda | r2-50 | r2 | 19.5 | see r2 | 2.330 | 2.54 | 4 | 0 | | yoda/runs.md:30 |
| yoda | r2-100 | r2 | 37.3 | see r2 | 2.257 | 2.75 | 3 | 0 | | yoda/runs.md:31 |
| yoda | r2-150 | r2 | 56.0 | see r2 | 2.392 | 2.92 | 3 | 0 | | yoda/runs.md:32 |
| yoda | r2-200 | r2 | 74.8 | see r2 | 2.499 | 2.96 | 4 | 0 | | yoda/runs.md:33 |
| yoda | r3-50 | r3 | 19.7 | see r3 | 2.330 | 2.54 | 4 | 0 | | yoda/runs.md:34 |
| yoda | r3-100 | r3 | 38.2 | see r3 | 2.257 | 2.75 | 3 | 0 | | yoda/runs.md:35 |
| yoda | r3-150 | r3 | 57.5 | see r3 | 2.392 | 2.92 | 3 | 0 | | yoda/runs.md:36 |
| yoda | r3-200 | r3 | 76.8 | see r3 | 2.499 | 2.96 | 4 | 0 | | yoda/runs.md:37 |
| yoda | r3-250 | r3 | 96.0 | see r3 | 2.999 | 2.88 | 4 | 0 | | yoda/runs.md:38 |
| yoda | r3-300 | r3 | 115.0 | see r3 | 3.865 | 3.08 | 2 | 0 | **best** (yoda/runs.md:16) | yoda/runs.md:39 |
| yoda | r3-350 | r3 | 133.9 | see r3 | 3.712 | 3.08 | 2 | 0 | | yoda/runs.md:40 |
| yoda | r3-400 | r3 | 152.9 | see r3 | 4.154 | 3.04 | 3 | 0 | | yoda/runs.md:41 |
| gandalf | base | - | 0 | - | - | 1.29 | 6 | 0 | | gandalf/runs.md:10 |
| gandalf | r1 | 200, 5e-6, 8/16, mask_prompt | 84.12 | not logged | 2.539 | 2.46 (r1-150) | 4 | 0 | | gandalf/runs.md:11 |
| gandalf | r2 | 400 (200 resumed + 200 new), 5e-6, 8/16 | 86.22 new (170.34 total) | 1231880192 B | 3.431 | 2.33 (r2-400) | 4 | 2 | | gandalf/runs.md:12 |
| gandalf | r3 | 200, 1e-5, 8/16 | 88.85 | 1228390400 B | 2.850 | 2.29 (r3-150, r3-200) | 6 | 1 | | gandalf/runs.md:13 |
| gandalf | r1-50 | r1 | 22.79 | not logged | 2.602 | 2.21 | 5 | 0 (gandalf/runs.md:35) | | gandalf/runs.md:20 |
| gandalf | r1-100 | r1 | 42.66 | not logged | 2.527 | 2.17 | 5 | 0 | | gandalf/runs.md:21 |
| gandalf | r1-150 | r1 | 63.30 | not logged | 2.492 | 2.46 | 4 | 0 | **best** (gandalf/runs.md:15) | gandalf/runs.md:22 |
| gandalf | r1-200 | r1 | 84.12 | not logged | 2.539 | 2.38 | 5 | 0 | | gandalf/runs.md:23 |
| gandalf | r2-250 | r2 | 84.12 + 22.51 = 106.63 | see r2 | 2.625 | 2.08 | 7 | 0 | | gandalf/runs.md:24 |
| gandalf | r2-300 | r2 | 84.12 + 43.29 = 127.41 | see r2 | 2.848 | 2.21 | 5 | 0 | | gandalf/runs.md:25 |
| gandalf | r2-350 | r2 | 84.12 + 64.59 = 148.71 | see r2 | 3.129 | 2.25 | 6 | 0 | | gandalf/runs.md:26 |
| gandalf | r2-400 | r2 | 84.12 + 86.22 = 170.34 | see r2 | 3.431 | 2.33 | 4 | 2 | | gandalf/runs.md:27 |
| gandalf | r3-50 | r3 | 22.65 | see r3 | 2.535 | 2.12 | 6 | 0 | | gandalf/runs.md:28 |
| gandalf | r3-100 | r3 | 44.55 | see r3 | 2.546 | 2.12 | 6 | 0 | | gandalf/runs.md:29 |
| gandalf | r3-150 | r3 | 66.59 | see r3 | 2.520 | 2.29 | 6 | 0 | | gandalf/runs.md:30 |
| gandalf | r3-200 | r3 | 88.77 | see r3 | 2.850 | 2.29 | 6 | 1 | | gandalf/runs.md:31 |
| tony-stark | base | - | 0 | - | - | 1.16 | 6 | 0 | | tony-stark/runs.md:10 |
| tony-stark | r1 | 200, 5e-6, 8/16, mask_prompt, seed 11 | 73.26 | 1197572096 B | 2.902 | 2.60 | 7 | 0 | | tony-stark/runs.md:11 |
| tony-stark | r2 | 200, 5e-6, 8/16, seed 22, +12 fact lines | 74.80 | 1193099264 B | 2.768 | 2.72 | 6 | 0 | | tony-stark/runs.md:12 |
| tony-stark | r3 | 200, 5e-6, 16/16, seed 33 | 76.33 | 1262452736 B | 2.926 | 2.80 | 6 | 0 | | tony-stark/runs.md:13 |
| tony-stark | r1-50 | r1 | 19.68 | see r1 | 2.772 | 2.16 | 6 | 0 (tony-stark/runs.md:38) | | tony-stark/runs.md:21 |
| tony-stark | r1-100 | r1 | 36.97 | see r1 | 2.688 | 2.40 | 7 | 0 | | tony-stark/runs.md:22 |
| tony-stark | r1-150 | r1 | 55.02 | see r1 | 2.715 | 2.32 | 8 | 0 | | tony-stark/runs.md:23 |
| tony-stark | r1-200 | r1 | 73.16 | see r1 | 2.902 | 2.60 | 7 | 0 | | tony-stark/runs.md:24 |
| tony-stark | r2-50 | r2 | 19.72 | see r2 | 2.765 | 2.16 | 7 | 0 | | tony-stark/runs.md:25 |
| tony-stark | r2-100 | r2 | 37.29 | see r2 | 2.726 | 2.20 | 8 | 0 | | tony-stark/runs.md:26 |
| tony-stark | r2-150 | r2 | 55.99 | see r2 | 2.755 | 2.04 | 7 | 0 | | tony-stark/runs.md:27 |
| tony-stark | r2-200 | r2 | 74.69 | see r2 | 2.768 | 2.72 | 6 | 0 | | tony-stark/runs.md:28 |
| tony-stark | r3-50 | r3 | 19.98 | see r3 | 2.725 | 1.92 | 7 | 0 | | tony-stark/runs.md:29 |
| tony-stark | r3-100 | r3 | 38.49 | see r3 | 2.702 | 2.28 | 6 | 0 | | tony-stark/runs.md:30 |
| tony-stark | r3-150 | r3 | 57.46 | see r3 | 3.012 | 2.72 | 7 | 0 | | tony-stark/runs.md:31 |
| tony-stark | r3-200 | r3 | 76.21 | see r3 | 2.926 | 2.80 | 6 | 0 | **best** (tony-stark/runs.md:15) | tony-stark/runs.md:32 |

Values that differ from the brief or within a file, kept as written:
- jack-sparrow r1 train s is 293.92 in the file and 293.9 in the brief.
- sherlock-holmes r3 is 85.9 s in the run row (:12) and 85.8 s in the checkpoint row (:30).
- gandalf r3 is 88.85 s in the run row (:13) and 88.77 s in the checkpoint row (:31).
- yoda r1-150 is 55.1 s in the file and 55 s in the brief.
- None of the five meets the 4/5 bar.

## What each build learned and changed for the next
- jack-sparrow: do not pick by val loss (jack-sparrow/runs.md:25); make everyday questions half the data (:26); give 6-10 examples per Never rule (:27); use temp 0.3 with a fixed seed, a repetition penalty and a system line under 80 words (:29); memory is not the limit (:34).
- sherlock-holmes: the base is the ceiling (sherlock-holmes/runs.md:41); write 4-6 phrasings per fact (:42) and give greetings several answers (:44); start at 5e-6 with mask_prompt for 200 iters, saving every 50 (:47); timestamp every log line (:48).
- yoda: val loss ran opposite to voice (yoda/runs.md:55); put the fact first in every phrasing (:56); give 2-3 answers per greeting family and sum (:57) and explicit N-bullet Never examples (:58); resume rather than retrain with the same seed (:61); deduplicate the blind batch (:62).
- gandalf: facts did not take, even fact-first (gandalf/runs.md:41); more iters and lr 1e-5 both scored lower (:42); late checkpoints start to memorise (:44); the best came at 63.30 s, about 150 iters at 5e-6 (:48).
- tony-stark: data (+12 fact lines) and rank 16 each added a little (tony-stark/runs.md:44); facts still failed in all 13 sets (:45) and gap dodges did not hold (:46); greeting variety gave 0 of 13 verbatim (:48); the last checkpoint was best in every run, so resuming r3 is the next test (:51).
