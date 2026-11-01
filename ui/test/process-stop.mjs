// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Ending what a test started, a signal ending the run too.
import os from "node:os";

// Stop only the child the harness owns, and wait before deleting its state or tmux.
export async function stopChild(child, grace = 5000) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  let timer, forced, ended, failed;
  const exit = new Promise((resolve, reject) => {
    ended = resolve; failed = reject;
    child.once("exit", ended); child.once("error", failed);
    timer = setTimeout(() => {
      child.kill("SIGKILL");
      forced = setTimeout(() => reject(Error("The private test daemon did not exit after SIGKILL")), 2000);
    }, grace);
    child.kill("SIGTERM");
  });
  try { await exit; }
  finally {
    clearTimeout(timer); clearTimeout(forced);
    child.off("exit", ended); child.off("error", failed);
  }
}

/** Playwright's launch options that leave SIGINT, SIGTERM and SIGHUP to `endOnSignal`: its own
 *  SIGINT handler exits once the browsers close, before a teardown ends. Its exit handler still
 *  kills the browsers. */
export const NO_SIGNALS = { handleSIGINT: false, handleSIGTERM: false, handleSIGHUP: false };

/** A run ended by SIGINT, SIGTERM or SIGHUP ends what it started too: Node's default for them
 *  exits at once, skipping every `finally`, and a test's terminumd and tmux server outlive it.
 *  `teardown` runs once, from the signal or from the `finally` that calls the returned function;
 *  after a signal the run exits as the signal would (128 + its number), and a second one ends
 *  it at once. */
export function endOnSignal(teardown) {
  let ran = null;
  const once = () => (ran ??= Promise.resolve().then(teardown));
  for (const sig of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.once(sig, () => {
      console.error(`${sig}: ending what this run started`);
      once().catch((e) => console.error(e)).finally(() => process.exit(128 + os.constants.signals[sig]));
    });
  }
  return once;
}
