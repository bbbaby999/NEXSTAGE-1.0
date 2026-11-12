// Configura as atualizações pelo app. Uso (na pasta do projeto):
//   node scripts/setup-updater.mjs SEU_USUARIO/SEU_REPOSITORIO
// Lê a chave pública gerada em %USERPROFILE%\.tauri\nexstage.key.pub e grava a configuração
// em src-tauri/tauri.conf.json (endpoint do GitHub Releases + assinatura).
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const repo = process.argv[2];
if (!repo || !/^[\w.-]+\/[\w.-]+$/.test(repo)) {
  console.error("Uso: node scripts/setup-updater.mjs USUARIO/REPOSITORIO   (ex.: joao/nexstage-releases)");
  process.exit(1);
}
const pubPath = process.argv[3] || path.join(os.homedir(), ".tauri", "nexstage.key.pub");
if (!fs.existsSync(pubPath)) {
  console.error(`Chave pública não encontrada em ${pubPath}.\nGere com: npx tauri signer generate -w "%USERPROFILE%\\.tauri\\nexstage.key"`);
  process.exit(1);
}
const pubkey = fs.readFileSync(pubPath, "utf8").trim();
const confPath = "src-tauri/tauri.conf.json";
const conf = JSON.parse(fs.readFileSync(confPath, "utf8"));
conf.bundle.createUpdaterArtifacts = true;
conf.plugins = {
  ...(conf.plugins || {}),
  updater: {
    pubkey,
    endpoints: [`https://github.com/${repo}/releases/latest/download/latest.json`],
    windows: { installMode: "passive" },
  },
};
fs.writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");
console.log(`OK: atualizações configuradas para github.com/${repo}`);
