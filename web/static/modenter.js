// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial

/** ⌘Enter or Ctrl+Enter in dialog `box` presses its primary button when it is on (the keyboard's
 *  send, as in a chat); a plain Enter stays a new line in its note. */
export function sendOnModEnter(box) {
  box.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" || !(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey || e.isComposing) return;
    e.preventDefault();
    const go = box.querySelector(".actions .primary");
    if (go && !go.disabled) go.form.requestSubmit(go);
  });
}
