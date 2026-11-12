import { useEffect, useState } from "react";
import Button from "../Button";
import Modal from "../Modal";
import Seg from "../Seg";
import GamerStatsPanel from "./GamerStatsPanel";
import { api, errMsg } from "../../lib/api";
import { useSession } from "../../state/session";
import { t, tk } from "../../lib/i18n";
import type { GamerStats, Profile, StatsVisibility } from "../../types";

type Cat = "summary" | "topGames" | "achievements" | "history" | "steam";
const CATS: [Cat, string][] = [
  ["summary", tk("Resumo gamer")], ["topGames", tk("Jogos mais jogados")], ["achievements", tk("Conquistas")], ["history", tk("Histórico")], ["steam", tk("Estatísticas Steam")],
];

/**
 * Visibilidade das estatísticas para os OUTROS perfis deste NEXSTAGE. O filtro é aplicado no backend
 * (`public_gamer_stats`): o modo privado não devolve nada e nenhuma chave, SteamID ou token faz parte
 * dos dados. Nada é enviado para a internet.
 */
export default function StatsPrivacy() {
  const { profile, toast } = useSession();
  const p = profile!;
  const [v, setV] = useState<StatsVisibility | null>(null);
  const [preview, setPreview] = useState<{ title: string; data: GamerStats | null } | null>(null);
  const [others, setOthers] = useState<Profile[]>([]);

  useEffect(() => {
    api.getStatsVisibility(p.id).then(setV).catch(() => {});
    api.listProfiles().then((l) => setOthers(l.filter((x) => x.id !== p.id))).catch(() => {});
  }, [p.id]);

  const save = async (next: StatsVisibility) => {
    setV(next);
    try { await api.setStatsVisibility(p.id, next); } catch (e) { toast(errMsg(e), "err"); }
  };
  const show = async (id: number, title: string) => {
    try { setPreview({ title, data: await api.publicGamerStats(id) }); } catch (e) { toast(errMsg(e), "err"); }
  };
  if (!v) return null;

  return (
    <div className="gs-privacy">
      <div className="row">
        <strong>{t("Visibilidade das estatísticas")}</strong>
        <Seg<StatsVisibility["mode"]> value={v.mode} onChange={(mode) => save({ ...v, mode })}
          opts={[[t("Privado"), "private"], [t("Público"), "public"], [t("Personalizado"), "custom"]]} />
      </div>
      <small className="muted">
        {v.mode === "private" ? t("Somente você vê suas estatísticas.")
          : v.mode === "public" ? t("Outros perfis deste NEXSTAGE podem ver suas estatísticas (sem dados de conta ou chaves).")
          : t("Escolha o que outros perfis deste NEXSTAGE podem ver:")}
      </small>
      {v.mode === "custom" && (
        <div className="gs-cats">
          {CATS.map(([k, l]) => (
            <label key={k} className="ra-check"><input type="checkbox" checked={v[k]} onChange={(e) => save({ ...v, [k]: e.target.checked })} /> {t(l)}</label>
          ))}
        </div>
      )}
      <div className="row">
        <Button variant="ghost" onClick={() => show(p.id, t("Como outros perfis veem você"))}>{t("Pré-visualizar como outros veem")}</Button>
        {others.map((o) => <Button key={o.id} variant="ghost" onClick={() => show(o.id, t("Estatísticas de {name}", { name: o.displayName }))}>{t("Ver {name}", { name: o.displayName })}</Button>)}
      </div>
      {preview && (
        <Modal title={preview.title} onClose={() => setPreview(null)} xl>
          {preview.data ? <GamerStatsPanel data={preview.data} /> : <p className="muted">{t("Estas estatísticas são privadas.")}</p>}
          <div className="row end"><Button variant="ghost" onClick={() => setPreview(null)}>{t("Fechar")}</Button></div>
        </Modal>
      )}
    </div>
  );
}
