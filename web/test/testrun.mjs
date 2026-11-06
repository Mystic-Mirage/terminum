// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Names of one test run (scripts/testrun.py's twin): a port no one listens on and tmux sockets
// carrying this run's pid, so checks in several worktrees or sessions at once never meet, and a
// run's cleanup (`tmux -L <socket> kill-server`) never ends another run's server. Where its folders
// go, and the socket files `kill-server` leaves behind, removed when the run exits (spec AC-102).
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";

/** A loopback port the system just gave out (TM_TEST_PORT names one instead). */
export const freePort = () => process.env.TM_TEST_PORT ? Promise.resolve(Number(process.env.TM_TEST_PORT)) :
  new Promise((resolve, reject) => {
    const s = net.createServer().once("error", reject);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => resolve(port)); });
  });

/** Where a test's folders go: the temporary folder if paths of sockets in it fit in 104 bytes
 *  (check_step.py's short one, Linux's /tmp), else /tmp: a Mac's own is 57 bytes once resolved. */
export const shortTmp = () => {
  const folder = fs.realpathSync(os.tmpdir());
  return folder.length <= 40 ? folder : "/tmp";
};

const sockets = new Set();
const nap = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
/** The socket file of each `own` name whose tmux server has gone (a killed one may take a moment
 *  to); a live one stays and is seen. */
const removeDeadSockets = () => {
  for (const name of sockets) {
    for (let i = 0; i < 20; i++) {
      if (spawnSync("tmux", ["-L", name, "list-sessions"], { stdio: "ignore" }).status !== 0) {
        fs.rmSync(path.join(process.env.TMUX_TMPDIR || "/tmp", `tmux-${process.getuid()}`, name), { force: true });
        break;
      }
      nap(50);
    }
  }
};

/** `name` for this run only: `<name>-<pid>` (short: a tmux socket path ends at 104 bytes). */
export const own = (name) => {
  if (!sockets.size) process.on("exit", removeDeadSockets);
  sockets.add(`${name}-${process.pid}`);
  return `${name}-${process.pid}`;
};
