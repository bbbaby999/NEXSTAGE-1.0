// node --experimental-strip-types scripts/test-version-iso.mjs
import fs from "node:fs";
import { APP_VERSION, VERSION_LABEL } from "../src/lib/version.ts";
import { isIsoPath, titleFromPath, fandomSearchUrl } from "../src/lib/iso.ts";
let fails = 0;
const ok = (c, m) => { if (!c) { fails++; console.log("FALHOU:", m); } else console.log("ok:", m); };

const pkg = JSON.parse(fs.readFileSync("package.json", "utf8")).version;
const tauri = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8")).version;
const build = fs.readFileSync("src-tauri/build.rs", "utf8");
ok(pkg === APP_VERSION && tauri === "../package.json" && build.includes("../package.json"), `versão técnica ${APP_VERSION} só no package.json (tauri.conf.json, build.rs e version.ts leem dele)`);
ok(VERSION_LABEL === APP_VERSION && !/beta/i.test(VERSION_LABEL), "rótulo exibido = só o número da versão (sem selo Beta desde a 1.1.0)");
const conf = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
ok(conf.app.windows[0].visible === false, "janela criada oculta (o backend exibe conforme o modo de abertura)");

ok(isIsoPath("D:\\Jogos\\Gran Turismo.ISO") && !isIsoPath("D:\\Jogos\\jogo.exe") && !isIsoPath("a.iso.zip"), "só arquivos .iso (sem diferenciar maiúsculas)");
ok(titleFromPath("D:\\Roms\\God of War II (USA).iso") === "God of War II (USA)", "título inicial = nome do arquivo sem .iso");
ok(titleFromPath("/mnt/x/Jogo_Legal.ISO") === "Jogo_Legal", "título com caminho tipo Unix e extensão em maiúsculas");
ok(titleFromPath("E:\\Roms\\Zelda [0100].nsp") === "Zelda [0100]" && titleFromPath("C:\\Jogos\\Hades\\Hades.exe") === "Hades", "título sem a extensão em outros formatos (NSP, EXE)");
ok(titleFromPath("") === "", "sem arquivo: título vazio");
ok(fandomSearchUrl("Elden Ring: Nightreign") === "https://www.fandom.com/?s=Elden+Ring%3A+Nightreign", "busca do Fandom com título codificado");
ok(fandomSearchUrl("Pokémon & Co").includes("Pok%C3%A9mon+%26+Co"), "acentos e & codificados");
console.log(fails ? `\n${fails} falha(s)` : "\nTodos os testes passaram");
process.exit(fails ? 1 : 0);
