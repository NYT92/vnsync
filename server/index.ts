import "dotenv/config";
import { createHash, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { H3, HTTPError, readBody, serve } from "h3";

const SECRET = process.env.SECRET ?? "";
if (!SECRET) throw new Error("SECRET is empty. Set it in server/.env");
const DATA_DIR = path.resolve(process.env.DATA_DIR || "./data");
const KEEP_SNAPSHOTS = Number(process.env.KEEP_SNAPSHOTS) || 20;
const MAX_BYTES = (Number(process.env.MAX_UPLOAD_MB) || 512) * 1024 * 1024;

const sha256 = (s: string) => createHash("sha256").update(s).digest();
const SECRET_HASH = sha256(SECRET);

/** Maps a storage key to a path inside DATA_DIR, rejecting anything that could escape it. */
function keyToPath(key: string): string {
  // Keys starting with "_" are reserved for server state (e.g. _server/devices.json).
  const safe =
    /^[A-Za-z0-9._/-]+$/.test(key) &&
    !key.startsWith("_") &&
    key.split("/").every((s) => s !== "" && s !== "." && s !== "..");
  const file = path.join(DATA_DIR, key);
  if (!safe || !file.startsWith(DATA_DIR + path.sep)) {
    throw new HTTPError({ status: 400, message: "Invalid key" });
  }
  return file;
}

function atomicWrite(file: string, data: Uint8Array | string) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${randomUUID()}.tmp`;
  fs.writeFileSync(tmp, data);
  fs.renameSync(tmp, file);
}

function readIfExists(file: string): Buffer | undefined {
  try {
    return fs.readFileSync(file);
  } catch {
    return undefined;
  }
}

function readHistory(gameDir: string): any[] {
  const raw = readIfExists(path.join(gameDir, "history.jsonl"))?.toString() ?? "";
  return raw.split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

/**
 * Compare-and-swap for games/{id}/head.json. Fully synchronous, so no other
 * request can interleave between the check and the rename.
 */
function writeHead(file: string, body: Buffer, headers: Headers) {
  let head: any;
  try {
    head = JSON.parse(body.toString());
  } catch {
    throw new HTTPError({ status: 400, message: "head.json is not valid JSON" });
  }
  const id = head?.headSnapshotId;
  if (typeof id !== "string" || !/^[A-Za-z0-9._-]+$/.test(id)) {
    throw new HTTPError({ status: 400, message: "Invalid headSnapshotId" });
  }

  const currentRaw = readIfExists(file);
  const current = currentRaw && JSON.parse(currentRaw.toString()).headSnapshotId;
  const ifMatch = headers.get("if-match")?.replace(/^"|"$/g, "");
  const stale =
    (headers.get("if-none-match") === "*" && current !== undefined) ||
    (ifMatch !== undefined && ifMatch !== current);
  if (stale) throw new HTTPError({ status: 412, message: "Remote head changed" });

  atomicWrite(file, body);

  // History doubles as the retention list: entries pushed out of it lose their archive.
  // ponytail: archives uploaded for a push that then got 412 are never pruned; sweep
  // snapshots/ against history when disk use matters.
  const gameDir = path.dirname(file);
  const history = readHistory(gameDir).filter((h) => h.headSnapshotId !== id);
  history.push(head);
  for (const old of history.splice(0, Math.max(0, history.length - KEEP_SNAPSHOTS))) {
    fs.rmSync(path.join(gameDir, "snapshots", `${old.headSnapshotId}.tar.zst`), { force: true });
  }
  atomicWrite(
    path.join(gameDir, "history.jsonl"),
    history.map((h) => JSON.stringify(h)).join("\n") + "\n",
  );
}

/** A signed-in PC. Only the token's hash is stored. */
interface Device {
  id: string;
  name: string;
  os: string;
  tokenHash: string;
  createdAt: string;
  lastSeenAt: string;
}

const DEVICES_FILE = path.join(DATA_DIR, "_server", "devices.json");

function readDevices(): Device[] {
  return JSON.parse(readIfExists(DEVICES_FILE)?.toString() ?? "[]");
}

function writeDevices(devices: Device[]) {
  atomicWrite(DEVICES_FILE, JSON.stringify(devices, null, 2));
}

export const app = new H3();

/**
 * Accepts the master SECRET (used to sign in, and by pre-0.2 clients) or a device token
 * issued by POST /api/sessions. Revoking a device deletes its token.
 */
app.use((event) => {
  const token = event.req.headers.get("authorization")?.replace(/^Bearer /, "") ?? "";
  if (timingSafeEqual(sha256(token), SECRET_HASH)) {
    event.context.master = true;
    return;
  }
  const hash = sha256(token).toString("hex");
  const devices = readDevices();
  const device = devices.find((d) => d.tokenHash === hash);
  if (!device) throw new HTTPError({ status: 401, message: "Invalid token" });
  event.context.deviceId = device.id;
  // ponytail: last-seen is written at most once a minute; move to SQLite if many devices sync often.
  if (Date.now() - Date.parse(device.lastSeenAt) > 60_000) {
    device.lastSeenAt = new Date().toISOString();
    writeDevices(devices);
  }
});

app.get("/api/ping", () => ({ ok: true }));

/** Sign in: trade the master SECRET for a per-device token. Re-signing in replaces the old token. */
app.post("/api/sessions", async (event) => {
  if (!event.context.master) {
    throw new HTTPError({ status: 403, message: "Sign in with the server password" });
  }
  const body = (await readBody<any>(event)) ?? {};
  const str = (v: unknown, max: number) => (typeof v === "string" ? v.trim().slice(0, max) : "");
  const id = str(body.deviceId, 64);
  if (!/^[A-Za-z0-9-]+$/.test(id)) {
    throw new HTTPError({ status: 400, message: "Invalid deviceId" });
  }
  const token = randomBytes(32).toString("base64url");
  const now = new Date().toISOString();
  const devices = readDevices().filter((d) => d.id !== id);
  devices.push({
    id,
    name: str(body.deviceName, 100) || "Unknown device",
    os: str(body.os, 32),
    tokenHash: sha256(token).toString("hex"),
    createdAt: now,
    lastSeenAt: now,
  });
  writeDevices(devices);
  return { token };
});

app.get("/api/sessions", (event) =>
  readDevices().map(({ tokenHash, ...d }) => ({ ...d, current: d.id === event.context.deviceId })),
);

/** Sign out this device, or revoke another one. */
app.delete("/api/sessions/:id", (event) => {
  const id = event.context.params!.id;
  writeDevices(readDevices().filter((d) => d.id !== id));
  return { ok: true };
});

app.get("/api/blob/**:key", (event) => {
  const data = readIfExists(keyToPath(event.context.params!.key));
  if (!data) throw new HTTPError({ status: 404, message: "Not found" });
  return new Response(data);
});

app.put("/api/blob/**:key", async (event) => {
  const key = event.context.params!.key;
  const file = keyToPath(key);
  if (Number(event.req.headers.get("content-length")) > MAX_BYTES) {
    throw new HTTPError({ status: 413, message: "Upload too large" });
  }
  const body = Buffer.from(await event.req.arrayBuffer());
  if (body.byteLength > MAX_BYTES) {
    throw new HTTPError({ status: 413, message: "Upload too large" });
  }
  if (/^games\/[^/]+\/head\.json$/.test(key)) writeHead(file, body, event.req.headers);
  else atomicWrite(file, body);
  return { ok: true };
});

app.get("/api/games/:id/history", (event) =>
  readHistory(path.dirname(keyToPath(`games/${event.context.params!.id}/head.json`))).reverse(),
);

app.delete("/api/games/:id", (event) => {
  fs.rmSync(path.dirname(keyToPath(`games/${event.context.params!.id}/head.json`)), {
    recursive: true,
    force: true,
  });
  return { ok: true };
});

if (import.meta.main) {
  serve(app, { port: Number(process.env.PORT) || 3000 });
}
