// node --experimental-strip-types scripts/test-crop-combo-notes.mjs
import { baseScale, clampCrop, centerCrop, zoomCrop, sourceRect } from "../src/lib/crop.ts";
import { validateCombo, combosConflict, comboLabel, popcount } from "../src/lib/padCombo.ts";
import { PATCH_NOTES } from "../src/data/patchnotes.ts";
import { APP_VERSION } from "../src/lib/version.ts";
import fs from "node:fs";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };
const near = (a, b, e = 1e-6) => Math.abs(a - b) < e;

// ----- enquadramento -----
const [iw, ih, vw, vh] = [4000, 3000, 300, 300];
let c = centerCrop(iw, ih, vw, vh);
let r = sourceRect(iw, ih, vw, vh, c);
ok(near(r.sw, r.sh) && near(r.sw, 3000) && near(r.sx, 500), "avatar 1:1 de foto 4:3: recorte central quadrado (3000×3000)");
ok(near(baseScale(1000, 3000, 560, 175), 0.56), "banner 16:5 de imagem alta: escala 'cobrir' pela largura");
for (const z of [1, 1.7, 3, 4]) {
  const z0 = zoomCrop(iw, ih, vw, vh, c, z);
  const rr = sourceRect(iw, ih, vw, vh, z0);
  ok(rr.sx >= -1e-6 && rr.sy >= -1e-6 && rr.sx + rr.sw <= iw + 1e-6 && rr.sy + rr.sh <= ih + 1e-6, `zoom ${z}: quadro sempre dentro da imagem (sem bordas vazias)`);
}
const far = clampCrop(iw, ih, vw, vh, { zoom: 2, ox: 9999, oy: -9999 });
const rf = sourceRect(iw, ih, vw, vh, far);
ok(rf.sx >= -1e-6 && rf.sy + rf.sh <= ih + 1e-6, "arrastar além do limite é contido");
const z2 = zoomCrop(iw, ih, vw, vh, c, 2);
const cx = (r0) => r0.sx + r0.sw / 2;
ok(near(cx(sourceRect(iw, ih, vw, vh, z2)), cx(r), 1e-3), "zoom mantém o ponto central");
const rb = sourceRect(2000, 1000, 560, 175, centerCrop(2000, 1000, 560, 175, 1.5));
ok(near(rb.sw / rb.sh, 560 / 175, 1e-6), "proporção do recorte = proporção do quadro (sem distorcer)");

// ----- combinações do controle (regra do backend) -----
const B = (...i) => i.reduce((m, k) => m | (1 << k), 0);
const SELECT = 8, START = 9, RB = 5, A = 0, Bt = 1, X = 2;
ok(validateCombo(B(SELECT, START)) === null, "Select + Start é válida");
ok(validateCombo(B(A)) !== null, "1 botão: recusada");
ok(validateCombo(B(A, Bt)) !== null, "A + B (sem botão de menu): recusada (conflita com jogos)");
ok(validateCombo(B(A, Bt, X, START)) !== null, "4 botões: recusada");
ok(combosConflict(B(SELECT, START), B(SELECT, START, RB)) && !combosConflict(B(SELECT, START), B(SELECT, RB)), "combinações que se contêm conflitam; as demais não");
ok(comboLabel(B(SELECT, START)) === "Select (Create) + Start (Options)" && comboLabel(0) === "Não definido" && popcount(B(1, 3, 5)) === 3, "rótulos legíveis");

// ----- Patch Notes e versão -----
const pkg = JSON.parse(fs.readFileSync("package.json", "utf8")).version;
const tau = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
ok(pkg === APP_VERSION && tau === "../package.json", `versão ${APP_VERSION} vem só do package.json (tauri.conf.json e version.ts leem dele)`);
ok(PATCH_NOTES[0].version === APP_VERSION, "a primeira entrada das Patch Notes é a versão atual");
ok(PATCH_NOTES.every((n) => n.date && n.sections.length && n.sections.every((s) => s.items.length && s.items.every((t) => t.trim()))), "toda entrada tem data e seções com itens");
ok(new Set(PATCH_NOTES.map((n) => n.version)).size === PATCH_NOTES.length, "sem versões repetidas");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
