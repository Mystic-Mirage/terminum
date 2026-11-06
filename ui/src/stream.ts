// SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-Commercial
// The event socket from terminumd (/fb/api/ws): a WebSocket, because a browser gives a page six
// HTTP connections per host and a held SSE stream would take one per panel (AC-61). The sign-in
// rides in its subprotocols. It opens only after terminumd proved that it knows the sign-in
// (AC 09): while terminumd is not running another program may hold its port. When it closes it
// is opened again after a new proof: at once while terminumd restarts (AC-16), then less often
// (up to every 2 s), and every 5 s while the proof fails. terminumd closes it too when this
// panel fell behind; the first message after opening is always the current state.

import { ApiError, forgetProof, proveServer, socketProtocols, socketUrl } from "./api";

export interface StreamHost {
  on: Record<string, (data: any) => void>;  // event name → handler of its JSON data
  open(): void;                               // connected, or connected again
  lost(unproven: boolean): void;              // terminumd is out of reach, or did not prove itself
}

export class Stream {
  private ws: WebSocket | null = null;
  private retry = 0;
  private delay = 250;
  private gen = 0;  // a connect() whose proof returns after a newer one or a close opens nothing

  constructor(private host: StreamHost) {}

  async connect() {
    const gen = ++this.gen;
    clearTimeout(this.retry);
    try { await proveServer(); } catch (e) { if (gen === this.gen) this.lost(e instanceof ApiError); return; }
    if (gen !== this.gen) return;
    const ws = (this.ws = new WebSocket(socketUrl(), socketProtocols()));
    ws.onmessage = (m) => {
      this.delay = 250; // reset only once terminumd sends: one that accepts and closes is not hammered
      const { event, data } = JSON.parse(m.data as string);
      this.host.on[event]?.(data);
    };
    ws.onclose = () => {
      if (this.ws !== ws) return; // closed by us
      this.drop();
      forgetProof();
      this.lost(false);
    };
    ws.onopen = () => this.host.open();
  }

  /** Stop listening; nothing reconnects until connect() is called. */
  close() { this.drop(); }

  private drop() {
    this.gen++;
    clearTimeout(this.retry);
    const ws = this.ws;
    this.ws = null;
    ws?.close();
  }

  private lost(unproven: boolean) {
    this.host.lost(unproven);
    clearTimeout(this.retry);
    this.retry = window.setTimeout(() => void this.connect(), unproven ? 5000 : this.delay);
    this.delay = Math.min(this.delay * 2, 2000);
  }
}
