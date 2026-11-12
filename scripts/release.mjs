// Gera o latest.json da versão atual depois do build. Uso (na pasta do projeto):
//   node scripts/release.mjs "O que mudou nesta versão"
// Saída: pasta release\ com o instalador, a assinatura e o latest.json prontos para enviar ao GitHub.
import fs from "node:fs";
import path from "node:path";

const conf = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const version = JSON.parse(fs.readFileSync("package.json", "utf8")).version; // única fonte da versão
const endpoint = conf.plugins?.updater?.endpoints?.[0] ?? "";
const m = /github\.com\/([^/]+\/[^/]+)\//.exec(endpoint);
if (!m) {
  console.error("Atualização não configurada. Rode antes: node scripts/setup-updater.mjs USUARIO/REPOSITORIO");
  process.exit(1);
}
const repo = m[1];
const dir = "src-tauri/target/release/bundle/nsis";
const exe = `NEXSTAGE_${version}_x64-setup.exe`;
const sigFile = path.join(dir, exe + ".sig");
if (!fs.existsSync(path.join(dir, exe)) || !fs.existsSync(sigFile)) {
  console.error(`Não encontrei ${exe} e/ou o .sig em ${dir}.\nRode o build com a chave de assinatura definida (veja as instruções) e confira a versão em package.json.`);
  process.exit(1);
}
const latest = {
  version,
  notes: process.argv.slice(2).join(" ") || `NEXSTAGE ${version}`,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: fs.readFileSync(sigFile, "utf8").trim(),
      url: `https://github.com/${repo}/releases/download/v${version}/${exe}`,
    },
  },
};
fs.mkdirSync("release", { recursive: true });
fs.copyFileSync(path.join(dir, exe), path.join("release", exe));
fs.copyFileSync(sigFile, path.join("release", exe + ".sig"));
fs.writeFileSync("release/latest.json", JSON.stringify(latest, null, 2));
console.log(`\nPronto! Envie estes 3 arquivos da pasta release\\ para um novo Release "v${version}" em github.com/${repo}:\n  - ${exe}\n  - ${exe}.sig\n  - latest.json\n`);
