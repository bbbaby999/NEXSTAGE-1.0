import { CSSProperties, useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSession } from "../state/session";
import { api } from "../lib/api";
import { gamePassLeaveDate } from "../lib/format";
import { rememberGamePass, takeGamePass } from "../lib/returnFocus";
import { currentLang, t } from "../lib/i18n";
import Button from "./Button";
import Seg from "./Seg";
import { tiltHandlers } from "../lib/tilt";
import type { GamePassItem, GamePassList, GamePassOverview } from "../types";

type ListKey = GamePassList;
/** Parallax 3D das capas (o mesmo da Biblioteca e dos carrosséis da Home). */
const cardTilt = tiltHandlers<HTMLButtonElement>((el) => el.querySelector<HTMLElement>(".replay-cover"));

/** Aba escolhida (mantida ao abrir a página de um jogo e voltar, enquanto o app estiver aberto). */
let lastTab: ListKey = "coming";
const AUTO_EVERY = 60 * 60 * 1000; // reavalia o cache (12 h) enquanto a Home fica aberta

const fmtDate = (s: number) => new Date(s * 1000).toLocaleDateString(currentLang(), { day: "2-digit", month: "short", year: "numeric" });
const isFuture = (s: number | null) => s !== null && s * 1000 > Date.now();

/**
 * Catálogo do PC Game Pass na Home (EXPERIMENTAL): "Em breve", "Adicionados recentemente" e "Saindo de cena".
 * Vem do catálogo público do site do Xbox (sem conta). Lê só o cache local; a atualização roda em segundo
 * plano no backend (gamepass.rs, a cada 12 h). Se o catálogo estiver indisponível e não houver cache, a
 * seção não aparece. Pode ser desligada em Configurações → Contas e plataformas → Xbox.
 * Cada jogo abre uma página própria (features/GamePassDetail.tsx); ao voltar, o foco retorna ao cartão.
 */
export default function GamePassHome() {
  const { setOpenGamePass } = useSession();
  const [ov, setOv] = useState<GamePassOverview | null>(null);
  const [tab, setTabState] = useState<ListKey>(lastTab);
  const track = useRef<HTMLDivElement>(null);
  // voltando da página de um jogo: cartão que deve receber o foco quando o carrossel aparecer
  const back = useRef<string | null>(takeGamePass());
  const setTab = (k: ListKey) => {
    lastTab = k;
    setTabState(k);
    if (track.current) track.current.scrollLeft = 0;
  };
  const openGame = (g: GamePassItem) => {
    const key = `gp-${tab}-${g.id}`;
    rememberGamePass(key);
    setOpenGamePass({ item: g, list: tab });
  };

  const load = useCallback(() => api.gamePassOverview().then(setOv).catch(() => {}), []);

  useEffect(() => {
    let alive = true;
    load();
    // só depois que a Home já apareceu (e das outras seções), e só se o cache estiver vencido
    const kick = setTimeout(() => { if (alive) api.gamePassRefresh(false).catch(() => {}); }, 5000);
    const iv = setInterval(() => api.gamePassRefresh(false).catch(() => {}), AUTO_EVERY);
    const uns: (() => void)[] = [];
    for (const ev of ["gamepass-updated", "gamepass-state"]) {
      listen(ev, () => load()).then((f) => (alive ? uns.push(f) : f()));
    }
    return () => { alive = false; clearTimeout(kick); clearInterval(iv); uns.forEach((f) => f()); };
  }, [load]);

  const lists: Record<ListKey, GamePassItem[]> = { coming: ov?.coming ?? [], added: ov?.added ?? [], leaving: ov?.leaving ?? [] };
  // saindo: quem sai primeiro vem antes (sem data prevista, no fim)
  const leaveKey = (g: GamePassItem) => gamePassLeaveDate(g.leavesOn)?.getTime() ?? Number.MAX_SAFE_INTEGER;
  const items = tab === "leaving" ? [...lists.leaving].sort((a, b) => leaveKey(a) - leaveKey(b)) : lists[tab];

  // roda do mouse sobre o carrossel rola na horizontal (como nos carrosséis da Home)
  useEffect(() => {
    const el = track.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      const max = el.scrollWidth - el.clientWidth;
      if (max <= 0) return;
      if ((e.deltaY < 0 && el.scrollLeft <= 0) || (e.deltaY > 0 && el.scrollLeft >= max - 1)) return;
      e.preventDefault();
      el.scrollBy({ left: e.deltaY * 1.4 });
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [tab, items.length]);

  // devolve o foco ao cartão que abriu a página (uma vez, quando ele existir)
  useEffect(() => {
    const key = back.current;
    if (!key || !items.length) return;
    back.current = null;
    const r = requestAnimationFrame(() => {
      const el = document.querySelector<HTMLElement>(`[data-focus-key="${CSS.escape(key)}"]`);
      el?.scrollIntoView({ block: "center", inline: "center", behavior: "auto" });
      el?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(r);
  }, [items.length]);

  if (!ov || !ov.enabled) return null;
  const empty = !ov.coming.length && !ov.added.length && !ov.leaving.length;
  // nunca buscado com sucesso: só mostra enquanto a primeira busca roda; com erro, a seção some
  if (empty && !ov.running) return null;

  const meta = (g: GamePassItem): string => {
    if (tab === "leaving") {
      const d = gamePassLeaveDate(g.leavesOn);
      return d ? t("Sai em {date}", { date: d.toLocaleDateString(currentLang(), { day: "2-digit", month: "short" }) }) : t("Sai em breve");
    }
    if (tab === "coming") {
      if (g.release === null) return t("Data a anunciar");
      if (isFuture(g.release)) return t("Lançamento: {date}", { date: fmtDate(g.release) });
    }
    return g.publisher ?? "";
  };

  return (
    <section className="gp" aria-label={t("Game Pass")} data-parallax>
      <div className="section-head">
        <h2 className="section">{t("PC Game Pass")} <span className="upd-badge likely gp-exp" title={t("Catálogo público do site do Xbox; pode mudar sem aviso.")}>{t("Experimental")}</span></h2>
        <div className="upd-tools">
          {ov.running ? <small className="upd-status"><i className="upd-spin" aria-hidden="true" />{t("Atualizando o catálogo…")}</small>
            : ov.fetchedAt ? <small className="upd-status">{t("Catálogo de {date}", { date: fmtDate(ov.fetchedAt) })}</small> : null}
          <Button variant="ghost" disabled={ov.running} onClick={() => api.gamePassRefresh(true).catch(() => {})}>{t("Atualizar")}</Button>
        </div>
      </div>
      <Seg<ListKey> value={tab} onChange={setTab}
        opts={[[t("Em breve no Game Pass"), "coming"], [t("Adicionados recentemente"), "added"], [t("Saindo de cena"), "leaving"]]} />
      {empty ? <div className="upd-skel gp-skel" role="status"><span>{t("Buscando o catálogo do Game Pass…")}</span></div>
        : items.length === 0 ? <p className="upd-none gp-none">{tab === "leaving" ? t("Nenhum jogo anunciado para sair do PC Game Pass.") : tab === "coming" ? t("Nenhum jogo anunciado para chegar ao PC Game Pass.") : t("Nenhum jogo adicionado recentemente.")}</p>
        : (
          <div className="replay-track gp-track" ref={track}>
            {items.map((g, i) => (
              <button key={g.id} data-focus data-focus-key={`gp-${tab}-${g.id}`} type="button" className="replay-card gp-card" style={{ "--i": Math.min(i, 9) } as CSSProperties}
                onMouseMove={cardTilt.onMouseMove} onMouseLeave={cardTilt.onMouseLeave}
                onClick={() => openGame(g)} aria-label={[g.title, meta(g)].filter(Boolean).join(", ")}>
                <span className="replay-inner">
                  <span className="replay-cover" style={g.image ? { backgroundImage: `url("${g.image}")` } : undefined}>
                    {!g.image && <b>{g.title.charAt(0).toUpperCase()}</b>}
                    {tab === "leaving" && <i className="gp-flag">{t("Saindo")}</i>}
                  </span>
                  <strong>{g.title}</strong>
                  <small>{meta(g)}</small>
                </span>
              </button>
            ))}
          </div>
        )}
    </section>
  );
}
