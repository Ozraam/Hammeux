import type { ClientMsg } from "./protocol/ClientMsg";
import type { ServerMsg } from "./protocol/ServerMsg";

interface Handlers {
  onMessage(msg: ServerMsg): void;
  onStatus(connected: boolean): void;
}

export interface Link {
  send(msg: ClientMsg): void;
}

/** WebSocket vers le serveur, avec reconnexion automatique (1 s, puis jusqu'à 15 s). */
export function connect(h: Handlers): Link {
  let delay = 1000;
  let ws: WebSocket | null = null;
  let last: ClientMsg | null = null;
  const open = () => {
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    ws = new WebSocket(`${proto}//${location.host}/ws`);
    ws.onopen = () => {
      delay = 1000;
      h.onStatus(true);
      // Après une reconnexion, le serveur doit réapprendre ce qu'on regarde.
      if (last) ws?.send(JSON.stringify(last));
    };
    ws.onmessage = (e) => h.onMessage(JSON.parse(e.data) as ServerMsg);
    ws.onclose = () => {
      h.onStatus(false);
      setTimeout(open, delay);
      delay = Math.min(delay * 2, 15000);
    };
  };
  open();
  return {
    send(msg) {
      last = msg;
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
    },
  };
}
