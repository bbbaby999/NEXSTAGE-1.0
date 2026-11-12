// node --experimental-strip-types scripts/test-profile.mjs
import { computeStats, normalizeCfg, normalizeShowcase, showcaseValid } from "../src/lib/profileStats.ts";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };
const G = (platform, genres, h, last, completed = false) => ({ platform, genres, playtimeSeconds: h * 3600, lastPlayedAt: last, completed });

const s0 = computeStats([]);
ok(s0.total === 0 && s0.seconds === 0 && s0.topPlatform === null && s0.topGenre === null, "perfil sem jogos: zeros e sem plataforma/gênero (sem inventar)");

const lib = [
  G("PC", ["Ação", "Aventura"], 10, "2026-10-01 10:00:00"),
  G("PC", ["RPG", "Ação"], 5, "2026-10-02 10:00:00", true),
  G("Playstation 2", ["Terror"], 2, "2026-10-03 10:00:00"),
  G("PC", [], 0, null),
  G("PSP", ["Esportes"], 0, "2026-10-04 10:00:00"), // iniciado, 0 s registrados
];
const s = computeStats(lib);
ok(s.total === 5, "total de jogos = 5");
ok(s.completed === 1, "concluídos = 1");
ok(s.inProgress === 3, "em andamento = 3 (iniciados e não concluídos)");
ok(s.seconds === 17 * 3600, "horas jogadas somam 17 h");
ok(s.topPlatform === "PC", "plataforma mais usada = PC (por tempo)");
ok(s.topGenre === "Ação", "gênero mais jogado = Ação (15 h)");
ok(computeStats([G("PC", ["Ação"], 0, null)]).topPlatform === null, "sem tempo registrado -> sem plataforma mais usada");

const d = normalizeCfg(null);
ok(d.layout === "cinematic" && d.order.length === 3 && d.hidden.length === 0 && d.accent === null, "configuração padrão");
const c = normalizeCfg(JSON.stringify({ layout: "compact", order: ["activity", "stats"], hidden: ["favorites", "x"], accent: "#12b886" }));
ok(c.layout === "compact" && c.order.join() === "activity,stats,favorites" && c.hidden.join() === "favorites" && c.accent === "#12b886", "ordem/ocultas/cor salvas; seção ausente acrescentada; inválidas descartadas");
ok(normalizeCfg("{lixo").layout === "cinematic", "JSON inválido -> padrão");
ok(normalizeCfg(JSON.stringify({ accent: "azul" })).accent === null, "cor inválida descartada");

ok(normalizeShowcase("[3,1,2,2,99]", [1, 2, 3]).join() === "3,1,2", "favoritos: mantém ordem, remove duplicados e jogos inexistentes");
ok(normalizeShowcase("[1,2,3,4,5,6,7]", [1, 2, 3, 4, 5, 6, 7]).length === 5, "favoritos: no máximo 5");
ok(showcaseValid(0) && !showcaseValid(1) && !showcaseValid(2) && showcaseValid(3) && showcaseValid(5) && !showcaseValid(6), "regra de 3 a 5 (ou nenhum)");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
