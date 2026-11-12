import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import Modal from "./Modal";
import CandidatePicker from "./CandidatePicker";
import { api, errMsg } from "../lib/api";
import { formatDate } from "../lib/format";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, GameInfo, MetacriticCandidate, MetacriticResult } from "../types";

/** Faixas oficiais do Metascore (texto sempre junto da cor: a cor nunca é a única informação). */
export function metaBand(score: number): { cls: "good" | "mid" | "bad"; label: string } {
  if (score >= 75) return { cls: "good", label: t("Avaliações favoráveis") };
  if (score >= 50) return { cls: "mid", label: t("Avaliações mistas") };
  return { cls: "bad", label: t("Avaliações desfavoráveis") };
}
const bandCls = (s: number) => (s >= 75 ? "good" : s >= 50 ? "mid" : "bad");

/**
 * Nota do Metacritic no topo da página do jogo. Fonte principal: RAWG (nota por plataforma, jogo
 * identificado com segurança e salvo pelo ID); reserva para PC sem chave: loja da Steam. O NEXSTAGE
 * não lê o site do Metacritic. Busca sozinho uma vez por jogo e de novo quando a plataforma do jogo
 * muda em "Editar jogo"; depois usa o cache (offline).
 */
export default function MetacriticPanel({ game, info, loaded, term, onInfo }: {
  game: Game; info: GameInfo | null; loaded: boolean; term: string; onInfo: (i: GameInfo) => void;
}) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [manual, setManual] = useState("");
  const [choices, setChoices] = useState<MetacriticCandidate[] | null>(null);
  const tried = useRef<number | null>(null);
  const lastPlatform = useRef(game.platform);

  const explain = (r: MetacriticResult) =>
    r.status === "found" ? null
      : r.status === "no_score" ? t("{name} não tem nota publicada no Metacritic para esta plataforma.", { name: r.matchedName ?? game.title })
      : r.status === "no_source" ? t("Para jogos de console, a nota vem do RAWG: salve uma chave gratuita em Configurações → Pesquisa de imagens.")
      : r.status === "choose" ? t("Escolha qual destes é o seu jogo.")
      : t("Nota não encontrada para esse nome.");

  const fetchScore = async (silent: boolean, rawgId: number | null = null, reidentify = false) => {
    setBusy(true);
    try {
      const r = await api.fetchMetacritic(p.id, game.id, term || game.title, rawgId, reidentify);
      onInfo(r.info);
      setNote(explain(r));
      if (r.status === "choose") { setChoices(r.candidates); return; }
      setChoices(null);
      if (!silent) toast(r.status === "found" ? t("Nota do Metacritic atualizada.") : explain(r)!, r.status === "found" ? "ok" : "err");
    } catch (e) {
      if (!silent) toast(errMsg(e), "err");
      else setNote(errMsg(e));
    } finally { setBusy(false); }
  };

  // primeira visita: busca automática uma única vez (sem nota e nunca verificado)
  useEffect(() => {
    if (!loaded || tried.current === game.id) return;
    if (info?.metacriticScore != null || info?.metacriticCheckedAt) return;
    tried.current = game.id;
    fetchScore(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.id, loaded, info?.metacriticCheckedAt, info?.metacriticScore]);

  // plataforma trocada em "Editar jogo": o backend já aplicou a nota da versão (cache); recarrega e,
  // se não houver nota daquela versão em cache, consulta o RAWG pelo jogo já identificado.
  useEffect(() => {
    if (lastPlatform.current === game.platform) return;
    lastPlatform.current = game.platform;
    (async () => {
      const fresh = await api.getGameInfo(p.id, game.id).catch(() => null);
      if (fresh) onInfo(fresh);
      if (fresh?.metacriticSource === "manual") return;
      if (!fresh?.metacriticPlatform || fresh.metacriticScore == null) await fetchScore(true);
      else if (fresh.metacriticSource === "rawg") toast(t("Nota do Metacritic ajustada para {p}.", { p: fresh.metacriticPlatform }));
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [game.platform]);

  const score = info?.metacriticScore ?? null;
  const band = score !== null ? metaBand(score) : null;
  const src = info?.metacriticSource;
  const srcLabel = src === "steam" ? t("via loja da Steam") : src === "rawg" ? t("via RAWG") : src === "manual" ? t("informada manualmente") : null;
  const scope = info?.metacriticPlatform ?? (src === "rawg" && score !== null ? t("nota geral (sem nota própria desta versão)") : null);
  const pageUrl = info?.metacriticUrl ?? `https://www.metacritic.com/search/${encodeURIComponent(game.title)}/`;
  const others = (info?.metacriticPlatforms ?? []).filter((x) => x.name !== info?.metacriticPlatform);

  return (
    <>
      <h2 className="section mc-sec">{t("Metacritic")}</h2>
      <div className={`mc ${band?.cls ?? "none"}`}>
        <div className="mc-main">
          <b className={`mc-score ${band?.cls ?? "none"}`} aria-label={score !== null ? t("Metascore {n} de 100", { n: score }) : t("Sem Metascore")}>
            {busy && score === null ? "…" : score ?? "—"}
          </b>
          <div className="mc-text">
            <strong>{t("Metascore")}</strong>
            <span>{band ? band.label : busy ? t("Buscando a nota…") : t("Sem nota disponível")}</span>
            <small>
              {[scope, srcLabel].filter(Boolean).join(" · ")}
              {info?.metacriticCheckedAt && <>{scope || srcLabel ? " · " : ""}{t("verificado em {d}", { d: formatDate(info.metacriticCheckedAt) })}</>}
            </small>
          </div>
        </div>
        {score !== null && <div className="mc-bar" aria-hidden="true"><i style={{ width: `${score}%` }} /></div>}
        {others.length > 0 && (
          <div className="mc-others" aria-label={t("Outras versões")}>
            <small>{t("Outras versões")}:</small>
            {others.map((o) => <span key={o.slug} className={`mc-chip ${bandCls(o.score)}`}>{o.name} <b>{o.score}</b></span>)}
          </div>
        )}
        {note && score === null && <p className="muted note">{note}</p>}
        <div className="row">
          <Button onClick={() => openUrl(pageUrl).catch(() => {})}>{info?.metacriticUrl ? t("Ver no Metacritic") : t("Procurar no Metacritic")}</Button>
          <Button variant="ghost" disabled={busy} onClick={() => fetchScore(false)}>{busy ? t("Buscando…") : t("Atualizar nota")}</Button>
          {info?.rawgId && <Button variant="ghost" disabled={busy} onClick={() => fetchScore(false, null, true)}>{t("Não é este jogo?")}</Button>}
          <Button variant="ghost" onClick={() => { setManual(score !== null ? String(score) : ""); setEditing(true); }}>{t("Informar manualmente")}</Button>
        </div>
      </div>

      {choices && (
        <CandidatePicker title={t("Qual destes é o seu jogo?")} onClose={() => setChoices(null)}
          intro={t("Encontramos mais de um jogo parecido com “{name}” no RAWG. A escolha fica salva e as próximas atualizações usam este jogo.", { name: term || game.title })}
          onPick={(id) => { setChoices(null); fetchScore(false, Number(id)); }}
          items={choices.map((c) => ({
            id: c.id, title: `${c.name}${c.year ? ` (${c.year})` : ""}`, image: c.image,
            subtitle: c.platforms.slice(0, 5).join(" · ") || t("Plataformas não informadas"),
            warning: c.onPlatform ? null : t("Não consta para {p} no RAWG", { p: game.platform }),
            side: <b className={`mc-score small ${c.metacritic !== null ? bandCls(c.metacritic) : "none"}`}>{c.metacritic ?? "—"}</b>,
          }))} />
      )}

      {editing && (
        <Modal title={t("Nota do Metacritic")} onClose={() => setEditing(false)}>
          <p className="muted">{t("Use quando o jogo não for encontrado automaticamente. Deixe vazio para remover a nota.")}</p>
          <div className="fields">
            <label>{t("Metascore (0 a 100)")}<input type="number" min="0" max="100" step="1" value={manual} onChange={(e) => setManual(e.target.value)} /></label>
          </div>
          <div className="row end">
            <Button variant="ghost" onClick={() => setEditing(false)}>{t("Cancelar")}</Button>
            <Button variant="primary" onClick={async () => {
              const v = manual.trim() === "" ? null : Math.round(Number(manual));
              if (v !== null && (!Number.isFinite(v) || v < 0 || v > 100)) { toast(t("A nota do Metacritic vai de 0 a 100."), "err"); return; }
              try { onInfo(await api.saveManualMetacritic(p.id, game.id, v)); setEditing(false); setNote(null); toast(t("Nota salva.")); }
              catch (e) { toast(errMsg(e), "err"); }
            }}>{t("Salvar")}</Button>
          </div>
        </Modal>
      )}
    </>
  );
}
