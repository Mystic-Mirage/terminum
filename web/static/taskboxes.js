// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A task's window and RUN's, one markup for the board and the session page (terminum-kanban.md AC-150): each page
// puts them in once, before kanbanwin.js and kanbanrun.js look for them.
const MARKUP = `
<dialog id="taskbox" class="box dlg" aria-labelledby="tasktitle">
  <form method="dialog">
    <div class="dhead"><h2 id="tasktitle"></h2><button id="taskclose" class="icon x" type="button" title="Close (Esc)" aria-label="Close"><svg class="lu" viewBox="0 0 24 24" aria-hidden="true" style="--s:16px"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg></button></div>
    <div id="taskbody"></div>
  </form>
</dialog>

<dialog id="runbox" class="box dlg" aria-labelledby="runhead">
  <form method="dialog" id="runform">
    <div class="dhead"><h2 id="runhead">Run</h2><button class="icon x" type="button" title="Close (Esc)" aria-label="Close"><svg class="lu" viewBox="0 0 24 24" aria-hidden="true" style="--s:16px"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg></button></div>
    <p id="runnone" hidden></p>
    <label id="runagentrow" class="fieldrow">Agent <select id="runagent" class="field"></select></label>
    <label class="fieldrow">Message to the agent <textarea id="runmessage" class="field" rows="3" placeholder="Optional: what to mind, in a line or two"></textarea></label>
    <details id="runhow" class="more"><summary>Where and how</summary>
      <dl class="runfacts"><dt>Server</dt><dd id="runserver"></dd><dt>Folder</dt><dd id="runfolder"></dd><dt>Command</dt><dd id="runcommand"></dd></dl>
    </details>
    <details id="runwhat" class="more"><summary>What will be sent</summary>
      <p id="runmarked" class="runmarked" hidden>An agent wrote some of this text.</p>
      <pre id="runprompt" class="runprompt" tabindex="0" aria-label="What will be sent"></pre>
    </details>
    <p id="runerror" role="alert" hidden></p>
    <div class="actions">
      <button id="runadd" type="button" hidden>Add agent</button>
      <button id="runfresh" type="button" hidden>Run from scratch</button>
      <button value="cancel" type="submit" formnovalidate>Cancel</button>
      <button id="rungo" type="button" class="primary">Run</button>
    </div>
  </form>
</dialog>
`;

/** The two windows in the page, once. */
export function taskBoxes() {
  if (document.getElementById("taskbox")) return;
  document.body.insertAdjacentHTML("beforeend", MARKUP);
}
