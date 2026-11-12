import { useCallback, useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import Button from "./Button";
import Modal from "./Modal";
import RaProgress from "./RaProgress";
import RaAchievementItem from "./RaAchievementItem";
import RaGameModal from "./RaGameModal";
import { TrophyIcon } from "./RaUnlockToasts";
import { api, errMsg } from "../lib/api";
import { isUnlocked, raSupported, useRaEvents } from "../lib/ra";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { Game, RaGame, RaGameDetail, RaLinkInfo } from "../types";

/**
 * Bloco "RetroAchievements" na página do jogo. Só aparece para plataformas com suporte.
 * Associação automática apenas por hash oficial; sugestões por nome precisam de confirmação.
 */
export default function RaGamePanel({ game, onChanged }: { game: Game; onChanged: () => void }) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [info, setInfo] = useState<RaLinkInfo | null>(null);
  const [detail, setDetail] = useState<RaGameDetail | null>(null);
  const [connected, setConnected] = useState<boolean | null>(null);
  const [picking, setPicking] = useState(false);
  const [all, setAll] = useState(false);
  const supported = raSupported(game.platform);

  const load = useCallback(async () => {
    if (!supported) return;
    try {
      const [acc, li] = await Promise.all([api.raGetAccount(p.id), api.raLinkStatus(p.id, game.id)]);
      setConnected(!!acc);
      setInfo(li);
      if (li.status === "linked" && li.raGame) setDetail(await api.raGame(p.id, li.raGame.id, false));
      else setDetail(null);
    } catch (e) { toast(errMsg(e), "err"); }
  }, [p.id, game.id, supported, toast]);
  useEffect(() => { load(); }, [load]);
  useRaEvents(p.id, (kind) => { if (kind === "synced") load(); });

  if (!supported) return null;

  const setLink = async (raGameId: number | null, unsupported = false, msg?: string) => {
    try {
      await api.raSetLink(p.id, game.id, raGameId, unsupported);
      if (msg) toast(msg);
      setPicking(false);
      onChanged();
      await load();
    } catch (e) { toast(errMsg(e), "err"); }
  };

  const g = detail?.game ?? info?.raGame ?? null;
  const achs = detail?.achievements ?? [];
  const preview = [...achs].sort((a, b) => Number(isUnlocked(b)) - Number(isUnlocked(a))).slice(0, 4);

  return (
    <>
      <h2 className="section ra-sec"><TrophyIcon size={14} /> {t("RetroAchievements")}</h2>
      <div className="ra-panel">
        {!info && <p className="muted">{t("Verificando compatibilidade…")}</p>}

        {info?.status === "linked" && g && (
          <>
            <div className="ra-panel-game">
              {g.iconUrl && <img src={g.iconUrl} alt="" />}
              <div><strong>{g.title}</strong><small>{g.consoleName || game.platform}</small></div>
            </div>
            {connected ? <RaProgress game={g} /> : <p className="muted">{t("Conecte sua conta em Configurações → RetroAchievements para ver seu progresso.")}</p>}
            {detail?.warning && <p className="ra-warn">{detail.warning}</p>}
            {preview.length > 0 && (
              <div className="ra-ach-list mini">{preview.map((a) => <RaAchievementItem key={a.id} a={a} showGame={false} />)}</div>
            )}
            {info.method === "manual" && <p className="muted note">{t("Associação confirmada manualmente.")}</p>}
            {info.hashMismatch && (
              <p className="ra-warn">{t("O hash deste arquivo não está entre as versões aceitas pelo RetroAchievements para este jogo: o emulador pode não registrar conquistas com ele.")}</p>
            )}
            <div className="row">
              {achs.length > 0 && <Button onClick={() => setAll(true)}>{t("Ver todas as conquistas")}</Button>}
              <Button variant="ghost" onClick={() => openUrl(g.url).catch(() => {})}>{t("Abrir no RetroAchievements")}</Button>
              <Button variant="ghost" onClick={() => setPicking(true)}>{t("Alterar associação")}</Button>
              {info.method === "manual" && <Button variant="ghost" onClick={() => setLink(null, false, t("Associação removida."))}>{t("Desfazer")}</Button>}
            </div>
          </>
        )}

        {info?.status === "suggested" && (
          <>
            <p>{t("Possível correspondência encontrada pelo nome. Confirme antes de associar:")}</p>
            {info.hash && <p className="muted note">{t("O hash deste arquivo não corresponde a nenhuma versão aceita pelo RetroAchievements; confirme apenas se tiver certeza.")}</p>}
            <div className="ra-cands">
              {info.candidates.map((c) => <Candidate key={c.id} c={c} onPick={() => setLink(c.id, false, t("Jogo associado."))} />)}
            </div>
            <div className="row">
              <Button variant="ghost" onClick={() => setPicking(true)}>{t("Procurar outro…")}</Button>
              <Button variant="ghost" onClick={() => setLink(null, true, t("Marcado como sem suporte."))}>{t("Não tem suporte")}</Button>
            </div>
          </>
        )}

        {info?.status === "none" && (
          <>
            <p className="muted">
              {info.hashError ? t("Não foi possível identificar o arquivo: {e}", { e: t(info.hashError) })
                : !game.isoPath && !game.romPath ? t("Jogo sem arquivo de disco/ROM associado: a identificação automática por hash não é possível.")
                : info.catalogReady ? t("Nenhum jogo correspondente encontrado no RetroAchievements.") : t("Catálogo do RetroAchievements ainda não baixado.")}
            </p>
            {info.catalogError && <p className="ra-warn">{info.catalogError}</p>}
            <div className="row">
              <Button onClick={() => setPicking(true)} disabled={!connected && !info.catalogReady}>{t("Procurar manualmente…")}</Button>
              <Button variant="ghost" onClick={() => setLink(null, true, t("Marcado como sem suporte."))}>{t("Não tem suporte")}</Button>
            </div>
          </>
        )}

        {info?.status === "unsupported" && (
          <>
            <p className="muted">{t("Marcado como sem suporte ao RetroAchievements.")}</p>
            <Button variant="ghost" onClick={() => setLink(null, false, t("Identificação automática reativada."))}>{t("Desfazer")}</Button>
          </>
        )}
      </div>

      {picking && <RaLinkPicker game={game} onClose={() => setPicking(false)} onPick={(id) => setLink(id, false, t("Jogo associado."))} />}
      {all && g && <RaGameModal raGameId={g.id} onClose={() => { setAll(false); load(); }} />}
    </>
  );
}

function Candidate({ c, onPick }: { c: RaGame; onPick: () => void }) {
  return (
    <div className="ra-cand">
      {c.iconUrl ? <img src={c.iconUrl} alt="" loading="lazy" /> : <span className="ra-icon-ph"><TrophyIcon /></span>}
      <div><strong>{c.title}</strong><small>{t("{n} conquistas", { n: c.numAchievements })}{c.consoleName ? ` · ${c.consoleName}` : ""}</small></div>
      <Button variant="primary" onClick={onPick}>{t("Confirmar")}</Button>
    </div>
  );
}

/** Associação manual: pesquisa no catálogo do console da plataforma do jogo. */
function RaLinkPicker({ game, onClose, onPick }: { game: Game; onClose: () => void; onPick: (raGameId: number) => void }) {
  const { profile } = useSession();
  const [q, setQ] = useState(game.title.replace(/[([].*?[)\]]/g, "").trim());
  const [res, setRes] = useState<RaGame[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    const h = setTimeout(() => {
      api.raSearchGames(profile!.id, game.platform, q)
        .then((r) => { if (alive) { setRes(r); setErr(null); } })
        .catch((e) => alive && setErr(errMsg(e)));
    }, 250);
    return () => { alive = false; clearTimeout(h); };
  }, [q, game.platform, profile]);
  return (
    <Modal title={t("Associar ao RetroAchievements")} onClose={onClose} wide>
      <p className="muted">{t("Escolha o jogo correspondente no catálogo de {p}. Apenas jogos com conquistas aparecem.", { p: game.platform })}</p>
      <input className="search" style={{ width: "100%", margin: "12px 0" }} value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Nome do jogo")} />
      {err && <p className="ra-warn">{err}</p>}
      <div className="ra-cands scroll">
        {res === null && !err && <p className="muted">{t("Carregando…")}</p>}
        {res?.length === 0 && <p className="muted">{t("Nenhum jogo encontrado.")}</p>}
        {res?.map((c) => <Candidate key={c.id} c={c} onPick={() => onPick(c.id)} />)}
      </div>
      <div className="row end"><Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button></div>
    </Modal>
  );
}
