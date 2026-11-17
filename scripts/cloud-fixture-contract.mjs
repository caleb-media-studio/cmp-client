import assert from "node:assert/strict";

export function validateCloudFixtureManifest(manifest) {
 assert.equal(manifest?.fixtureVersion,1,"Fixture version 1 required");
 assert.equal(manifest.kind,"firebase-auth-emulator-postgresql",
  "Fixture MUST use disposable Firebase Auth emulator and PostgreSQL");
 assert.notEqual(manifest.firebaseProjectId,"cmp-cloud-1","Production Firebase project forbidden");
 assert.match(manifest.firebaseProjectId,/^[a-z][a-z0-9-]{4,50}$/);
 const u=new URL(manifest.origin);
 assert.equal(u.protocol,"http:","Fixture must use local HTTP only");
 assert.equal(u.hostname,"127.0.0.1","Fixture must be bound to numeric loopback");
 assert(u.port,"Fixture requires an explicit ephemeral port");
 assert.equal(u.username,"");assert.equal(u.password,"");
 assert.equal(u.pathname,"/");assert.equal(u.search,"");assert.equal(u.hash,"");
 assert.equal(manifest.origin,u.origin,"Fixture origin must be normalized");
 assert.equal(manifest.disposable,true,"Backend must certify disposable setup");
 assert.equal(manifest.realCloudHttp,true,
  "Must be real Next.js CMP Cloud API, not a synthetic/mock server");
 const names=["ownerWritable","readerReadable","outsiderPrivate","revocableMember",
              "streamWritable","revocableGrant"];
 for(const key of names) {
  assert.match(manifest.repositories?.[key]??"",/^[a-z0-9_.-]+\/[a-z0-9_.-]+$/,
   "Missing canonical disposable repository "+key);
 }
 const values=Object.values(manifest.repositories);
 assert.equal(new Set(values).size,values.length,"Fixture repos must not overlap");
 assert.notEqual(manifest.repositories.ownerWritable.split("/")[0],
  manifest.repositories.outsiderPrivate.split("/")[0],
  "Cross-tenant fixture must use two distinct owner namespaces");
 return manifest;
}
