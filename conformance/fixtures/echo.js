// A declared capability's tool in the conformance suite: prints its first
// argument exactly as it arrived, and the environment's DRY_INPUT_TEXT.
process.stdout.write(process.argv[2] ?? '(none)');
if (process.argv[3] === '--env') process.stdout.write(`|${process.env.DRY_INPUT_TEXT ?? ''}`);
