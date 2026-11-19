import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { WebSocket } from "ws";

const PORT = 47831;
const relay = spawn(process.execPath, ["relay.mjs"], {
	env: { ...process.env, PORT: String(PORT) },
	stdio: ["ignore", "pipe", "pipe"],
});
relay.stdout.on("data", (d) => process.stdout.write("[relay] " + d));
relay.stderr.on("data", (d) => process.stderr.write("[relay:err] " + d));

const base = "ws://127.0.0.1:" + PORT + "/ws?v=2&serverId=agt_test123456";
function wait(ms) {
	return delay(ms);
}
function open(url) {
	return new Promise((resolve, reject) => {
		const ws = new WebSocket(url);
		ws.once("open", () => resolve(ws));
		ws.once("error", reject);
	});
}
function next(ws, timeout = 3000) {
	return new Promise((resolve, reject) => {
		const timer = setTimeout(() => reject(new Error("timeout")), timeout);
		ws.once("message", (data, isBinary) => {
			clearTimeout(timer);
			resolve({ data: isBinary ? data : data.toString(), isBinary });
		});
	});
}
function fail(message) {
	console.error("FAIL:", message);
	relay.kill("SIGKILL");
	process.exit(1);
}

try {
	await wait(400);
	const control = await open(base + "&role=server");
	const sync = JSON.parse((await next(control)).data);
	if (sync.type !== "sync" || !Array.isArray(sync.connectionIds))
		fail("control did not receive sync: " + JSON.stringify(sync));

	const client = await open(base + "&role=client");
	const connected = JSON.parse((await next(control)).data);
	if (connected.type !== "connected" || !connected.connectionId)
		fail("control did not receive connected: " + JSON.stringify(connected));

	const data = await open(
		base + "&role=server&connectionId=" + connected.connectionId,
	);
	client.send("hello-relay");
	const forwarded = await next(data);
	if (forwarded.data !== "hello-relay")
		fail("client -> host forwarding failed: " + forwarded.data);

	data.send(Buffer.from([1, 2, 3, 4]));
	const back = await next(client);
	if (!back.isBinary || back.data[0] !== 1 || back.data[3] !== 4)
		fail("host -> client binary forwarding failed");

	client.close();
	const disconnected = JSON.parse((await next(control)).data);
	if (
		disconnected.type !== "disconnected" ||
		disconnected.connectionId !== connected.connectionId
	)
		fail(
			"control did not receive disconnected: " + JSON.stringify(disconnected),
		);

	control.close();
	data.close();
	console.log("PASS relay protocol");
	relay.kill("SIGTERM");
	process.exit(0);
} catch (error) {
	console.error("ERROR:", error);
	relay.kill("SIGKILL");
	process.exit(1);
}
