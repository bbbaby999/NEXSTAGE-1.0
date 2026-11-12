// Confere os dicionários de src/locales: mesma quantidade de itens que keys.ts, mesmas {variáveis}
// e mesmos espaços nas pontas (ex.: " · baixa"). Uso (na raiz do projeto): node scripts/i18n/check-locales.cjs
const fs = require("fs"), path = require("path");
const dir = "src/locales";
const read = (f) => {
  const s = fs.readFileSync(path.join(dir, f), "utf8");
  const body = s.slice(s.indexOf("["), s.lastIndexOf("]") + 1);
  // eslint-disable-next-line no-new-func
  return Function(`"use strict"; return (${body});`)();
};
const keys = read("keys.ts");
const vars = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
let bad = 0;
for (const f of fs.readdirSync(dir).filter((f) => f !== "keys.ts" && f.endsWith(".ts"))) {
  const list = read(f);
  const cjk = /^(ja|ko|zh)/.test(f);
  const problems = [];
  if (list.length !== keys.length) problems.push(`quantidade: ${list.length} (esperado ${keys.length})`);
  list.forEach((tr, i) => {
    const k = keys[i];
    if (k === undefined || !tr) return;
    if (vars(k) !== vars(tr)) problems.push(`#${i} variáveis diferentes: ${JSON.stringify(k)} -> ${JSON.stringify(tr)}`);
    // japonês/coreano/chinês juntam textos sem espaço: lá a regra dos espaços nas pontas não se aplica
    if (!cjk && (/^\s/.test(k) !== /^\s/.test(tr) || /\s$/.test(k) !== /\s$/.test(tr))) problems.push(`#${i} espaços nas pontas: ${JSON.stringify(k)} -> ${JSON.stringify(tr)}`);
  });
  const empty = list.filter((x) => !x).length;
  console.log(`${problems.length ? "✗" : "✓"} ${f}: ${list.length} itens${empty ? `, ${empty} vazios (mostram o português)` : ""}`);
  problems.slice(0, 15).forEach((p) => console.log("   " + p));
  if (problems.length > 15) console.log(`   … +${problems.length - 15}`);
  bad += problems.length;
}
process.exit(bad ? 1 : 0);
