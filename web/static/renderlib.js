// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
let lib = null, tries = 0;
/** The renderers: the Panel's renderers.js through the page's /fb proxy, loaded on first need. */
export function renderers() {
  // a failed module stays failed under its URL: a new try asks under another
  return (lib ??= import(tries ? `/fb/renderers.js?try=${tries}` : "/fb/renderers.js").catch((e) => {
    lib = null;                                    // asked again next time (fbd may come back)
    tries++;
    throw new Error(`The renderer did not load (${e.message}); is terminum's server running?`);
  }));
}
