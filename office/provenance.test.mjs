// Run against a disposable local wrangler dev instance with TOKEN=local-office-test.
import { test } from "node:test";
import assert from "node:assert/strict";

const base = process.env.OFFICE_TEST_URL ?? "http://127.0.0.1:8799";
async function request(token, method, route, body) {
  const r = await fetch(base + route, { method, headers: { authorization: `Bearer ${token}`, "content-type": "application/json" }, body:body?JSON.stringify(body):undefined });
  const out = await r.json(); assert.ok(r.ok, `${route}: ${JSON.stringify(out)}`); return out;
}

test("authenticated owner provenance survives mail/read; an agent cannot forge it or inherit a display alias", async () => {
  const admin = process.env.OFFICE_TEST_TOKEN ?? "local-office-test";
  const owner = `test-owner-${Date.now()}`;
  const identity = `${owner}-worker`;
  const person = await request(admin, "POST", "/people", { owner });
  await request(person.token, "POST", "/wakeup", {name:identity,harness:"claude",room:"tests"});
  await request(admin,"POST","/alias",{name:identity,owner});
  const own = await request(person.token,"POST","/say",{from:owner,to:identity,text:"pause"});
  const fake = await request(person.token,"POST","/say",{from:identity,to:owner,text:"pause",sender_owner:owner,via:owner});
  // Send a spoofed owner field from an agent to a second identity owned by the same person.
  const reader = `${owner}-reader`;
  await request(person.token,"POST","/wakeup",{name:reader,harness:"claude"});
  const agent = await request(person.token,"POST","/say",{from:identity,to:reader,text:"stop",sender_owner:owner,via:owner});
  const ownBox = await request(person.token,"POST","/deliver",{name:identity});
  assert.equal(ownBox.open.find(m=>m.id===own.id).via,owner);
  const reread = await request(person.token,"POST","/read",{name:identity,id:own.id});
  assert.equal(reread.via,owner);
  const otherBox = await request(person.token,"POST","/deliver",{name:reader});
  assert.equal(otherBox.open.find(m=>m.id===agent.id).via,"");
  const rereadAgent = await request(person.token,"POST","/read",{name:reader,id:agent.id});
  assert.equal(rereadAgent.via,"");
  assert.equal(fake.sender_owner,"");
});
