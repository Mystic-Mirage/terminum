// The web app's output widgets (AC-56): which parts of a terminal's output become code,
// Markdown, a Mermaid diagram or a table.
import { test } from "node:test";
import assert from "node:assert/strict";
import { logicalLines, pipeTable, regions, scan } from "../static/outparse.js";
import { insideBox, tableRule } from "../static/drawntable.js";

// One row per line, every one with a hard end.
const rows = (text) => text.split("\n").map((t) => ({ text: t, eol: true }));
const found = (text, end) => regions(rows(text), end).map(({ kind, from, to, lang }) => ({ kind, from, to, lang }));
// one output line, its styling given or plain
const L = (text, o = {}) => ({ text, eol: true, styled: false, ...o });

const README = `alex@devbox ~/app % cat README.md
# App

A small tool.

## Install

\`\`\`sh
$ npm install
\`\`\`

- fast
- small
alex@devbox ~/app % `;

test("a file printed by cat is its kind, up to the next prompt (a '$ ' line inside it is not one)", () => {
  assert.deepEqual(found(README), [{ kind: "markdown", from: 1, to: 12, lang: "markdown" }]);
  const py = "alex@devbox ~ % bat -p src/tool.py\nimport os\n\ndef main():\n    return 1\nalex@devbox ~ % ";
  assert.deepEqual(found(py), [{ kind: "code", from: 1, to: 4, lang: "tool.py" }]);
  const mmd = "alex@devbox ~ % cat flow.mmd\ngraph TD\n  A --> B\nalex@devbox ~ % ";
  assert.deepEqual(found(mmd), [{ kind: "mermaid", from: 1, to: 2, lang: "mermaid" }]);
});

test("the next prompt may be the cursor's row, which is not output", () => {
  const text = "alex@devbox ~ % cat a.json\n{\n  \"a\": 1\n}\nalex@devbox ~ % ";
  assert.deepEqual(found(text, 4), [{ kind: "code", from: 1, to: 3, lang: "a.json" }]);
});

test("a fenced block: code with its language, mermaid as a diagram; still open: nothing yet", () => {
  assert.deepEqual(found("Here:\n```python\nprint(1)\n```\ndone"), [{ kind: "code", from: 1, to: 3, lang: "python" }]);
  assert.deepEqual(found("~~~mermaid\nsequenceDiagram\n  A->>B: hi\n~~~"), [{ kind: "mermaid", from: 0, to: 3, lang: "mermaid" }]);
  assert.deepEqual(found("```js\nlet a = 1\n"), []);
  const r = regions(rows("  ```js\n  if (a) {\n    b()\n  }\n  ```"))[0];
  assert.equal(r.text, "if (a) {\n  b()\n}", "the fence's indentation is taken off");
});

test("a Markdown answer: headings, lists and fences in one widget", () => {
  const text = "Sure.\n\n## Plan\n\n1. Read the **config**\n2. Write `out.json`\n\n```sh\nmake\n```\n\nThat is all.\n$ ";
  assert.deepEqual(found(text), [{ kind: "markdown", from: 2, to: 11, lang: "markdown" }]);
});

test("not Markdown: a program's comments, YAML, git log --graph, a lone list", () => {
  const python = "# Load the config\nimport os\nx = os.environ.get('A')\n# Then run\ndef run():\n    return x\n# - not a list";
  const yaml = "# settings\nname: app\nitems:\n  - a\n  - b\n# end";
  const shell = "# install\nmkdir -p \"$dir\"\n# copy\ncp a b && echo ok\n# done\n- item";
  const graph = "* 1a2b3c fix\n* 4d5e6f feat\n* 7a8b9c docs";
  for (const text of [python, yaml, shell, graph]) assert.deepEqual(found(text), [], text);
});

test("a diagram printed without a fence, after a blank line, up to the next blank line", () => {
  assert.deepEqual(found("\nflowchart LR\n  A --> B\n  B --> C\n\nok"), [{ kind: "mermaid", from: 1, to: 3, lang: "mermaid" }]);
  assert.deepEqual(found("draw a graph TD please"), []);
});

test("rows iTerm wrapped are one line; region rows are rows", () => {
  const wrapped = [{ text: "```js", eol: true }, { text: "const long = ", eol: false }, { text: "1;", eol: true }, { text: "```", eol: true }];
  assert.deepEqual(logicalLines(wrapped).map((l) => l.text), ["```js", "const long = 1;", "```"]);
  assert.deepEqual(regions(wrapped).map(({ from, to, text }) => ({ from, to, text })), [{ from: 0, to: 3, text: "const long = 1;" }]);
});

test("output a program colored itself is rendered already: bat, glow, a coding agent", () => {
  const styled = rows("```python\nprint(1)\n```").map((r, i) => ({ ...r, styled: i === 1 }));
  assert.deepEqual(regions(styled), []);
});

test("more that is not a widget: GitHub Actions YAML, prose about pies, an encoding line", () => {
  const actions = "name: ci\non: push\n\n# build it\njobs:\n  build:\n    steps:\n      - uses: actions/checkout@v4\n      - run: make";
  const pie = "\npie chart of the sales\nfor each region";
  const enc = "\n# -*- coding: utf-8 -*-\n\n- a\n- **b**";
  for (const text of [actions, pie, enc]) assert.deepEqual(found(text), [], text);
});

test("settled: where scanning again finds the same regions; an open fence or file waits", () => {
  const done = "```js\na()\n```\n" + "text\n".repeat(20);
  const s = scan(rows(done));
  assert.ok(s.settled > 3 && s.settled <= 24, String(s.settled));
  // the last lines always wait (a document may go on); an open fence or file further back too
  assert.equal(scan(rows("x\n".repeat(40) + "```js\n" + "a()\n".repeat(30))).settled, 40, "an open fence");
  assert.equal(scan(rows("x\n".repeat(40) + "alex@devbox ~ % cat a.py\n" + "print(1)\n".repeat(30))).settled, 40, "a file whose prompt has not come back");
  const tail = rows(done + "## Next\n\n- a\n- b");
  const all = regions(tail), from = scan(tail).settled;
  assert.deepEqual(regions(tail.slice(from)).map((r) => r.from + from), all.filter((r) => r.from >= from).map((r) => r.from));
});

test("10000 lines of a list take little time (no rescan per line)", () => {
  const text = Array.from({ length: 10000 }, (_, i) => `- item ${i}`).join("\n");
  const t0 = performance.now();
  assert.deepEqual(found(text), []);
  assert.ok(performance.now() - t0 < 500, `${performance.now() - t0} ms`);
});

test("settled never stays far behind: a log with a list item on every line, a file whose prompt never came back", () => {
  const log = rows(Array.from({ length: 3000 }, (_, i) => `- step ${i} done`).join("\n"));
  assert.ok(scan(log).settled >= 3000 - 2000, String(scan(log).settled));
  const tail = rows("alex@devbox ~ % tail -f app.log\n" + Array.from({ length: 3000 }, (_, i) => `line ${i}`).join("\n"));
  assert.ok(scan(tail).settled >= 3001 - 2000, String(scan(tail).settled));
});

test("Starship's prompt: a colored line, then a clock and ❯; the next one a minute later", () => {
  const lines = [["", false], ["~/app via 🦀 v1.98", true], ["at 12:21 ❯ cat guide.md", false], ["# Guide", false], ["", false],
    ["- one", false], ["- **two**", false], ["", false], ["~/app via 🦀 v1.98", true], ["at 12:22 ❯ cat flow.mmd", false],
    ["flowchart LR", false], ["  A --> B", false], ["", false], ["~/app via 🦀 v1.98", true], ["at 12:22 ❯ sh answer.sh", false],
    ["## Plan", false], ["", false], ["1. Read the **config**", false], ["2. Run `make`", false], ["", false], ["That is all.", false],
    ["", false], ["~/app via 🦀 v1.98", true]].map(([text, styled]) => ({ text, eol: true, styled }));
  lines.splice(15, 0, { text: "", eol: true, styled: false });   // a blank line before the heading
  const got = regions(lines, lines.length).map(({ kind, from, to }) => ({ kind, from, to }));
  assert.deepEqual(got, [{ kind: "markdown", from: 3, to: 6 }, { kind: "mermaid", from: 10, to: 11 }, { kind: "markdown", from: 16, to: 21 }]);
});

test("a document a coding agent's tool printed, indented: found, and its text without the indentation", () => {
  const out = ["  ⎿  # Releasing", "", "     How releases are signed, see [CLAUDE.md](../CLAUDE.md).", "",
    "     ## Signed releases", "", "     Installs check `SHA256SUMS`:", "", "     ```bash", "     make signing-key", "     ```", "",
    "     - one", "     - two", "", "⏺ Done."];
  const r = regions(rows(out.join("\n")));
  assert.equal(r.length, 1);
  assert.equal(r[0].kind, "markdown");
  assert.equal(r[0].from, 0, "from the tool's first line, its mark read as indentation");
  assert.ok(r[0].text.startsWith("# Releasing\n"), r[0].text.slice(0, 40));
  assert.ok(!/^ {4}/m.test(r[0].text.split("\n").filter((t) => !t.startsWith("make")).join("\n")), "no line indented 4 spaces: Markdown would take it for code");
});

test("a shell script's comment block a coding agent's tool printed is not Markdown, a document with headings still is", () => {
  const script = [
    "⏺ Read(scripts/build.sh)", "  ⎿  Read 80 lines", "     fi", "",
    "     # Bundle id is an explicit caller input, never auto-applied here.", "     `build.sh`",
    "     # passes the worktree-isolated id for its Debug macOS run (`run.sh`'s", "     app);",
    "     # the qa/iq-robot UI-test host deliberately leaves it unset so it", "     keeps the",
    "     # project's own id and does NOT collide in LaunchServices with a live", "     `run.sh`",
    "     # instance (XCUITest needs to own the app it launches). deploy/Release", "     leave it",
    "     # to the project's distribution/provisioning settings.", '     BUNDLE_ID="${IQ_XCB_BUNDLE_ID:-}"', "",
    "⏺ Done.",
  ].join("\n");
  assert.deepEqual(found(script).filter((r) => r.kind === "markdown"), []);
  const doc = "# Notes\n\nThe `build.sh` script.\n\n## Usage\n\n- run it\n- check `out/`\n# not a comment, a heading after text\n";
  assert.equal(found(doc)[0]?.kind, "markdown");
  const changelog = "## 1.3.0\n- fix `a`\n## 1.2.0\n- fix `b`\n## 1.1.0\n- add `c`\n";
  assert.equal(found(changelog)[0]?.kind, "markdown", "a compact changelog");
  const readme = "# App\nA small `tool`.\n## Install\n- run `make`\n- done\n";
  assert.equal(found(readme)[0]?.kind, "markdown", "a heading right after a line of text");
});

test("a document a tool printed in slices (sed -n) is one widget from the output's first line, after a ● mark", () => {
  const out = [
    "● Bash(sed -n '1p;23p;232,238p' docs/plan.md)",
    "  ⎿  # Audio capture; import for administrators only",
    "     | O4 | The server's import stays for administrators only: a sign-in whose checked token carries `hd`;",
    "     enforced on the server; nobody else ever gets it (`example.com` has no mail yet; storing `hd`",
    "     is a new account field, so the Privacy Policy changes first) |",
    "     | Terms §1, §4 | add: the Service can record audio playing in another tab; a file the user imports is",
    "     converted on our server and deleted when the conversion ends; the user records only what they own;",
    "     shared cache on the public host) |",
    "     | EULA §3 | `import` becomes `import or record` in the list; a new bullet: recording may be forbidden by",
    "     that platform's terms, which the user follows (no platform named) |",
    "",
    "     ## 2.10 Import for administrators, enforced on the server (O4)",
    "",
    "     | Piece | Design |",
    "     | --- | --- |",
    "     | Who | an account whose checked token carried `hd` |",
    "",
    "● Done.",
  ];
  const r = regions(rows(out.join("\n")));
  assert.deepEqual(r.map(({ kind, from, to }) => ({ kind, from, to })), [{ kind: "markdown", from: 1, to: 15 }]);
  assert.ok(r[0].text.startsWith("# Audio capture;"), r[0].text.slice(0, 40));
  // a tool output whose first line is no document leaves the rest to be looked at as before
  const plain = ["⏺ Bash(cat notes.md)", "  ⎿  Read 3 lines", "", "     # Notes", "", "     - one `a`", "     - two `b`", "", "⏺ Done."];
  assert.deepEqual(found(plain.join("\n")).map((x) => x.kind), ["markdown"]);
});

// A table Claude Code drew: rules between its rows, a cell wrapped over two lines, its code colored.
const CLAUDE_TABLE = [
  "⏺ The commits:", "",
  "  ┌─────────┬───────────────────────────┐",
  "  │ Commit  │           What            │",
  "  ├─────────┼───────────────────────────┤",
  "  │ eecb6df │ Web access: port 48080 by │",
  "  │         │ default | a README section│",
  "  ├─────────┼───────────────────────────┤",
  "  │ 7962f12 │ Host actions              │",
  "  └─────────┴───────────────────────────┘", "", "  Done."];

test("a drawn table: its head, a row per part between rules, a wrapped cell one cell; colored, it is one still", () => {
  const colored = rows(CLAUDE_TABLE.join("\n")).map((r) => ({ ...r, styled: /eecb|7962/.test(r.text) }));
  const r = regions(colored);
  assert.deepEqual(r.map(({ kind, from, to }) => ({ kind, from, to })), [{ kind: "table", from: 2, to: 9 }]);
  assert.deepEqual(r[0].table.head, ["Commit", "What"]);
  assert.deepEqual(r[0].table.rows, [["eecb6df", "Web access: port 48080 by default | a README section"], ["7962f12", "Host actions"]]);
  assert.equal(r[0].text, "| Commit | What |\n| --- | --- |\n| eecb6df | Web access: port 48080 by default \\| a README section |\n| 7962f12 | Host actions |",
    "Copy gives Markdown, a | in a cell escaped");
});

test("drawn tables of other programs: mysql's +---+ (a column of numbers to the right), rich's heavy head, rows without rules", () => {
  const mysql = "mysql> select id, name from t;\n+----+-------+\n| id | name  |\n+----+-------+\n|  1 | a     |\n| 22 | b     |\n+----+-------+\n2 rows in set";
  const m = regions(rows(mysql));
  assert.deepEqual(m.map(({ kind, from, to }) => ({ kind, from, to })), [{ kind: "table", from: 1, to: 6 }]);
  assert.deepEqual([m[0].table.head, m[0].table.rows, m[0].table.align], [["id", "name"], [["1", "a"], ["22", "b"]], ["right", ""]]);
  const rich = "┏━━━━━━┳━━━━━━━━━━┓\n┃ Name ┃ Note     ┃\n┡━━━━━━╇━━━━━━━━━━┩\n│ api  │ a long   │\n│      │ note     │\n│ web  │ short    │\n└──────┴──────────┘";
  const t = regions(rows(rich))[0]?.table;
  assert.deepEqual(t && [t.head, t.rows], [["Name", "Note"], [["api", "a long note"], ["web", "short"]]], "a line with an empty first cell goes on the row above");
  const bare = "╭───┬───╮\n│ a │ b │\n│ c │ d │\n╰───┴───╯";
  assert.deepEqual(regions(rows(bare))[0]?.table.head, null, "no rule after the first part: no head");
});

test("an ASCII table's row with an empty first value is a row of its own; numbers with units or none ('-') align right", () => {
  const t = regions(rows("+------+------+\n| name | size |\n+------+------+\n| Ann  | 1 KB |\n|      | 512 B|\n| Bob  | -    |\n+------+------+"))[0]?.table;
  assert.deepEqual(t && [t.rows, t.align], [[["Ann", "1 KB"], ["", "512 B"], ["Bob", "-"]], ["", "right"]]);
});

test("drawn tables in a coding agent's tool output, and with double rules (tabulate's fancy_grid, a double frame)", () => {
  const tool = ["⏺ Bash(python3 report.py)", "  ⎿  ┌──────┬─────┐", "     │ name │ n   │", "     ├──────┼─────┤", "     │ api  │ 3   │", "     └──────┴─────┘", "⏺ Done."];
  const r = regions(rows(tool.join("\n")));
  assert.deepEqual(r.map(({ kind, from, to }) => ({ kind, from, to })), [{ kind: "table", from: 1, to: 5 }]);
  assert.deepEqual([r[0].table.head, r[0].table.rows], [["name", "n"], [["api", "3"]]]);
  const fancy = "╒══════╤═════╕\n│ name │ n   │\n╞══════╪═════╡\n│ api  │ 3   │\n├──────┼─────┤\n│ web  │ 4   │\n╘══════╧═════╛";
  assert.deepEqual(regions(rows(fancy))[0]?.table.rows, [["api", "3"], ["web", "4"]]);
  const double = "╔══════╦═════╗\n║ name ║ n   ║\n╠══════╬═════╣\n║ api  ║ 3   ║\n╚══════╩═════╝";
  assert.deepEqual(regions(rows(double))[0]?.table.head, ["name", "n"]);
});

// Claude Code's table whose top rule is not there (copied without it, or scrolled away): it starts at its head row.
const HEADLESS = [
  "  Turn the classifier off?", "",
  "  │                 What                 │ Without │ With │",
  "  ├──────────────────────────────────────┼─────────┼──────┤",
  "  │ Typing while speaking (30 takes)     │ 2       │ 23   │",
  "  ├──────────────────────────────────────┼─────────┼──────┤",
  "  │ Music under speech, 10 dB below (6)  │ 0       │ 6    │",
  "  └──────────────────────────────────────┴─────────┴──────┘", "", "  Done."];

test("a drawn table without its top rule starts at its head row, when a middle rule follows it", () => {
  const r = regions(rows(HEADLESS.join("\n")));
  assert.deepEqual(r.filter((x) => x.kind === "table").map(({ from, to }) => ({ from, to })), [{ from: 2, to: 7 }]);
  const t = r.find((x) => x.kind === "table").table;
  assert.deepEqual([t.head, t.rows, t.align], [["What", "Without", "With"],
    [["Typing while speaking (30 takes)", "2", "23"], ["Music under speech, 10 dB below (6)", "0", "6"]], ["", "right", "right"]]);
  // rows and a bottom rule: the rule shows the grid; rows with no rule at all are text
  assert.deepEqual(regions(rows("  │ a │ b │\n  │ c │ d │\n  └───┴───┘"))[0]?.table.rows, [["a", "b"], ["c", "d"]]);
  assert.equal(regions(rows("  │ a │ b │\n  │ c │ d │\n  done")).filter((x) => x.kind === "table").length, 0);
  // the rule under it must have as many columns
  assert.equal(regions(rows("  │ a │ b │ c │\n  ├───┼───┤\n  │ d │ e │\n  └───┴───┘")).filter((x) => x.kind === "table").length, 0);
});

test("not a drawn table: an agent's box of one column, a line off the grid, a table not finished yet", () => {
  const box = "╭──────────────────╮\n│ ✻ Welcome!       │\n│   cwd: /tmp      │\n╰──────────────────╯";
  const broken = "┌───┬───┐\n│ a │ b │\n│ c   d │\n└───┴───┘";
  const columns = "┌───┬───┐\n│ a │ b │ c │\n└───┴───┘";
  for (const text of [box, broken, columns]) assert.deepEqual(found(text), [], text);
  const open = "┌───┬───┐\n│ a │ b │\n├───┼───┤\n" + "│ c │ d │\n".repeat(30).trimEnd();
  assert.deepEqual(found(open), []);
  assert.equal(scan(rows("x\n".repeat(30) + open)).settled, 30, "it waits for its last rule");
});

test("a drawn table caught half-drawn waits for the rest: a row cut midway, blank rows under it; never settled past its top", () => {
  // longer than the lines a scan keeps open at the end anyway
  const body = Array.from({ length: 8 }, (_, i) => [`  │ row ${i}    │ value ${i}                   │`, "  ├─────────┼───────────────────────────┤"]).flat();
  const table = [...CLAUDE_TABLE.slice(2, 5), ...body, ...CLAUDE_TABLE.slice(8, 10)], cut = table.at(-2).slice(0, 14);
  const lead = "x\n".repeat(30);
  for (const [why, text] of [["a row cut midway", [...table.slice(0, -2), cut]], ["blank rows under it", [...table.slice(0, -3), "", "", ""]]]) {
    const s = scan(rows(lead + text.join("\n")));
    assert.equal(s.found.length, 0, why);
    assert.ok(s.settled <= 30, `${why}: its top rule is not settled as text (${s.settled})`);
  }
  assert.deepEqual(found(lead + table.join("\n")).map(({ kind, from }) => [kind, from]), [["table", 30]], "drawn whole, it is one");
  assert.deepEqual(found("┌───┬───┐\n│ a │ b │\n│ c   d │\n\n  Done."), [], "a line off the grid with text after it: no table");
});

// Claude Code's box table of 13 rows, Cyrillic text and dashes: an owner's answer split in two
const RESULTS = [["Набор", "Коммит", "Результат"], ...["make test", "clippy", "e2e_all", "e2e_live", "e2e_panel", "e2e_lastlines",
  "e2e_remote", "e2e_remote_agents", "реальный sshd AC-81–84", "агенты на хосте, Alpine / Debian", "security_check (включая отказы terminumd)"]
  .map((s, i) => [s, "3e78316", i ? "пройдено" : "пройдено — 0 ошибок"])];
const boxTable = (cells) => {
  const w = cells[0].map((_, c) => Math.max(...cells.map((r) => r[c].length)) + 2);
  const rule = (l, j, r) => `  ${l}${w.map((n) => "─".repeat(n)).join(j)}${r}`;
  const row = (r) => `  │${r.map((v, c) => ` ${v.padEnd(w[c] - 1)}`).join("│")}│`;
  return [rule("┌", "┬", "┐"), ...cells.flatMap((r, i) => (i ? [rule("├", "┼", "┤"), row(r)] : [row(r)])), rule("└", "┴", "┘")];
};

test("a long drawn table is one widget with its head: never a widget from a row of it, the rows above it text", () => {
  const table = boxTable(RESULTS), lead = "x\n".repeat(30);
  assert.equal(table.length, 25);
  const whole = scan(rows(lead + table.join("\n") + "\n\n✻ Done"));
  assert.deepEqual(whole.found.map(({ kind, from, to, table: t }) => [kind, from, to, t.head, t.rows.length]),
    [["table", 30, 54, ["Набор", "Коммит", "Результат"], 11]]);
  // caught while a row is drawn again (text after it): no table yet, and settled at its top, not
  // inside it, so the next scan, from there, finds it whole
  const half = [...table.slice(0, 11), table[11].slice(0, 20), "", "✻ Thinking…"];
  const first = scan(rows(lead + half.join("\n")));
  assert.deepEqual(first.found, []);
  assert.ok(first.settled <= 30, `settled at its top rule, not inside it (${first.settled})`);
  const later = rows(lead + table.join("\n") + "\n\n✻ Done").slice(first.settled);
  assert.deepEqual(scan(later).found.map(({ from, table: t }) => [from + first.settled, t.head[0], t.rows.length]), [[30, "Набор", 11]]);
  // a line off its grid with text after it (a frame drawn over another): it stays text, all of
  // it; no widget from the row after that line, that row as its head
  const broken = [...table.slice(0, 11), table[11].slice(0, 20), ...table.slice(5)];
  assert.deepEqual(found(lead + broken.join("\n") + "\n\n✻ Done"), []);
  // its top rule scrolled away (a history's first line): it starts at its head row still
  assert.deepEqual(found(table.slice(1).join("\n")).map(({ kind, from }) => [kind, from]), [["table", 0]]);
});

test("a drawn table across the history's end: settled at its top rule, never at its head row on the screen", () => {
  // Claude Code redraws (Ask typed into its box): a frame with a row drawn over, the screen
  // beginning at the table's head row; the next scan, from settled, finds it whole with its top
  const table = boxTable(RESULTS), lead = "x\n".repeat(30);
  const over = [...table.slice(0, 11), table[11].slice(0, 20), ...table.slice(5), "", "✻ Thinking…", ..."abcdefghij"];
  const first = scan(rows(lead + over.join("\n")), undefined, 31);
  assert.deepEqual(first.found, []);
  assert.ok(first.settled <= 30, `settled at its top rule, not at the screen's first row (${first.settled})`);
  const later = rows(lead + table.join("\n") + "\n\n✻ Done").slice(first.settled);
  assert.deepEqual(scan(later).found.map(({ from, table: t }) => [from + first.settled, t.head[0], t.rows.length]), [[30, "Набор", 11]]);
  // nothing on the screen is settled, a box or not
  assert.equal(scan(rows("x\n".repeat(40)), undefined, 10).settled, 10);
});

test("a Markdown document ends before a drawn table, which is a widget of its own", () => {
  const text = "## Plan\n\n- read **a**\n- write `b`\n" + CLAUDE_TABLE.slice(2, 10).join("\n");
  assert.deepEqual(found(text).map((r) => r.kind), ["markdown", "table"]);
});

test("pipe tables: their own widget where an agent colored text around them, rows tmux wrapped joined", () => {
  const rows = [L("- a run of `code`, colored:", { styled: true }), L(""), L("| Итог | Версии"), L("        |"), L("|------|-----"), L("---|"),
    L("| Подтверждено | minimatch |"), L(""), L("| Вердикт | Строк |"), L("|---|---:|"), L("| Подтверждено | 212 |"), L(""), L("- next", { styled: true })];
  const found = scan(rows).found.filter((r) => r.kind === "table");
  assert.deepEqual(found.map((r) => [r.from, r.to, r.table.head.join("/")]), [[2, 6, "Итог/Версии"], [8, 10, "Вердикт/Строк"]]);
  assert.deepEqual(found[1].table.align, [false, true], "a right-aligned column");
});

// Claude Code's pipe table at a phone's width (44 columns): each row padded to its columns' width,
// wrapped at a bar in its middle, the padding into blank lines, the separator over six lines.
test("a pipe table wrapped at a phone's width: rows joined by their bars, blank lines and all", () => {
  const rows = ["  Статусы:", "", "  | Статус                           |", "  Что видит пользователь", "", "           |",
    "  |----------------------------------|-------", "  ------------------------------------------", "  ------------------------------------------",
    "  ----------|", "  | защищено                         |", "  «Защищено», время последней проверки", "", "",
    "           |", "  | свой NODE_OPTIONS                |", "  «Не защищено: задан свой NODE_OPTIONS.", "  Добавьте --require в NODE_OPTIONS» |",
    "  | неподдерживаемый Node | «Обновите Node.js до 16+» |", "", "  Дальше текст."].map(L);
  const t = scan(rows).found.filter((r) => r.kind === "table");
  assert.equal(t.length, 1);
  assert.deepEqual([t[0].from, t[0].to], [2, 18]);
  assert.deepEqual(t[0].table.head, ["Статус", "Что видит пользователь"]);
  assert.deepEqual(t[0].table.rows, [["защищено", "«Защищено», время последней проверки"],
    ["свой NODE_OPTIONS", "«Не защищено: задан свой NODE_OPTIONS. Добавьте --require в NODE_OPTIONS»"],
    ["неподдерживаемый Node", "«Обновите Node.js до 16+»"]]);
});

test("a pipe table's row with a bar too many is a row of its own, the next rows joined again", () => {
  const rows = ["| a | b |", "|---|---|", "| 1 |", "2 |", "| x | y | z |", "| 3 | 4 |"].map(L);
  const t = scan(rows).found.filter((r) => r.kind === "table");
  assert.deepEqual(t[0].table.rows, [["1", "2", ""], ["x", "y", "z"], ["3", "4", ""]]);
  // a wrapped table: after a row with a bar more the next wrapped rows join again
  const wrapped = ["| a", "| b |", "|---|---|", "| x | y | z |", "| 3 |", "", "", "4 |", "| 5 | 6 |"].map(L);
  assert.deepEqual(scan(wrapped).found.find((r) => r.kind === "table").table.rows, [["x", "y", "z"], ["3", "4", ""], ["5", "6", ""]]);
});

test("a table's last row short of its bars never takes the prose after a blank line; a wrapped table's may span blank lines", () => {
  const rows = ["| a | b |", "|---|---|", "| 4 | 5", "", "Use `ls | wc` here."].map(L);
  const t = scan(rows).found;
  assert.deepEqual(t.map((r) => [r.kind, r.from, r.to]), [["table", 0, 2]], "the row cut short is a row (GFM); the prose stays text");
  assert.deepEqual(t[0].table.rows, [["4", "5"]]);
});

test("a row cut short in the middle of a pipe table is a row, and the rows after it too (GFM)", () => {
  const rows = ["| a | b |", "|---|---|", "| 1 | 2 |", "| 3 | four is cut", "| 5 | 6 |", "", "after"].map(L);
  const t = scan(rows).found.filter((r) => r.kind === "table");
  assert.deepEqual(t.map((r) => [r.from, r.to]), [[0, 4]]);
  assert.deepEqual(t[0].table.rows, [["1", "2"], ["3", "four is cut"], ["5", "6"]]);
  assert.deepEqual(pipeTable(["| a | b |", "|---|---|", "| x | y \\|"].map(L), 0).rows, [["x", "y \\|"]], "an escaped bar at the end is the cell's");
  const tree = pipeTable(["| a | b |", "|---|---|", "| 1 | 2 |", "|-- src", "| grep x", "|   `-- lib"].map(L), 0);
  assert.deepEqual([tree.to, tree.rows], [2, [["1", "2"]]], "a tree's or a pipeline's line under it is no row");
});

test("a pipe table whose rows are all cut short (`cut -c`) is a table of them; a head alone before other text still is one", () => {
  const cut = ["| Version | Date | Change |", "|---|---|---|", "| 0.24.13 | 2026-10-31 | Owner: the list's foot is a", "again (it unfolded", "",
    "| Version | Date | Change |", "|---|---|---|", "| 0.24.13 | 2026-10-31 | Owner: `cut -c` and **bold** the", "| 0.24.12 | 2026-10-30 | Owner: `tmux` a **b** and",
    "| 0.24.11 | 2026-10-29 | Owner: [a](b) `x`", ""];
  const found = scan(cut.map((t) => L(t))).found;
  assert.deepEqual(found.map((r) => [r.kind, r.from, r.to]), [["table", 0, 2], ["table", 5, 9]], "never a head alone, nor a document from its separator");
  assert.deepEqual(found[1].table.rows.map((r) => r[0]), ["0.24.13", "0.24.12", "0.24.11"]);
  const alone = scan(["| Field | Value |", "|---|---|", "", "next"].map((t) => L(t))).found.filter((r) => r.kind === "table");
  assert.deepEqual(alone.map((r) => [r.from, r.to, r.table.rows.length]), [[0, 1, 0]]);
});

test("a git diff of a Markdown table is one table from its head to its last row, never its head alone", () => {
  const text = ["     @@ -13,6 +13,7 @@ Change log:", "", "      | Version | Date |", "      |---|---|",
    "     +| 1.21.0 | signs of automation, cut at the wi", "     dth here", "      | 1.20.0 | the server's language |",
    "     @@ -128,7 +129,7 @@ Open questions:"].join("\n");
  const t = regions(rows(text)).filter((r) => r.kind === "table");
  assert.equal(t.length, 1, JSON.stringify(t));
  assert.equal(t[0].from, 2);
  assert.equal(t[0].to, 6);
  assert.deepEqual(t[0].table.marks, ["+", ""]);
});

test("a git diff table through the whole parse: a cut context row never takes the removed row under it; Claude Code's collapsed lines, a prompt and a one-column code diff are no rows", () => {
  const cut = ["      | Version | Date | Change |", "      |---|---|---|",
    "     +| 1.21.0 | 2026-11-01 | signs of automation —", "     (WebDriver, driver globals, an a",
    "      | 1.20.0 | 2026-11-01 | the server answers", "     every text the server words",
    "     -| 1.19.0 | 2026-10-31 | Ready for translation. |",
    "      | 1.18.0 | 2026-10-31 | Visitors counted |"].join("\n");
  const t = regions(rows(cut)).filter((r) => r.kind === "table");
  assert.equal(t.length, 1, JSON.stringify(t));
  assert.deepEqual(t[0].table.marks, ["+", "", "-", ""]);
  assert.deepEqual(t[0].table.rows.map((r) => r.length), [3, 3, 3, 3]);
  assert.equal(t[0].table.rows[2][2], "Ready for translation.");

  const folded = ["      | a | b |", "      |---|---|", "     +| 1 | a row cut sh", "     … +42 lines (ctrl+o to expand)"].join("\n");
  const f = regions(rows(folded)).find((r) => r.kind === "table");
  assert.equal(f.table.rows[0][1], "a row cut sh", "Claude Code's collapsed lines are no part of a row");

  const raw = ["| a | b |", "|---|---|", "+| 1 | added, no closing bar", "alex@devbox ~/app % git status"].join("\n");
  const r = regions(rows(raw)).find((x) => x.kind === "table");
  assert.ok(!r || !JSON.stringify(r.table.rows).includes("git status"), "at column 0 the prompt is never a row's rest");

  const code = ["-| A", "+| B", " | C", "alex@devbox ~/app % "].join("\n");
  assert.equal(regions(rows(code)).filter((x) => x.kind === "table").length, 0, "a code diff of variants is no table");
});

test("AC-31 a table's rule is told from a tree's or a panel's lines", () => {
  for (const t of ["┌───┬───┐", "  ├───┼───┤", "└───┴───┘", "+---+---+", "  ⎿  ┌───┬───┐"]) assert.ok(tableRule(t), t);
  for (const t of ["├── src", "│   └── x.js", "└── y", "╭────────╮", "│ one │", "| a | b |", "+---+"]) assert.ok(!tableRule(t), t);
});

test("AC-31 a line goes on the table above it, not one under a bottom rule, nor a line of prose", () => {
  assert.ok(insideBox("│ a │ b │", "┌───┬───┐") && insideBox("├───┼───┤", "│ a │ b │"));
  assert.ok(insideBox("| 1 | a |", "+---+---+") && insideBox("| 2 | b |", "| 1 | a |"));
  assert.ok(!insideBox("┌───┬───┐", "└───┴───┘"), "a table right under another");
  assert.ok(!insideBox("│ a │ b │", "some prose") && !insideBox("some prose", "│ a │ b │") && !insideBox("│ a │ b │", undefined));
});

test("AC-31 a fence a tool's output left open stops nothing: what follows is read", () => {
  const table = ["┌───┬───┐", "│ a │ b │", "├───┼───┤", "│ 1 │ 2 │", "└───┴───┘", "after"];
  const kinds = (lines) => scan(lines.map((t) => L(t))).found.map((r) => [r.kind, r.lang]);
  assert.deepEqual(kinds(["⏺ Bash(sed spec)", "  ⎿  text", "     ```gherkin", "     Scenario: cut", "     … +40 lines", "", "⏺ The answer", "", ...table]), [["table", ""]], "cut by the next message");
  assert.deepEqual(kinds(["$ cat part", "```js", "let a", "$ ls", ...table]), [["table", ""]], "cut by a prompt");
  assert.deepEqual(kinds(["⏺ Bash(sed spec)", "     ```gherkin", "⏺ Next", "  ```js", "  let a", "  ```"]), [["code", "js"]], "never closed by the next one's fence");
  assert.deepEqual(kinds(["  ```gherkin", "  Scenario: whole", "  ```", "⏺ Next"]), [["code", "gherkin"]], "closed: its starter is its language");
  assert.deepEqual(kinds(["  ```text", "  ⏺ quoted, indented as it", "  ```"]), [["code", "text"]], "a mark inside it, no less indented, is its text");
  assert.deepEqual(kinds(["```gherkin", "x", "$ ls", ...table, "```js", "let a", "```"]), [["table", ""], ["code", "js"]], "at column 0: a prompt, then another fence opening");
  assert.deepEqual(kinds(["• Ran sed spec", "  └ text", "     ```gherkin", "• The answer", ...table]), [["table", ""]], "Codex's marks");
  const asked = scan(["text", "```sh", "$ ls", "a", "b", "c", "d", "e", "f", "g"].map((t) => L(t)));
  assert.ok(asked.settled <= 1, `a fence open with a prompt-shaped line is not settled past its start: ${asked.settled}`);
  const printing = scan(["text", "```js", "let a"].map((t) => L(t)));
  assert.ok(printing.found.length === 0 && printing.settled <= 1, "with nothing after it, it is still printing");
});

test("AC-31 any starter is its language; Mermaid by its names is a diagram", () => {
  const kind = (lang) => scan(["```" + lang, "a", "```", "b"].map((t) => L(t))).found[0];
  for (const lang of ["mermaid", "Mermaid", "mermaidjs", "mmd"]) assert.equal(kind(lang).kind, "mermaid", lang);
  for (const lang of ["gherkin", "json", "c++", "objective-c", "plantuml", "dot"]) assert.deepEqual([kind(lang).kind, kind(lang).lang], ["code", lang]);
});

test("AC-31 scan says where a table not whole yet starts", () => {
  const open = ["text", "┌───┬───┐", "│ a │ b │"].map((t) => L(t));
  assert.equal(scan(open, open.length).waiting, 1);
  assert.equal(scan(["text", "more"].map((t) => L(t)), 2).waiting, null);
});
