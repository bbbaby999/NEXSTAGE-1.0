// Roda todos os testes de frontend (scripts/test-*.mjs). Uso: npm test
import { spawnSync } from "node:child_process";
import fs from "node:fs";

const files = fs.readdirSync("scripts").filter((f) => /^test-.*\.mjs$/.test(f)).sort();
let failed = 0;
for (const f of files) {
  const r = spawnSync(process.execPath, ["--experimental-strip-types", "--no-warnings", "--import", "./scripts/ts-register.mjs", `scripts/${f}`], { encoding: "utf8" });
  const ok = r.status === 0;
  if (!ok) failed++;
  console.log(`${ok ? "✓" : "✗"} ${f}`);
  if (!ok) console.log((r.stdout + r.stderr).split("\n").filter((l) => /FALHOU|Error|error/.test(l)).slice(0, 8).join("\n"));
}
console.log(`\n${files.length - failed}/${files.length} arquivos de teste passaram`);
process.exit(failed ? 1 : 0);
