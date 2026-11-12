import { useCallback, useEffect, useState } from "react";
import { useSession } from "../state/session";
import { useLibrary } from "../state/useLibrary";
import { api, errMsg } from "../lib/api";
import Button from "../components/Button";
import GameCard from "../components/GameCard";
import NamePrompt from "../components/NamePrompt";
import type { GameCollection } from "../types";
import { t } from "../lib/i18n";

/** Coleções do perfil (como as pastas da Steam). "Favoritos" é uma coleção automática. */
export default function Collection() {
  const { profile, toast, setOpenGameId } = useSession();
  const p = profile!;
  const { games, loading } = useLibrary(p.id);
  const [cols, setCols] = useState<GameCollection[]>([]);
  const [sel, setSel] = useState<"fav" | number>("fav");
  const [dlg, setDlg] = useState<null | "new" | "rename">(null);
  const [confirm, setConfirm] = useState(false);

  const load = useCallback(
    () => api.listCollections(p.id).then(setCols).catch((e) => toast(errMsg(e), "err")),
    [p.id, toast]
  );
  useEffect(() => { load(); }, [load]);
  useEffect(() => { setConfirm(false); }, [sel]);

  const current = typeof sel === "number" ? cols.find((c) => c.id === sel) : undefined;
  const list = sel === "fav" ? games.filter((g) => g.favorite) : games.filter((g) => current?.gameIds.includes(g.id));
  if (loading) return null;

  return (
    <>
      <div className="page-head"><h1>{t("Coleções")}</h1></div>
      <div className="folders">
        <button data-focus className={`folder ${sel === "fav" ? "on" : ""}`} onClick={() => setSel("fav")}>
          {t("★ Favoritos")}{" "}<small>{games.filter((g) => g.favorite).length}</small>
        </button>
        {cols.map((c) => (
          <button key={c.id} data-focus className={`folder ${sel === c.id ? "on" : ""}`} onClick={() => setSel(c.id)}>
            {c.name} <small>{c.gameIds.length}</small>
          </button>
        ))}
        <button data-focus className="folder add" onClick={() => setDlg("new")}>{t("+ Nova coleção")}</button>
      </div>

      {current && (
        <div className="row" style={{ margin: "14px 0" }}>
          <Button onClick={() => setDlg("rename")}>{t("Renomear")}</Button>
          {confirm ? (
            <Button variant="danger" onClick={async () => {
              try { await api.deleteCollection(p.id, current.id); setSel("fav"); await load(); toast(t("Coleção excluída (os jogos foram mantidos).")); }
              catch (e) { toast(errMsg(e), "err"); }
            }}>{t("Confirmar exclusão (os jogos continuam na biblioteca)")}</Button>
          ) : <Button variant="danger" onClick={() => setConfirm(true)}>{t("Excluir coleção")}</Button>}
        </div>
      )}

      {list.length === 0 ? (
        <div className="empty"><p>{sel === "fav" ? t("Nenhum favorito ainda.") : t("Coleção vazia. Abra um jogo e escolha Coleções para adicioná-lo.")}</p></div>
      ) : (
        <div className="grid">{list.map((g, i) => <GameCard key={g.id} game={g} index={i} entry={i === 0} onOpen={(x) => setOpenGameId(x.id)} />)}</div>
      )}

      {dlg === "new" && (
        <NamePrompt title={t("Nova coleção")} onClose={() => setDlg(null)} onSubmit={async (name) => {
          try { const c = await api.createCollection(p.id, name); await load(); setSel(c.id); setDlg(null); }
          catch (e) { toast(errMsg(e), "err"); }
        }} />
      )}
      {dlg === "rename" && current && (
        <NamePrompt title={t("Renomear coleção")} initial={current.name} onClose={() => setDlg(null)} onSubmit={async (name) => {
          try { await api.renameCollection(p.id, current.id, name); await load(); setDlg(null); }
          catch (e) { toast(errMsg(e), "err"); }
        }} />
      )}
    </>
  );
}
