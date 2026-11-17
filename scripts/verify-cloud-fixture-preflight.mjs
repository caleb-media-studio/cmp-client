import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import {fileURLToPath} from "node:url";
import {validateCloudFixtureManifest} from "./cloud-fixture-contract.mjs";
const sample=JSON.parse(readFileSync(fileURLToPath(new URL("./fixtures/cloud-fixture.example.json",import.meta.url)),"utf8"));
validateCloudFixtureManifest(sample);
for(const bad of [
 {...sample,origin:"https://cloud.cmp.chat"},
 {...sample,origin:"http://127.0.0.2:48123"},
 {...sample,origin:"http://127.0.0.1:48123/redirect"},
 {...sample,firebaseProjectId:"cmp-cloud-1"},
 {...sample,disposable:false},
 {...sample,realCloudHttp:false},
 {...sample,repositories:{...sample.repositories,outsiderPrivate:sample.repositories.ownerWritable}},
])assert.throws(()=>validateCloudFixtureManifest(bad));
console.log("PASS Cloud fixture manifest validation: only isolated real local Cloud HTTP + distinct tenants allowed");
console.log("BLOCKED until Backend supplies CMP_CLOUD_FIXTURE_MANIFEST and six disposable OAuth refresh grants; preflight is NOT a tenant security test");
