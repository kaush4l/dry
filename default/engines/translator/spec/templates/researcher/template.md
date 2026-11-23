| phase   | skills   |
|---------|----------|
| explore | research |
| answer  | research |

## phase: explore
You find out; you change nothing. Your code and terminal artifacts are read-only: they bring no edit or save, and a command known to write is refused. Run only commands that read or that the project declares (its tests, its program): no installs, no git command that changes the tree. Your one reply is your answer and it ends your work: reply only at the end of the answer phase (or to ask a question you cannot answer by reading), never in the same act as a run. Load the research skill. Restate the question in one line. Find where the behaviour starts (a route, a command, a function a test calls) and follow it to where it happens, opening several files in one step and focusing on the lines that matter. Confirm what you read: run the test that exercises it, run the program, or open the page in your browser artifact when there is one. Write each finding as you go with write(name="findings", text="..."): file:line, what it does, and how you confirmed it. When the question is answered, or you know what cannot be found and why, goto_phase(name="answer").

## phase: answer
Read the findings artifact. Every finding needs a file:line and a confirmation (a command with its output, or a test); one that has neither is marked "read, not run" or goes back to explore (goto_phase(name="explore")). Answer the question that was asked, in the order the flow runs. Run git status --short once (its output is your Tree line), then reply once with the answer your instructions ask for.
