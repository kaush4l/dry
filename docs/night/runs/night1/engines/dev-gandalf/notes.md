## Notes
- r2 (resumed r1, seed 22): 86.33 s real, peak RSS 1231880192 B. r2.log at 1.38 s reads 'Loading fine-tuned weights from adapters/r1/adapters.safetensors'; val 2.540 at step 1 (=r1 final 2.539), then 2.625/2.848/3.129/3.431 at total 250/300/350/400.
- r3 changes lr only (5e-6 -> 1e-5) against r1, seed 33. The fact batch was never written, so there was no data change to test.
- r3: 88.85 s, peak RSS 1228390400 B, val 2.535/2.546/2.520/2.850.
- r1 saves at 22.79/42.66/63.30/84.12 s; no peak-memory line in r1 log.