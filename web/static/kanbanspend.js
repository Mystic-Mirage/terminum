// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// What a card says its session uses and spent (terminum-dispatcher.md AC-127 to AC-129): terminumd's numbers,
// in words; nothing is added up here.
import { el } from "./listparts.js";
import { countOf } from "./kanbanutil.js";

/** What a finished task's sessions spent (terminum-dispatcher.md AC-129): tokens that count, dollars where known. */
export function spentLine(s) {
  const line = el("span", "cspent", [countOf(s.counted), s.usd != null ? `≈$${s.usd.toFixed(2)}` : null].filter(Boolean).join(" · "));
  line.title = [`${s.input.toLocaleString("en")} in, ${s.output.toLocaleString("en")} out, ${s.cache_write.toLocaleString("en")} written to the cache, ${s.cache_read.toLocaleString("en")} read from it`,
    s.usd != null ? "an estimate at list prices" : "no price known for its model", s.with.length ? `for this and ${s.with.join(", ")} together` : null].filter(Boolean).join("; ");
  return line;
}

/** What its session uses and what stands idle in it (terminum-dispatcher.md AC-127 to AC-129): as of the last walk. */
export function usageWords(u) {
  if (!u) return "";
  const words = u.not_measured ? [u.not_measured] : [`CPU ${u.cpu}%`, `${u.gib} GiB`, `${u.minutes} min`];
  if (u.budget_total) words.unshift(`${Math.floor(u.budget_used / 60)} of ${u.budget_total / 60} min`);
  if (u.tokens) words.push(`${u.tokens.counted.toLocaleString("en")} tokens`);
  else if (u.tokens === null) words.push("Tokens not known");
  for (const s of u.idle_subs ?? []) words.push(`idle: ${s.kind || "subagent"} ${s.id}`);
  return words.join(" · ");
}
