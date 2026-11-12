// node --experimental-strip-types scripts/test-genres.mjs
import { canonGenres, availableGenres, matchesGenre } from "../src/lib/genres.ts";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);

ok(eq(canonGenres(["Ação", "Aventura", "Gratuito para Jogar", "Esportes"]), ["Ação", "Aventura", "Esporte"]), "gêneros da Steam padronizados; 'Gratuito para jogar' ignorado");
ok(eq(canonGenres(["Action", "Ação"]), ["Ação"]), "en/pt equivalentes não duplicam");
ok(eq(canonGenres(["Terror"]), ["Terror"]), "gênero manual (Terror) é mantido");
ok(eq(canonGenres(["Roguelike"]), ["Roguelike"]), "gênero desconhecido é preservado como escrito");
ok(eq(canonGenres([]), []), "lista vazia");

const games = [
  { genres: ["Ação", "Aventura"] }, { genres: ["RPG", "Ação"] }, { genres: ["Corrida"] },
  { genres: ["Terror"] }, { genres: [] }, { genres: ["Esportes"] },
];
ok(games.filter((g) => matchesGenre(g, "Ação")).length === 2, "jogo com vários gêneros aparece em qualquer um deles (Ação = 2)");
ok(games.filter((g) => matchesGenre(g, "Aventura")).length === 1, "Aventura = 1");
ok(games.filter((g) => matchesGenre(g, "")).length === 6, "sem filtro: todos os jogos (6)");
ok(games.filter((g) => matchesGenre(g, "Esporte")).length === 1, "'Esportes' da Steam casa com o filtro 'Esporte'");
const av = availableGenres(games);
ok(av[0][0] === "Ação" && av[0][1] === 2, "lista de gêneros disponíveis com contagem, na ordem padrão");
ok(!av.some(([g]) => g === "Plataforma"), "gêneros sem nenhum jogo não aparecem");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
