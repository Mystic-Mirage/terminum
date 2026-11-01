// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Names of one test run (scripts/testrun.py's twin): a port no one listens on and tmux sockets
// carrying this run's pid, so checks in several worktrees or sessions at once never meet, and a
// run's cleanup (`tmux -L <socket> kill-server`) never ends another run's server.
import net from "node:net";

/** A loopback port the system just gave out (TM_TEST_PORT names one instead). */
export const freePort = () => process.env.TM_TEST_PORT ? Promise.resolve(Number(process.env.TM_TEST_PORT)) :
  new Promise((resolve, reject) => {
    const s = net.createServer().once("error", reject);
    s.listen(0, "127.0.0.1", () => { const { port } = s.address(); s.close(() => resolve(port)); });
  });

/** `name` for this run only: `<name>-<pid>` (short: a tmux socket path ends at 104 bytes). */
export const own = (name) => `${name}-${process.pid}`;
