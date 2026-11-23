---
name: debugging
description: When something fails and the cause is not obvious - build a fast loop that shows the failure, then test one hypothesis at a time; never guess-and-edit.
---
# Debugging: a loop first, then hypotheses

1. **A loop that shows the bug.** Before any fix, find one command that
   fails because of the bug, every time, fast, with no person in it: a
   failing test, a `run` of the program with the bad input, a `curl`
   against the dev server. Run it and see it fail. If you have this, the
   bug is mostly found. If you cannot make one, that is a finding for the
   notes.
2. **Make it small.** Cut the input and the steps down until every part
   left is needed to show the failure.
3. **Hypotheses, ranked.** Write three or fewer causes in the notes, each
   one something a single run can prove wrong.
4. **One change at a time.** Test the top hypothesis: add one print or
   assertion, or read one value, then rerun the loop. Remove what you
   added when you are done.
5. **Fix the cause, not the symptom.** The loop now passes, and the test
   that guards it is the one the plan names. Rerun everything else.
6. **Same failure twice.** If a fix leaves the same failure, stop editing.
   Go back to triage with the notes: the loop, what you tried, what it
   showed.

Ideas from Matt Pocock's `diagnosing-bugs` (the feedback loop as the gate,
falsifiable hypotheses) and gstack's `qa` stop rules, both MIT; written for
dry.
