import { randomBytes } from "node:crypto";
import http from "node:http";
import { WebSocket, WebSocketServer } from "ws";

const HOST = process.env.HOST ?? "127.0.0.1";
const PORT = Number(process.env.PORT ?? 47821);
const VERSION = "2";
const MAX_PAYLOAD = 16 * 1024 * 1024;
const MAX_BUFFERED_BYTES = 1024 * 1024;
const MAX_BUFFERED_MESSAGES = 64;

/** serverId -> { control, connectionIds:Set, data:Map } */
const servers = new Map();
/** connectionId -> { ws, serverId, data, buffer, bufferedBytes } */
const clients = new Map();

function log(...args) {
	console.log(new Date().toISOString(), ...args);
}

function sendJson(ws, value) {
	if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify(value));
}

function closeQuietly(ws, code, reason) {
	try {
		ws.close(code, reason);
	} catch {
		// already closed
	}
}

function serverState(serverId) {
	let state = servers.get(serverId);
	if (!state) {
		state = { control: null, connectionIds: new Set(), data: new Map() };
		servers.set(serverId, state);
	}
	return state;
}

const httpServer = http.createServer((req, res) => {
	if (req.url === "/healthz") {
		res.writeHead(200, { "content-type": "application/json" });
		res.end(
			JSON.stringify({
				ok: true,
				servers: servers.size,
				clients: clients.size,
			}),
		);
		return;
	}
	res.writeHead(404, { "content-type": "text/plain" });
	res.end("not found");
});

const wss = new WebSocketServer({ noServer: true, maxPayload: MAX_PAYLOAD });

httpServer.on("upgrade", (req, socket, head) => {
	let url;
	try {
		url = new URL(req.url, "http://localhost");
	} catch {
		socket.destroy();
		return;
	}
	if (url.pathname !== "/ws") {
		socket.write("HTTP/1.1 404 Not Found\r\n\r\n");
		socket.destroy();
		return;
	}
	wss.handleUpgrade(req, socket, head, (ws) => wss.emit("connection", ws, req));
});

wss.on("connection", (ws, req) => {
	ws.isAlive = true;
	ws.on("pong", () => {
		ws.isAlive = true;
	});

	const url = new URL(req.url, "http://localhost");
	const version = url.searchParams.get("v") ?? "";
	const serverId = url.searchParams.get("serverId") ?? "";
	const role = url.searchParams.get("role") ?? "";
	const connectionId = url.searchParams.get("connectionId") ?? "";

	if (
		version !== VERSION ||
		!/^agt_[A-Za-z0-9_-]{6,}$/.test(serverId) ||
		(role !== "server" && role !== "client")
	) {
		closeQuietly(ws, 1008, "invalid handshake");
		return;
	}

	if (role === "client") {
		handleClient(ws, serverId);
	} else if (connectionId) {
		handleData(ws, serverId, connectionId);
	} else {
		handleControl(ws, serverId);
	}
});

function handleControl(ws, serverId) {
	const state = serverState(serverId);
	if (state.control && state.control !== ws) {
		closeQuietly(state.control, 1000, "replaced by a new control connection");
	}
	state.control = ws;
	log("control connected", serverId);
	sendJson(ws, { type: "sync", connectionIds: [...state.connectionIds] });

	ws.on("message", (data, isBinary) => {
		if (isBinary || data.length > 128) return;
		try {
			const message = JSON.parse(data.toString());
			if (message?.type === "ping") {
				sendJson(ws, { type: "pong", ts: message.ts ?? null });
			}
		} catch {
			// control channel only carries small JSON messages
		}
	});
	ws.on("close", () => {
		if (state.control === ws) {
			state.control = null;
			log("control disconnected", serverId);
		}
	});
	ws.on("error", (error) => log("control error", serverId, error.message));
}

function handleClient(ws, serverId) {
	const state = serverState(serverId);
	if (!state.control) {
		closeQuietly(ws, 1013, "host offline");
		return;
	}

	const connectionId = randomBytes(16).toString("base64url");
	const entry = {
		ws,
		serverId,
		data: null,
		buffer: [],
		bufferedBytes: 0,
	};
	clients.set(connectionId, entry);
	state.connectionIds.add(connectionId);
	sendJson(state.control, { type: "connected", connectionId });
	log("client connected", serverId, connectionId);

	ws.on("message", (data, isBinary) => {
		if (entry.data && entry.data.readyState === WebSocket.OPEN) {
			entry.data.send(data, { binary: isBinary });
			return;
		}
		entry.buffer.push({ data, isBinary });
		entry.bufferedBytes += data.length ?? 0;
		if (
			entry.buffer.length > MAX_BUFFERED_MESSAGES ||
			entry.bufferedBytes > MAX_BUFFERED_BYTES
		) {
			closeQuietly(ws, 1009, "buffer limit exceeded");
		}
	});
	ws.on("close", () => cleanupClient(connectionId, "client closed"));
	ws.on("error", (error) => {
		log("client error", connectionId, error.message);
		cleanupClient(connectionId, "client error");
	});
}

function handleData(ws, serverId, connectionId) {
	const state = serverState(serverId);
	const entry = clients.get(connectionId);
	if (!entry || entry.serverId !== serverId) {
		closeQuietly(ws, 1013, "unknown connection");
		return;
	}

	if (entry.data && entry.data !== ws) {
		closeQuietly(entry.data, 1000, "replaced by a new data channel");
	}
	entry.data = ws;
	state.data.set(connectionId, ws);

	for (const item of entry.buffer) {
		ws.send(item.data, { binary: item.isBinary });
	}
	entry.buffer = [];
	entry.bufferedBytes = 0;

	ws.on("message", (data, isBinary) => {
		if (entry.ws.readyState === WebSocket.OPEN) {
			entry.ws.send(data, { binary: isBinary });
		}
	});
	ws.on("close", () => {
		if (state.data.get(connectionId) === ws) state.data.delete(connectionId);
		if (entry.data === ws) entry.data = null;
		cleanupClient(connectionId, "host data channel closed");
	});
	ws.on("error", (error) => {
		log("data error", connectionId, error.message);
		cleanupClient(connectionId, "host data error");
	});
}

function cleanupClient(connectionId, reason) {
	const entry = clients.get(connectionId);
	if (!entry) return;
	clients.delete(connectionId);

	const state = servers.get(entry.serverId);
	if (state) {
		state.connectionIds.delete(connectionId);
		const data = state.data.get(connectionId);
		if (data) {
			state.data.delete(connectionId);
			closeQuietly(data, 1000, "client closed");
		}
		if (entry.data === data) entry.data = null;
		sendJson(state.control, { type: "disconnected", connectionId });
	}
	closeQuietly(entry.ws, 1000, reason);
	log("client closed", connectionId, reason);
}

const heartbeat = setInterval(() => {
	for (const ws of wss.clients) {
		if (ws.isAlive === false) {
			ws.terminate();
			continue;
		}
		ws.isAlive = false;
		ws.ping();
	}
}, 30_000);
heartbeat.unref();

function shutdown(signal) {
	log("shutting down", signal);
	clearInterval(heartbeat);
	for (const ws of wss.clients) closeQuietly(ws, 1001, "server shutdown");
	httpServer.close(() => process.exit(0));
	setTimeout(() => process.exit(0), 3000).unref();
}

process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));

httpServer.listen(PORT, HOST, () => {
	log("listening on ws://" + HOST + ":" + PORT + "/ws");
});
