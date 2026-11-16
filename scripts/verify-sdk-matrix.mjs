import { readFileSync, existsSync } from "node:fs";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { dirname,join } from "node:path";

const root=join(dirname(fileURLToPath(import.meta.url)),"..");
const matrix=JSON.parse(readFileSync(join(root,"sdk/support-matrix.json"),"utf8"));
const phase1=["typescript","python","go","rust"];
const phase2=["java","kotlin","csharp","php","ruby","swift"];
assert.deepEqual(matrix.priorities.phase1,phase1);
assert.deepEqual(matrix.priorities.phase2,phase2);
assert.equal(matrix.languages.length,10);
assert.equal(new Set(matrix.languages.map(l=>l.id)).size,10);
for(const language of matrix.languages){
 assert.ok(existsSync(join(root,language.path)),"Missing SDK location: "+language.id);
 assert.ok(language.package&&language.runtime);
 assert.ok(language.requiredTests.length>=2);
 assert.equal(language.phase,phase1.includes(language.id)?1:2);
 assert.equal(language.status,language.phase===1?"implemented-development":"planned");
 if(language.phase===2)
  assert.ok(existsSync(join(root,language.path,"README.md")),
   "Planned SDK must say plainly that it is not yet supported: "+language.id);
}
console.log("PASS CMP 10-language SDK matrix, 4 implemented development clients and 6 explicit planned targets");
