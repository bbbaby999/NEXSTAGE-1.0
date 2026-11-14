import { useEffect, useMemo, useRef, useState } from "react";
import { useInputDevice } from "../lib/inputDevice";
import { onPadButton } from "../lib/nav";
import { takeClosedGame } from "../lib/returnFocus";
import { open } from "@tauri-apps/plugin-dialog";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import { availableGenres, canonGenres, matchesGenre } from "../lib/genres";
import Button from "../components/Button";
import GameCard from "../components/GameCard";
import GameForm from "../components/GameForm";
import FoldersPanel from "../components/FoldersPanel";
import SteamImport from "../components/SteamImport";
import Seg from "../components/Seg";
import { GLYPHS } from "../components/Glyph";
import Select from "../components/Select";
import Skeleton from "../components/Skeleton";
import { PLATFORMS } from "../types";
import type { Game, GameCollection } from "../types";
import { t, tk } from "../lib/i18n";

type SortKey = "recent" | "title" | "release" | "playtime" | "genre" | "platform";
type Dir = "asc" | "desc";
const DEFAULT_DIR: Record<SortKey, Dir> = { recent: "desc", title: "asc", release: "desc", playtime: "desc", genre: "asc", platform: "asc" };
const SORT_OPTS: [string, SortKey][] = [[tk("Recentes"), "recent"], [tk("Título"), "title"], [tk("Tempo jogado"), "playtime"], [tk("Lançamento"), "release"], [tk("Gênero"), "genre"], [tk("Plataforma"), "platform"]];
const DIR_LABEL: Record<SortKey, Record<Dir, string>> = {
  recent: { asc: tk("Jogados há mais tempo primeiro"), desc: tk("Jogados recentemente primeiro") },
  title: { asc: tk("A → Z"), desc: tk("Z → A") },
  release: { asc: tk("Mais antigos primeiro"), desc: tk("Mais novos primeiro") },
  playtime: { asc: tk("Menos jogados primeiro"), desc: tk("Mais jogados primeiro") },
  genre: { asc: tk("A → Z"), desc: tk("Z → A") },
  platform: { asc: tk("PC → Nintendinho"), desc: tk("Nintendinho → PC") },
};
/** Formatos do "Adicionar jogo": executáveis (jogo de PC) e arquivos iniciados por emulador. */
const EXE_EXTS = ["exe", "bat", "cmd"];
const FILE_EXTS = ["iso", "bin", "cue", "nsp", "xci", "nup", "pkg", "vpk"];
type NewGame = { exePath?: string; isoPath?: string; platform?: string | null; emulatorId?: number | null };

const byTitle = (a: Game, b: Game) => a.title.localeCompare(b.title, "pt");
/** Posição na lista oficial de plataformas; valores antigos (ex.: RetroArch) ficam no fim. */
const platRank = (p: string) => { const i = PLATFORMS.indexOf(p); return i < 0 ? 1000 : i; };
const primaryGenre = (g: Game) => canonGenres(g.genres ?? [])[0] ?? null;

/** Comparador por campo. Jogos sem o dado (data/gênero) ficam sempre no fim, em qualquer direção. */
function compare(key: SortKey, dir: Dir) {
  const s = dir === "asc" ? 1 : -1;
  const text = (x: string | null, y: string | null, a: Game, b: Game) => {
    if (!x && !y) return byTitle(a, b);
    if (!x) return 1;
    if (!y) return -1;
    return s * x.localeCompare(y, "pt") || byTitle(a, b);
  };
  return (a: Game, b: Game) => {
    switch (key) {
      case "title": return s * byTitle(a, b);
      case "recent": return text(a.lastPlayedAt, b.lastPlayedAt, a, b); // nunca jogados ficam no fim
      case "playtime": return s * (a.playtimeSeconds - b.playtimeSeconds) || byTitle(a, b);
      case "release": return text(a.releaseIso, b.releaseIso, a, b);
      case "genre": return text(primaryGenre(a), primaryGenre(b), a, b);
      case "platform": return s * (platRank(a.platform) - platRank(b.platform)) || a.platform.localeCompare(b.platform, "pt") || byTitle(a, b);
    }
  };
}

/** Agrupa mantendo a ordem já aplicada à lista. */
function groupBy(list: Game[]): [string, Game[]][] {
  const m = new Map<string, Game[]>();
  for (const g of list) { if (!m.has(g.platform)) m.set(g.platform, []); m.get(g.platform)!.push(g); }
  return [...m.entries()];
}

export default function Library() {
  const { profile, setOpenGameId, toast } = useSession();
  const p = profile!;
  const { games, loading, reload } = useLibrary(p.id);
  const [q, setQ] = useState("");
  const [newGame, setNewGame] = useState<NewGame | null>(null);
  const [panel, setPanel] = useState(false);
  const [steamImport, setSteamImport] = useState(false);
  const [sort, setSort] = useState<{ key: SortKey; dir: Dir }>({ key: "title", dir: "asc" });
  const [genre, setGenre] = useState("");
  const [platform, setPlatform] = useState("");
  // Instalação ("" | "installed" | "not_installed") e origem ("" | "steam" | "other"), salvos por perfil
  const [install, setInstall] = useState("");
  const [origin, setOrigin] = useState("");
  const [progress, setProgress] = useState<string | null>(null);

  // Ordenação e filtro de plataforma lembrados por perfil. `prefsReady` indica que já foram aplicados
  // (o foco inicial espera por isso, para cair no primeiro jogo da ordem real e não da padrão).
  const [prefsReady, setPrefsReady] = useState(false);
  useEffect(() => {
    const s = api.getProfileSetting(p.id, "library_sort").then((v) => {
      const [k, d] = (v ?? "").split(":");
      if (k in DEFAULT_DIR && (d === "asc" || d === "desc")) setSort({ key: k as SortKey, dir: d });
    }).catch(() => {});
    const pl = api.getProfileSetting(p.id, "library_platform").then((v) => setPlatform(v ?? "")).catch(() => {});
    api.getProfileSetting(p.id, "library_install").then((v) => setInstall(v ?? "")).catch(() => {});
    api.getProfileSetting(p.id, "library_origin").then((v) => setOrigin(v ?? "")).catch(() => {});
    Promise.all([s, pl]).then(() => setPrefsReady(true));
  }, [p.id]);
  const applySort = (key: SortKey, dir: Dir) => {
    setSort({ key, dir });
    api.setProfileSetting(p.id, "library_sort", `${key}:${dir}`).catch(() => {});
  };

  const applyPlatform = (v: string) => {
    setPlatform(v);
    api.setProfileSetting(p.id, "library_platform", v).catch(() => {});
  };
  const platforms = useMemo(() => {
    const c = new Map<string, number>();
    games.forEach((g) => c.set(g.platform, (c.get(g.platform) ?? 0) + 1));
    return [...c.entries()].sort((a, b) => platRank(a[0]) - platRank(b[0]) || a[0].localeCompare(b[0], "pt"));
  }, [games]);
  useEffect(() => { if (platform && games.length && !platforms.some(([x]) => x === platform)) applyPlatform(""); }, [platforms]); // eslint-disable-line react-hooks/exhaustive-deps

  // Filtro por coleção (Favoritos ou coleção do perfil).
  const [cols, setCols] = useState<GameCollection[]>([]);
  const [collection, setCollection] = useState("");
  useEffect(() => { api.listCollections(p.id).then(setCols).catch(() => {}); }, [p.id]);
  useEffect(() => { if (collection && collection !== "fav" && cols.length && !cols.some((c) => String(c.id) === collection)) setCollection(""); }, [cols, collection]);
  const inCollection = useMemo(() => {
    if (!collection || collection === "fav") return null;
    return new Set(cols.find((c) => String(c.id) === collection)?.gameIds ?? []);
  }, [cols, collection]);

  // Controle: R2/RT abre o filtro de plataformas e L2/LT o de gêneros (só sem janela aberta por cima).
  const device = useInputDevice();
  const pad = device !== "keyboard";
  const genreOpen = useRef<(() => void) | null>(null);
  const platformOpen = useRef<(() => void) | null>(null);
  // X/□ (Y no Nintendo) troca a ordenação em ciclo: Recentes → Título → Tempo jogado → Lançamento → …
  const sortRef = useRef(sort);
  sortRef.current = sort;
  const cycleSort = useRef<() => void>(() => {});
  cycleSort.current = () => {
    if (!games.length) return;
    const i = SORT_OPTS.findIndex(([, k]) => k === sortRef.current.key);
    const [label, key] = SORT_OPTS[(i + 1) % SORT_OPTS.length];
    applySort(key, DEFAULT_DIR[key]);
    toast(t("Ordenado por {v0} · {v1}", { v0: t(label).toLowerCase(), v1: t(DIR_LABEL[key][DEFAULT_DIR[key]]) }));
  };
  useEffect(() => onPadButton((b, e) => {
    if (e.defaultPrevented || (b !== "l2" && b !== "r2" && b !== "x")) return;
    if (document.querySelector(".overlay, .pick-root, .kb-root, .sm-root:not(.leaving), .ctx-root")) return;
    if (b === "x") { e.preventDefault(); cycleSort.current(); return; }
    const fn = (b === "r2" ? platformOpen : genreOpen).current;
    if (fn) { e.preventDefault(); fn(); }
  }), []);

  const genres = useMemo(() => availableGenres(games), [games]);
  // Se o gênero escolhido deixou de existir (ex.: jogo editado), volta para "todos".
  useEffect(() => { if (genre && !genres.some(([g]) => g === genre)) setGenre(""); }, [genres, genre]);

  const applyInstall = (v: string) => { setInstall(v); api.setProfileSetting(p.id, "library_install", v).catch(() => {}); };
  const applyOrigin = (v: string) => { setOrigin(v); api.setProfileSetting(p.id, "library_origin", v).catch(() => {}); };
  const counts = useMemo(() => ({
    installed: games.filter((g) => g.installed).length,
    steam: games.filter((g) => g.steamAppId !== null).length,
  }), [games]);

  // Filtro de gênero + busca + ordenação funcionam juntos.
  const list = useMemo(
    () => games
      .filter((g) => matchesGenre(g, genre) && (!platform || g.platform === platform)
        && (collection !== "fav" || g.favorite) && (!inCollection || inCollection.has(g.id))
        && (!install || (install === "installed") === g.installed)
        && (!origin || (origin === "steam") === (g.steamAppId !== null))
        && g.title.toLowerCase().includes(q.trim().toLowerCase()))
      .sort(compare(sort.key, sort.dir)),
    [games, q, sort, genre, platform, collection, inCollection, install, origin]
  );

  // Foco inicial ao entrar na Biblioteca (controle/teclado): primeiro jogo da lista, ou o jogo cuja
  // página acabou de ser fechada. Uma vez por visita; não rouba o foco de uma janela aberta por cima.
  const focused = useRef(false);
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  useEffect(() => {
    if (focused.current || loading || !prefsReady) return;
    focused.current = true;
    const back = takeClosedGame();
    requestAnimationFrame(() => {
      if (!alive.current || document.querySelector(".overlay, .pick-root, .kb-root, .ctx-root")) return;
      const target = back !== null ? document.querySelector<HTMLElement>(`.main .grid .card[data-game-id="${back}"]`) : null;
      // sem jogo para devolver, lib/pageFocus.ts decide: posição da última visita, senão o 1º jogo
      if (target) target.focus();
    });
  }, [loading, prefsReady]);

  // Só jogos de PC são consultados na Steam (outras plataformas não existem lá).
  const toClassify = games.filter((g) => g.platform === "PC" && (g.genres ?? []).length === 0);
  const needsData = (sort.key === "release" && games.some((g) => !g.releaseIso)) || (sort.key === "genre" && games.some((g) => !primaryGenre(g)));

  /** "Adicionar jogo": seletor do Windows com todos os formatos reconhecidos. O arquivo é identificado pela
   *  mesma detecção da varredura de pastas (plataforma, emulador compatível, duplicidade) e só referenciado
   *  (nunca copiado, movido ou aberto). Executável → jogo de PC; demais → arquivo iniciado por emulador. */
  async function addGame() {
    try {
      const sel = await open({ multiple: false, directory: false, title: t("Escolha o jogo"), filters: [
        { name: t("Jogos (todos os formatos)"), extensions: [...EXE_EXTS, ...FILE_EXTS] },
        { name: t("Executáveis"), extensions: EXE_EXTS },
        { name: t("Imagens de disco (ISO, BIN, CUE)"), extensions: ["iso", "bin", "cue"] },
        { name: "Nintendo Switch (NSP, XCI, NUP)", extensions: ["nsp", "xci", "nup"] },
        { name: t("Pacotes de PS3/PS4 e PS Vita (PKG, VPK)"), extensions: ["pkg", "vpk"] },
      ] });
      if (typeof sel !== "string") return;
      const it = await api.inspectGameFile(p.id, sel);
      if (it.status === "duplicate") { toast(t("Este jogo já está na sua biblioteca."), "err"); return; }
      if (it.status === "invalid") { toast(t(it.note), "err"); return; }
      if (it.kind === "windows") { setNewGame({ exePath: sel }); return; }
      if (it.kind === "pkg" || it.kind === "vpk") toast(t("Pacotes .pkg e .vpk precisam ser instalados no emulador antes de jogar."));
      setNewGame({ isoPath: sel, platform: it.platform, emulatorId: it.assigned });
    } catch (e) { toast(errMsg(e), "err"); }
  }

  async function classify() {
    let up = 0, unc = 0, skip = 0, err = 0;
    for (let i = 0; i < toClassify.length; i++) {
      setProgress(`${i + 1}/${toClassify.length}`);
      try {
        const r = await api.autoClassify(p.id, toClassify[i].id, toClassify[i].title);
        if (r.status === "updated") up++; else if (r.status === "uncertain") unc++; else skip++;
      } catch { err++; }
      await new Promise((r) => setTimeout(r, 400));
    }
    setProgress(null);
    await reload();
    toast(t("{up} classificados pela Steam · {unc} sem correspondência segura (classifique manualmente){v2}{v3}.", { up, unc, v2: skip ? ` · ${skip} já classificados` : "", v3: err ? ` · ${err} com erro de rede` : "" }), up || !err ? "ok" : "err");
  }

  if (loading) return <Skeleton count={12} />;

  return (
    <>
      <div className="page-head">
        <h1>{t("Biblioteca")}</h1>
        <div className="row">
          <input className="search" placeholder={t("Buscar por título")} value={q} onChange={(e) => setQ(e.target.value)} />
          <Button onClick={() => setPanel(true)}>{t("Pastas de jogos")}</Button>
          <Button onClick={() => setSteamImport(true)}>{t("Importar da Steam")}</Button>
          <Button variant="primary" onClick={addGame}>{t("Adicionar jogo")}</Button>
        </div>
      </div>

      {games.length > 0 && (
        <div className="sortbar">
          <span className="muted">{t("Ordenar por")}{pad && <> <kbd>{GLYPHS[device].x}</kbd></>}</span>
          <Seg<SortKey> value={sort.key} onChange={(k) => applySort(k, DEFAULT_DIR[k])} opts={SORT_OPTS.map(([l, k]): [string, SortKey] => [t(l), k])} />
          <Button onClick={() => applySort(sort.key, sort.dir === "asc" ? "desc" : "asc")}>⇅ {t(DIR_LABEL[sort.key][sort.dir])}</Button>
        </div>
      )}
      {games.length > 0 && (
        // Filtros numa linha só, na ordem da navegação: Gênero → Plataforma → Coleção.
        <div className="filterbar">
          <span className="muted">{t("Gênero")}{pad && <> <kbd>{GLYPHS[device].l2}</kbd></>}</span>
          <Select<string> value={genre} width={220} title={t("Filtrar por gênero")} onChange={setGenre} openRef={genreOpen}
            options={[[t("Todos os gêneros"), ""], ...genres.map(([g, n]): [string, string] => [`${t(g)} (${n})`, g])]} />
          {genre && <Button variant="ghost" onClick={() => setGenre("")}>{t("✕ Mostrar todos")}</Button>}
          <span className="muted">{t("Plataforma")}{pad && <> <kbd>{GLYPHS[device].r2}</kbd></>}</span>
          <Select<string> value={platform} width={220} title={t("Filtrar por plataforma")} onChange={applyPlatform} openRef={platformOpen}
            options={[[t("Todas as plataformas"), ""], ...platforms.map(([x, n]): [string, string] => [`${x} (${n})`, x])]} />
          {platform && <Button variant="ghost" onClick={() => applyPlatform("")}>{t("✕ Todas")}</Button>}
          <span className="muted">{t("Coleção")}</span>
          <Select<string> value={collection} width={220} title={t("Filtrar por coleção")} onChange={setCollection}
            options={[[t("Todas as coleções"), ""], [t("★ Favoritos"), "fav"], ...cols.map((c): [string, string] => [`${c.name} (${c.gameIds.length})`, String(c.id)])]} />
          {collection && <Button variant="ghost" onClick={() => setCollection("")}>{t("✕ Todas")}</Button>}
          <span className="muted">{t("Instalação")}</span>
          <Select<string> value={install} width={200} title={t("Filtrar por instalação")} onChange={applyInstall}
            options={[[t("Instalados e não instalados"), ""], [t("Instalados ({n})", { n: counts.installed }), "installed"], [t("Não instalados ({n})", { n: games.length - counts.installed }), "not_installed"]]} />
          {install && <Button variant="ghost" onClick={() => applyInstall("")}>{t("✕ Todos")}</Button>}
          <span className="muted">{t("Origem")}</span>
          <Select<string> value={origin} width={200} title={t("Filtrar por origem")} onChange={applyOrigin}
            options={[[t("Todas as origens"), ""], [t("Jogos da Steam ({n})", { n: counts.steam }), "steam"], [t("Outros jogos ({n})", { n: games.length - counts.steam }), "other"]]} />
          {origin && <Button variant="ghost" onClick={() => applyOrigin("")}>{t("✕ Todas")}</Button>}
        </div>
      )}
      {(toClassify.length > 0 || needsData) && (
        <div className="row" style={{ marginBottom: 16 }}>
          <span className="muted">
            {toClassify.length > 0
              ? t("{length} jogo(s) de PC sem gênero. A Steam só é usada quando o nome confere com segurança.", { length: toClassify.length })
              : t("Jogos sem esse dado aparecem no fim. Edite descrição e gêneros na página do jogo.")}
          </span>
          {toClassify.length > 0 && (
            <Button disabled={progress !== null} onClick={classify}>{progress ? t("Classificando {progress}…", { progress }) : t("Classificar gêneros pela Steam")}</Button>
          )}
        </div>
      )}

      {games.length === 0 ? (
        <div className="empty">
          <p>{t("Nenhum jogo ainda. Adicione o executável de um jogo ou de um emulador.")}</p>
          <Button variant="primary" onClick={addGame}>{t("Adicionar jogo")}</Button>
        </div>
      ) : list.length === 0 ? (
        <div className="empty">
          <p>{genre || platform || collection
            ? (q ? t("Nenhum jogo com os filtros escolhidos para “{q}”.", { q }) : t("Nenhum jogo com os filtros escolhidos."))
            : t("Nenhum jogo encontrado para “{q}”.", { q })}</p>
          {(genre || platform || collection) && (
            <Button onClick={() => { setGenre(""); applyPlatform(""); setCollection(""); }}>{t("Mostrar todos os jogos")}</Button>
          )}
        </div>
      ) : (
        sort.key === "platform" ? (
          // Agrupado por plataforma (na ordem escolhida), com título e contagem de cada grupo.
          groupBy(list).map(([name, items]) => (
            <section key={name} className="plat-group">
              <h2 className="section">{name} <small>{items.length}</small></h2>
              <div className="grid">{items.map((g, i) => <GameCard key={g.id} game={g} index={i} entry={i === 0} onOpen={(x) => setOpenGameId(x.id)} />)}</div>
            </section>
          ))
        ) : (
          <div className="grid">{list.map((g, i) => <GameCard key={g.id} game={g} index={i} entry={i === 0} onOpen={(x) => setOpenGameId(x.id)} />)}</div>
        )
      )}

      {steamImport && <SteamImport onClose={() => setSteamImport(false)} onImported={reload} />}
      {panel && <FoldersPanel profileId={p.id} onClose={() => setPanel(false)} onChanged={reload} />}
      {newGame && <GameForm profileId={p.id} {...newGame} onClose={() => setNewGame(null)} onSaved={() => { setNewGame(null); reload(); }} />}
    </>
  );
}
