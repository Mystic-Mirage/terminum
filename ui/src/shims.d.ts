// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
declare module "markdown-it-task-lists" { const plugin: any; export default plugin; }
declare module "markdown-it-footnote" { const plugin: any; export default plugin; }
declare module "*.css";
declare module "*.svg" { const markup: string; export default markup; }
/** The build this bundle belongs to (esbuild `define`, AC-34). */
declare const __BUILD__: string;
/** The page's rule for the address a fetched file is shown from (web/static/shown.js, AC-09). */
declare module "*/shown.js" { export function shownAs(type: string | null | undefined): string; export function shownUrl(blob: Blob): Promise<string>; }
/** The sign-in's proof and credential, one copy for the page and the panel (web/static/proof.js, AC-09). */
declare module "*/proof.js" { export function hello(secret: string, fetchFn: typeof fetch): Promise<{ state: "ok" | "unknown" | "down" | "bad"; cred?: string; apps?: boolean }>; }
