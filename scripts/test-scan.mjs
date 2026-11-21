// node --experimental-strip-types scripts/test-scan.mjs
import { selectable, isCompatible, compatiblePaths, counts, summarize } from "../src/lib/scanSelect.ts";
import { EMULATOR_PRESETS, ALL_FORMATS } from "../src/data/emulatorPresets.ts";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };
const mk = (path, o) => ({ path, name: path, ext: "iso", kind: "disc", platform: "Playstation 2", status: "new", note: "", size: 1, compatible: [], assigned: null, ...o });
const items = [
  mk("a.iso", { compatible: [1], assigned: 1 }),                       // PS2 com 1 emulador
  mk("b.iso", { compatible: [1, 2] }),                                  // 2 emuladores: usuário escolhe
  mk("c.iso", { compatible: [] }),                                      // sem emulador cadastrado
  mk("d.exe", { ext: "exe", kind: "windows", platform: "PC" }),         // Windows: não precisa de emulador
  mk("e.exe", { ext: "exe", kind: "windows", platform: "PC", status: "secondary" }),
  mk("f.iso", { status: "duplicate" }),
  mk("g.nsp", { status: "invalid", kind: "switch", platform: null }),
  mk("h.iso", { status: "unrecognized", platform: null }),
];
ok(items.filter(selectable).length === 5, "selecionáveis: novos e secundários (5)");
ok(!selectable(items[5]) && !selectable(items[6]) && !selectable(items[7]), "duplicado, inválido e não identificado não são selecionáveis");
ok(compatiblePaths(items).join() === "a.iso,b.iso,d.exe", "'importar todos os compatíveis' = com emulador compatível ou EXE do Windows (sem c.iso nem secundário)");
ok(isCompatible(items[3]) && !isCompatible(items[2]), "EXE é compatível sem emulador; ISO sem emulador cadastrado não");
const c = counts(items);
ok(c.total === 8 && c.new === 4 && c.secondary === 1 && c.duplicate === 1 && c.invalid === 1 && c.unrecognized === 1 && c.compatible === 3, "contagens por status");
const sum = summarize(items, new Set(["a.iso", "c.iso", "d.exe", "f.iso"]), new Map([["a.iso", 1], ["c.iso", null]]));
ok(sum.count === 3, "o duplicado marcado não entra na importação (3 de 4)");
ok(sum.byPlatform["Playstation 2"] === 2 && sum.byPlatform["PC"] === 1, "resumo por plataforma");
ok(sum.withoutEmulator === 1, "1 jogo ficará sem emulador associado (aviso na confirmação)");
const plats = ["PC", "XBOX", "Xbox 360", "Xbox Series", "Playstation 1", "Playstation 2", "Playstation 3", "Playstation 4", "Playstation 5", "PSP", "Ps Vita", "Nintendo Switch", "Nintendo Switch 2", "Wii U", "Wii", "GameCube", "Nintendo 64", "Nintendo 3DS", "Nintendo DS", "Game Boy Advance", "Game Boy Color", "Game Boy", "Dreamcast", "Mega Drive", "Super Nintendo", "Nintendinho"];
ok(EMULATOR_PRESETS.every((p) => p.platforms.every((x) => plats.includes(x)) && p.formats.every((f) => ALL_FORMATS.includes(f))), "presets usam só plataformas e formatos válidos");
ok(EMULATOR_PRESETS.filter((p) => p.formats.includes("pkg") || p.formats.includes("vpk")).every((p) => p.note), "presets com PKG/VPK avisam que precisam de instalação");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
