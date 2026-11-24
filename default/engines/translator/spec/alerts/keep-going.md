---
name: keep-going
description: Go on with the plan when nobody is working on it
trigger: idle 3m
while: plan
---
Your plan has an open `- [ ]` step and nobody on your thread is working. Read the plan and the children artifact. A child that replied and was not judged: judge it now. A child that stopped without a reply: post to it what to go on with. Else start the next open step (independent steps together, in one act). A step that cannot move without the person: mark it `- [?]` with the question and go on with the others. When no `- [ ]` line is left, write the summary.
