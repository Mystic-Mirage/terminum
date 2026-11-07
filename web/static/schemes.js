// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// Color schemes (AC-12): 19 well-known terminal palettes and a light one of terminum's own (Paper) as swatches in the menu. A press shows
// the scheme at once over the session's theme (its font and size stay) and keeps it in
// settings.json (page.colorScheme): one scheme for every browser, each told when it changes;
// "Theme" goes back to the theme terminumd sends for the session. Each scheme: its
// background, text, cursor, selection, whether it is dark, and the 16 ANSI colors (black, red,
// green, yellow, blue, magenta, cyan, white, then their bright forms).

const S = (name, bg, fg, cursor, sel, dark, ansi) => ({ name, bg, fg, cursor, sel, dark, ansi: ansi.split(" ") });
export const SCHEMES = [
  S("Dracula", "#282a36", "#f8f8f2", "#f8f8f2", "#44475a", true, "#21222c #ff5555 #50fa7b #f1fa8c #bd93f9 #ff79c6 #8be9fd #f8f8f2 #6272a4 #ff6e6e #69ff94 #ffffa5 #d6acff #ff92df #a4ffff #ffffff"),
  S("Solarized Dark", "#002b36", "#839496", "#93a1a1", "#073642", true, "#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #eee8d5 #002b36 #cb4b16 #586e75 #657b83 #839496 #6c71c4 #93a1a1 #fdf6e3"),
  S("Solarized Light", "#fdf6e3", "#657b83", "#586e75", "#eee8d5", false, "#073642 #dc322f #859900 #b58900 #268bd2 #d33682 #2aa198 #eee8d5 #002b36 #cb4b16 #586e75 #657b83 #839496 #6c71c4 #93a1a1 #fdf6e3"),
  S("Nord", "#2e3440", "#d8dee9", "#d8dee9", "#434c5e", true, "#3b4252 #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #88c0d0 #e5e9f0 #4c566a #bf616a #a3be8c #ebcb8b #81a1c1 #b48ead #8fbcbb #eceff4"),
  S("Gruvbox Dark", "#282828", "#ebdbb2", "#ebdbb2", "#504945", true, "#282828 #cc241d #98971a #d79921 #458588 #b16286 #689d6a #a89984 #928374 #fb4934 #b8bb26 #fabd2f #83a598 #d3869b #8ec07c #ebdbb2"),
  S("Gruvbox Light", "#fbf1c7", "#3c3836", "#3c3836", "#ebdbb2", false, "#fbf1c7 #cc241d #98971a #d79921 #458588 #b16286 #689d6a #7c6f64 #928374 #9d0006 #79740e #b57614 #076678 #8f3f71 #427b58 #3c3836"),
  S("One Dark", "#282c34", "#abb2bf", "#528bff", "#3e4451", true, "#282c34 #e06c75 #98c379 #e5c07b #61afef #c678dd #56b6c2 #abb2bf #5c6370 #e06c75 #98c379 #e5c07b #61afef #c678dd #56b6c2 #ffffff"),
  S("Monokai", "#272822", "#f8f8f2", "#f8f8f0", "#49483e", true, "#272822 #f92672 #a6e22e #f4bf75 #66d9ef #ae81ff #a1efe4 #f8f8f2 #75715e #f92672 #a6e22e #f4bf75 #66d9ef #ae81ff #a1efe4 #f9f8f5"),
  S("Tokyo Night", "#1a1b26", "#c0caf5", "#c0caf5", "#33467c", true, "#15161e #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff #a9b1d6 #414868 #f7768e #9ece6a #e0af68 #7aa2f7 #bb9af7 #7dcfff #c0caf5"),
  S("Catppuccin Mocha", "#1e1e2e", "#cdd6f4", "#f5e0dc", "#585b70", true, "#45475a #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #bac2de #585b70 #f38ba8 #a6e3a1 #f9e2af #89b4fa #f5c2e7 #94e2d5 #a6adc8"),
  S("Catppuccin Latte", "#eff1f5", "#4c4f69", "#dc8a78", "#acb0be", false, "#5c5f77 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #acb0be #6c6f85 #d20f39 #40a02b #df8e1d #1e66f5 #ea76cb #179299 #bcc0cc"),
  S("GitHub Dark", "#0d1117", "#c9d1d9", "#58a6ff", "#264f78", true, "#484f58 #ff7b72 #3fb950 #d29922 #58a6ff #bc8cff #39c5cf #b1bac4 #6e7681 #ffa198 #56d364 #e3b341 #79c0ff #d2a8ff #56d4dd #f0f6fc"),
  S("GitHub Light", "#ffffff", "#24292f", "#0969da", "#b6e3ff", false, "#24292f #cf222e #116329 #4d2d00 #0969da #8250df #1b7c83 #6e7781 #57606a #a40e26 #1a7f37 #633c01 #218bff #a475f9 #3192aa #8c959f"),
  S("Tomorrow Night", "#1d1f21", "#c5c8c6", "#c5c8c6", "#373b41", true, "#1d1f21 #cc6666 #b5bd68 #f0c674 #81a2be #b294bb #8abeb7 #c5c8c6 #969896 #cc6666 #b5bd68 #f0c674 #81a2be #b294bb #8abeb7 #ffffff"),
  S("Night Owl", "#011627", "#d6deeb", "#80a4c2", "#1d3b53", true, "#011627 #ef5350 #22da6e #addb67 #82aaff #c792ea #21c7a8 #ffffff #575656 #ef5350 #22da6e #ffeb95 #82aaff #c792ea #7fdbca #ffffff"),
  S("Ayu Dark", "#0b0e14", "#bfbdb6", "#e6b450", "#273747", true, "#11151c #ea6c73 #7fd962 #f9af4f #53bdfa #cda1fa #90e1c6 #c7c7c7 #686868 #f07178 #aad94c #ffb454 #59c2ff #d2a6ff #95e6cb #ffffff"),
  S("Everforest Dark", "#2d353b", "#d3c6aa", "#d3c6aa", "#543a48", true, "#475258 #e67e80 #a7c080 #dbbc7f #7fbbb3 #d699b6 #83c092 #d3c6aa #475258 #e67e80 #a7c080 #dbbc7f #7fbbb3 #d699b6 #83c092 #d3c6aa"),
  S("Rosé Pine", "#191724", "#e0def4", "#524f67", "#403d52", true, "#26233a #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4 #6e6a86 #eb6f92 #31748f #f6c177 #9ccfd8 #c4a7e7 #ebbcba #e0def4"),
  S("Kanagawa", "#1f1f28", "#dcd7ba", "#c8c093", "#2d4f67", true, "#16161d #c34043 #76946a #c0a36e #7e9cd8 #957fb8 #6a9589 #c8c093 #727169 #e82424 #98bb6c #e6c384 #7fb4ca #938aa9 #7aa89f #dcd7ba"),
  // terminum's own light scheme: warm paper, soft pastel hues dark enough to read (4.5:1 for the eight plain ones)
  S("Paper", "#faf9f5", "#2b2925", "#c4625a", "#e6e3d8", false, "#3d3a35 #b8524a #477a51 #946b1f #4a6fa8 #8f5a9c #36787d #8a8780 #6b675f #c4625a #5a8f64 #a37a2c #5a7fb8 #9d68aa #488b90 #b3afa5"),
];

/** The theme with scheme `name`'s colors over it (its font and size kept); the theme itself for none. */
export function withScheme(theme, name) {
  const s = SCHEMES.find((x) => x.name === name);
  if (!s || !theme) return theme;
  return { ...theme, bg: s.bg, fg: s.fg, cursor: s.cursor, cursorText: s.bg, bold: s.fg, sel: s.sel, selfg: s.fg, dark: s.dark, ansi: s.ansi };
}

/** The swatches in `box` (the menu's Colors row): a press applies a scheme at once, "Theme" the
 *  session's own, and sends the choice to terminumd; settings.json's choice comes with `page(p)`.
 *  onPick(): the colors changed; say(text): the press was not sent, so nothing changed. */
export function colorSchemes({ box, send, onPick, say }) {
  let chosen = null;
  try { localStorage.removeItem("colorScheme"); } catch { /* where a page before 0.9 kept its own: gone */ }
  const known = (name) => (SCHEMES.some((s) => s.name === name) ? name : null);
  const swatch = (s) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "swatch" + (s ? "" : " theme");
    b.dataset.scheme = s?.name ?? "";
    b.title = s?.name ?? "The session's theme";
    b.setAttribute("aria-label", s ? `Colors: ${s.name}` : "Colors: the session's theme");
    if (s) {
      b.style.cssText = `--sw-bg:${s.bg};--sw-fg:${s.fg}`;
      const bars = document.createElement("span");
      bars.className = "bars";
      for (const i of [1, 2, 3, 4, 5]) { const c = document.createElement("i"); c.style.background = s.ansi[i]; bars.append(c); }
      b.append(Object.assign(document.createElement("span"), { className: "aa", textContent: "Aa" }), bars);
    } else b.textContent = "Theme";
    b.onclick = () => pick(s?.name ?? null);
    return b;
  };
  box.replaceChildren(swatch(null), ...SCHEMES.map(swatch));
  function mark() {
    for (const b of box.querySelectorAll(".swatch")) b.setAttribute("aria-pressed", String((b.dataset.scheme || null) === chosen));
  }
  function show(name) {
    if (name === chosen) return;
    chosen = name;
    mark();
    onPick();
  }
  function pick(name) {
    if (send({ t: "scheme", name })) show(name);
    else say("Not connected: the colors stay as they were");
  }
  mark();
  return {
    over: (theme) => withScheme(theme, chosen),
    /** settings.json's page part: its scheme, on every browser. */
    page(p) { show(known(p.colorScheme ?? null)); },
  };
}
