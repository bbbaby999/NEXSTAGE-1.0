// Monta src/locales/keys.ts: todas as chaves (t/tk do front + mensagens fixas do backend), em ordem estável.
// Uso (na raiz do projeto): node scripts/i18n/build-keys.cjs
const fs = require("fs"), path = require("path"), cp = require("child_process");
const out = new Set();
const walk = (d) => fs.readdirSync(d).forEach((f) => {
  const p = path.join(d, f);
  if (fs.statSync(p).isDirectory()) { if (f !== "locales") walk(p); return; }
  if (!/\.tsx?$/.test(f)) return;
  const s = fs.readFileSync(p, "utf8");
  const re = /\bt[k]?\(\s*("(?:[^"\\]|\\.)*")/g;
  let m;
  while ((m = re.exec(s))) out.add(JSON.parse(m[1]));
});
walk("src");
// backend: mensagens fixas (sem format!)
const rs = [];
const walkRs = (d) => fs.readdirSync(d).forEach((f) => {
  const p = path.join(d, f);
  if (fs.statSync(p).isDirectory()) return walkRs(p);
  if (f.endsWith(".rs")) rs.push(fs.readFileSync(p, "utf8"));
});
walkRs("src-tauri/src");
const SKIP = new Set(["C:\\Windows\\win.ini", "DualSense", "DualSense Edge", "NEXSTAGE", "Playstation 1", "Spider-Man"]);
const reRs = /(?:Other\(|note: (?:Some\()?)"([^"{}\\]+)"|"([A-ZÀ-Ú][^"{}\\]{6,})"\.(?:into|to_string)\(\)/g;
for (const s of rs) { let m; while ((m = reRs.exec(s))) { const k = m[1] ?? m[2]; if (k && !SKIP.has(k) && /\s/.test(k)) out.add(k); } }
// rótulos de bateria vindos do backend
["Cheia", "Média", "Baixa", "Crítica"].forEach((k) => out.add(k));
// Estável: chaves que já existem mantêm a posição (as traduções são listas na mesma ordem); novas vão para o fim.
let prev = [];
try { prev = JSON.parse(fs.readFileSync("src/locales/keys.ts", "utf8").replace(/^[\s\S]*?= /, "").replace(/;\s*$/, "")); } catch {}
const keys = [...prev, ...[...out].filter((k) => !prev.includes(k))];
const unused = prev.filter((k) => !out.has(k));
if (unused.length) console.log("chaves sem uso (mantidas para não deslocar as traduções):", unused.length);
const ts = `// GERADO por scripts de i18n: lista de todas as chaves (texto em português). Não editar à mão.\nexport const KEYS: string[] = ${JSON.stringify(keys, null, 1)};\n`;
fs.mkdirSync("src/locales", { recursive: true });
fs.writeFileSync("src/locales/keys.ts", ts);
console.log("chaves:", keys.length, "| caracteres:", keys.join("").length);
