// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// A pressed blob's pulse (motion.css): a fresh name for its animation (a, b) so each press
// restarts it. Keys have no pulse: they sink into the panel while pressed (shapes.css).
const BLOBS = "#foldfab, #markadd, #markcopy, #marksfab, #findfab, #splitfab, #sentfab, #linksfab, #askfab, #attachfab, #remindfab, #fabs button, #bottom";
const calm = matchMedia("(prefers-reduced-motion: reduce)");

const blob = (e) => (!calm.matches && e.target instanceof Element && e.target.closest(BLOBS)) || null;
const mark = (b) => { if (!b.disabled) b.dataset.press = b.dataset.press === "a" ? "b" : "a"; };

document.addEventListener("pointerdown", (e) => { const b = e.button === 0 && blob(e); if (b) mark(b); }, { capture: true, passive: true });
document.addEventListener("click", (e) => { const b = e.detail === 0 && blob(e); if (b) mark(b); }, { capture: true, passive: true });   // Enter or Space
document.addEventListener("touchstart", () => {}, { passive: true });   // iOS Safari applies :active on a tap only with a listener
