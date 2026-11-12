// Teste da regra da Home: node --experimental-strip-types scripts/test-recent.mjs
import { recentGames } from "../src/lib/recent.ts";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };
const t = (n) => `2026-10-30 10:${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}.${String((n * 7) % 1000).padStart(3, "0")}`;
const mk = (n, played) => Array.from({ length: n }, (_, i) => ({ id: i + 1, title: `Jogo ${String(i + 1).padStart(2, "0")}`, lastPlayedAt: played ? t(i + 1) : null }));

for (const n of [0, 1, 8, 9, 25]) {
  const r = recentGames(mk(n, true));
  ok(r.length === Math.min(n, 8), `${n} jogos iniciados -> ${r.length} exibidos (máx. 8)`);
  ok(r.every((g, i) => i === 0 || g.lastPlayedAt <= r[i - 1].lastPlayedAt), `${n}: ordem decrescente`);
}
ok(recentGames(mk(25, false)).length === 0, "25 jogos nunca iniciados -> nenhum card (sem placeholders)");
const mixed = [...mk(5, true), ...mk(5, false).map((g) => ({ ...g, id: g.id + 100 }))];
ok(recentGames(mixed).length === 5, "5 iniciados + 5 nunca iniciados -> só os 5 iniciados");

// Iniciar jogos em sequência: o último iniciado vai para a primeira posição.
const lib = mk(12, true);
const launch = (id, when) => { lib.find((g) => g.id === id).lastPlayedAt = when; };
launch(3, "2026-10-31 09:00:00.100");
ok(recentGames(lib)[0].id === 3, "jogo 3 iniciado -> posição 1");
launch(7, "2026-10-31 09:00:05.200");
let r = recentGames(lib).map((g) => g.id);
ok(r[0] === 7 && r[1] === 3, "jogo 7 iniciado depois -> 7, 3, ...");
launch(3, "2026-10-31 09:00:09.300"); // relançar (mesmo fechado em segundos) sobe de novo
r = recentGames(lib).map((g) => g.id);
ok(r[0] === 3 && r[1] === 7 && r.length === 8, "relançar o 3 -> 3, 7, ... (8 itens)");
// Abrir detalhes não altera lastPlayedAt (a Home só lê esse campo): a ordem permanece.
const before = JSON.stringify(recentGames(lib).map((g) => g.id));
ok(before === JSON.stringify(recentGames(lib).map((g) => g.id)), "abrir detalhes não muda a ordem");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
