// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The header's menu (AC-07, AC-15, AC-17): the settings (a pane's actions are its row's menu,
// rowmenu.js), Restore again after a restore report, the report's banner over the terminal, and
// the chip of a newer release with its own menu.
import { placeModal } from "./fabpane.js";
import { closeButton } from "./banners.js";

const $ = (id) => document.getElementById(id);

/** pick(id) opens a pane; signOut() for Sign out (this browser's sign-in ends); copy(text) to the clipboard. */
const RESTORED_FOR = 30000;     // ms the restore report stays when nobody closes it

export function paneMenu({ send, pick, signOut, copy }) {
  function open() { $("menu").hidden = false; placeModal($("menu"), close); $("menubtn").setAttribute("aria-expanded", "true"); }
  function close() { $("menu").hidden = true; $("menubtn").setAttribute("aria-expanded", "false"); }
  $("menubtn").onclick = (e) => { e.stopPropagation(); $("menu").hidden ? open() : close(); };
  document.addEventListener("click", (e) => { if (!$("menu").hidden && !e.target.closest("#menu")) close(); });

  $("restoreagain").onclick = () => { close(); send({ t: "restore" }); };
  $("planmenu").onclick = () => { close(); document.dispatchEvent(new Event("planopen")); };   // the shown session's (AC-97)
  $("tipshow").onclick = () => { close(); document.dispatchEvent(new Event("tipshow")); };     // today's tip, closed with × (AC-101)
  $("signout").onclick = () => { close(); signOut(); };

  // ---------- the restore report (AC-15) ----------
  // "Restored 2 sessions, 4 windows. 1 pane ran ssh devbox: start it again", its panes linked.
  let restoreTimer = 0;
  function showRestore(report) {
    const box = $("restored");
    clearTimeout(restoreTimer);
    if (!report) { box.hidden = true; return; }        // another page closed it
    $("restoreagain").hidden = false;
    const text = document.createElement("span");
    if (typeof report === "string" || typeof report?.text === "string") text.textContent = report.text ?? report;
    else {
      const n = (k, one) => `${report?.[k] ?? 0} ${one}${report?.[k] === 1 ? "" : "s"}`;
      text.append(`Restored ${n("sessions", "session")}, ${n("windows", "window")}.`);
      const ran = report?.ran ?? [];
      if (ran.length) {
        text.append(` ${ran.length} pane${ran.length === 1 ? "" : "s"} ran `);
        ran.forEach((p, i) => {
          if (i) text.append(", ");
          const a = document.createElement("button");
          a.type = "button"; a.className = "link"; a.textContent = p.program ?? p.cmd ?? "a program";
          a.onclick = () => { if (p.id) pick(p.id); };
          text.append(a);
        });
        text.append(`: start ${ran.length === 1 ? "it" : "them"} again.`);
      }
      for (const f of report?.failed ?? []) text.append(` Not restored: ${f}.`);
    }
    box.replaceChildren(text, closeButton("Close the report", () => { box.hidden = true; send({ t: "restoreseen" }); }));   // closed on every page
    box.hidden = false;
    restoreTimer = setTimeout(() => { box.hidden = true; }, RESTORED_FOR);
  }

  // ---------- a newer release (AC-17) ----------
  // The header's chip "↑ v0.4.0"; its menu copies the upgrade command, skips the release or
  // stops the checks. Only a tag terminumd checked reaches here (vMAJOR.MINOR.PATCH).
  let latest = null;
  function showUpdate(tag) {
    latest = typeof tag === "string" && /^v\d+\.\d+\.\d+$/.test(tag) ? tag : null;
    $("updchip").hidden = !latest;
    $("updchip").textContent = latest ? `↑ ${latest}` : "";
    if (!latest) hideUpdate();
    else $("updskip").textContent = `Skip ${latest}`;
  }
  function hideUpdate() { $("updmenu").hidden = true; $("updchip").setAttribute("aria-expanded", "false"); }
  $("updchip").onclick = (e) => {
    e.stopPropagation();
    if (!$("updmenu").hidden) return hideUpdate();
    const r = $("updchip").getBoundingClientRect();
    $("updmenu").hidden = false;
    if (!placeModal($("updmenu"), hideUpdate)) {
      $("updmenu").style.top = `${r.bottom + 6}px`;
      $("updmenu").style.right = `${Math.max(8, innerWidth - r.right)}px`;
    }
    $("updchip").setAttribute("aria-expanded", "true");
  };
  $("updcopy").onclick = () => { hideUpdate(); copy("terminum upgrade"); };
  $("updskip").onclick = () => { hideUpdate(); send({ t: "updatechoice", action: "skip", tag: latest }); };
  $("updoff").onclick = () => { hideUpdate(); send({ t: "updatechoice", action: "off" }); };
  document.addEventListener("click", (e) => { if (!$("updmenu").hidden && !e.target.closest("#updmenu")) hideUpdate(); });

  return { open, close, showRestore, showUpdate };
}
