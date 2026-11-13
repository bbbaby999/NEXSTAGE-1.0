// Liga o envio de feedback do app ao serviço publicado (feedback-worker/). Uso (na pasta do projeto):
//   node scripts/setup-feedback.mjs https://nexstage-feedback.SEU-SUBDOMINIO.workers.dev
// Grava src-tauri/feedback.json, lido na compilação (build.rs). O endereço não é segredo: o token do
// GitHub fica só no Worker. Recompile o app depois (npm run tauri build).
import fs from "node:fs";

const url = (process.argv[2] || "").trim();
if (!/^https:\/\/[^\s/]+(\/\S*)?$/.test(url)) {
  console.error("Uso: node scripts/setup-feedback.mjs https://nexstage-feedback.SEU-SUBDOMINIO.workers.dev");
  process.exit(1);
}
fs.writeFileSync("src-tauri/feedback.json", JSON.stringify({ endpoint: url }, null, 2) + "\n");
console.log(`OK: feedback enviado para ${url}. Recompile o app para valer.`);
