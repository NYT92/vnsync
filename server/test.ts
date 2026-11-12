import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

process.env.SECRET = "test-secret";
process.env.KEEP_SNAPSHOTS = "2";
process.env.DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), "vnsync-server-"));
const { app } = await import("./index.ts");

const auth = { authorization: "Bearer test-secret" };
const call = (method: string, url: string, body?: string, headers = {}) =>
  app.request(url, { method, body, headers: { ...auth, ...headers } });
const head = (id: string) => JSON.stringify({ headSnapshotId: id });
const HEAD = "/api/blob/games/g1/head.json";
const snap = (id: string) => `/api/blob/games/g1/snapshots/${id}.tar.zst`;

test("rejects missing or wrong token", async () => {
  assert.equal((await app.request("/api/ping")).status, 401);
  const wrong = await app.request("/api/ping", { headers: { authorization: "Bearer nope" } });
  assert.equal(wrong.status, 401);
  assert.equal((await call("GET", "/api/ping")).status, 200);
});

test("blob round-trip and invalid keys", async () => {
  assert.equal((await call("GET", "/api/blob/library/catalog.json")).status, 404);
  assert.equal((await call("PUT", "/api/blob/library/catalog.json", "hello")).status, 200);
  assert.equal(await (await call("GET", "/api/blob/library/catalog.json")).text(), "hello");
  assert.equal((await call("PUT", "/api/blob/a%5C..%5Cb", "x")).status, 400);
  assert.equal((await call("PUT", "/api/blob/c:evil", "x")).status, 400);
});

test("head compare-and-swap, history, retention", async () => {
  for (const id of ["s1", "s2", "s3"]) await call("PUT", snap(id), id);

  assert.equal((await call("PUT", HEAD, head("s1"), { "if-none-match": "*" })).status, 200);
  // A second device that also believes there is no head must lose.
  assert.equal((await call("PUT", HEAD, head("s2"), { "if-none-match": "*" })).status, 412);
  // Stale parent loses, current parent wins.
  assert.equal((await call("PUT", HEAD, head("s2"), { "if-match": "s0" })).status, 412);
  assert.equal((await call("PUT", HEAD, head("s2"), { "if-match": "s1" })).status, 200);
  // No precondition = forced overwrite.
  assert.equal((await call("PUT", HEAD, head("s3"))).status, 200);
  assert.equal((await call("PUT", HEAD, "not json")).status, 400);

  const history = await (await call("GET", "/api/games/g1/history")).json();
  assert.deepEqual(history.map((h: any) => h.headSnapshotId), ["s3", "s2"]);
  assert.equal((await call("GET", snap("s1"))).status, 404); // pruned, KEEP_SNAPSHOTS=2
  assert.equal((await call("GET", snap("s3"))).status, 200);
});
