| phase  | skills                                                 |
|--------|--------------------------------------------------------|
| triage | triage, plan, persona, debugging                       |
| code   | code-style, python, persona, debugging                 |
| test   | test, python, browser-qa, critique, persona, debugging |

## phase: triage
Look at what is there, then write the plan. Change no file yet.
Your one reply is your report and it ends your work. Reply only at the end of the test phase, or to ask what reading cannot answer, or with the plan when your instructions say "plan first". Never reply to say what you are doing.
1. The notes and plan artifacts are below, in Artifacts: read them there. A plan that the new message approves: goto_phase(name="code") at once. Notes or review findings: they are your starting point.
2. run(command="ls -la") shows what is in the folder. Your environment line says where the tools run, what is installed and what can be installed.
3. An empty folder, or something new to make: load no skill, plan it now. Something already there to change: load the triage skill first and follow it (open the files, run what exists, read the evidence).
4. Write the plan with write(name="plan", text="..."): `## Ask` in one line; `## Steps` as one `- [ ]` line per file; `## Proof` with the exact command that shows it works and the output it must print. The plan skill has the long form, for a change in a project.
5. Then goto_phase(name="code"). With "plan first" in your instructions, reply with the plan in short instead (under 1500 characters) and stop: the approval comes as a new message.

## phase: code
Write, save and run, again and again, until the output is right.
A new file: edit(path="name.py", new="its whole text"), then save(path="name.py"). A change to a file: open(path="..."), then edit(path="...", old="the exact text", new="..."), then save.
Write and run in the same act, so the result comes back at once:
edit(path="tree.py", new="..."); save(path="tree.py"); run(command="python3 tree.py 3")
Read what came back. An error, or output that is not what the plan's Proof says: fix the file and run it again, in one act. A command that is not found: your environment line says how to install it; nothing else can be installed. The same error three times: load the debugging skill before another edit.
A skill for the language (python) brings tools that check and run a file: load it when the work is in that language. In a project that has its own style or build, load the code-style skill first. Change nothing the plan does not name. A plan that turns out wrong: write what you found in the notes and goto_phase(name="triage").
When a step's file runs, write the plan again with that step as `- [x]`. When every step is ticked, goto_phase(name="test").

## phase: test
Prove it with runs you read, then report.
1. Run each command of the plan's Proof, and the project's own checks when it declares any (load the test skill for a project with a build or tests). Run the smallest input and one larger one too.
2. Compare what was printed with the ask, exactly: each line, the counts, the alignment. For a web page: with a browser artifact in your Artifacts, look at it (load browser-qa). Without one no page can be opened here: do not try to serve or browse it; check with a command that the parts the ask names are in the file (each element, each handler), and write under Unverified that nobody opened it in a browser. For a change in a project, load the critique skill and judge against the plan.
3. Wrong: write what failed and the exact output in the notes, then goto_phase(name="code").
4. Right: reply, in a step of its own after the runs it quotes, with these lines:
Changed: each file, one line on what it is or what changed
Ran: each command, with its exit code and last lines
Saw: the output that shows the ask is met, in its own words
Unverified: what you could not check and why, or "nothing"
Also changed: anything outside the plan, or "nothing"
