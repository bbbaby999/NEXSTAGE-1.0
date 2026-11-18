import { useEffect, useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import type { XboxInstalled, XboxNotInstalledList } from "../types";
import { ImportSourceTabs, type ImportSource } from "./ImportSource";

/**
 * Importação dos jogos do Xbox / Microsoft Store instalados neste PC. A lista vem do próprio Windows
 * (pacotes de jogos instalados), sem precisar de conta; o usuário escolhe quais entram. Os jogos são
 * iniciados pelo Windows, como no menu Iniciar.
 */
export default function XboxImport({ onClose, onImported, onSource }: {
  onClose: () => void; onImported?: () => void;
  /** Biblioteca → Importar: mostra o seletor Steam/Xbox e troca de origem. */
  onSource?: (s: ImportSource) => void;
}) {
  const { profile, toast } = useSession();
  const p = profile!;
  const [list, setList] = useState<XboxInstalled[] | null>(null);
  const [sel, setSel] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [missing, setMissing] = useState<XboxNotInstalledList | null>(null);

  useEffect(() => {
    api.xboxInstalled(p.id)
      .then((l) => { setList(l); setSel(new Set(l.filter((g) => !g.inLibrary).map((g) => g.aumid))); })
      .catch((e) => { toast(errMsg(e), "err"); setList([]); });
  }, [p.id, toast]);
  // jogos de PC do histórico da conta que não estão instalados (só com conta Xbox conectada)
  useEffect(() => { api.xboxNotInstalled(p.id).then(setMissing).catch(() => setMissing(null)); }, [p.id]);
  const openStore = (pfn: string) => api.xboxOpenStore(pfn)
    .then(() => toast(t("Página do jogo aberta na Microsoft Store. Depois de instalar, importe-o por aqui.")))
    .catch((e) => toast(errMsg(e), "err"));

  const toggle = (a: string) => setSel((s) => { const n = new Set(s); if (n.has(a)) n.delete(a); else n.add(a); return n; });

  async function run() {
    setBusy(true);
    try {
      const r = await api.xboxImport(p.id, [...sel]);
      toast(t("{n} jogo(s) do Xbox adicionado(s) à biblioteca.", { n: r.added }));
      onImported?.();
      onClose();
    } catch (e) { toast(errMsg(e), "err"); }
    finally { setBusy(false); }
  }

  return (
    <Modal title={onSource ? t("Importar") : t("Importar jogos do Xbox instalados")} onClose={onClose} xl>
      {onSource && <ImportSourceTabs value="xbox" onChange={onSource} />}
      <p className="muted note">{t("Jogos do app Xbox / Microsoft Store instalados neste PC. Eles abrem pelo Windows, e o tempo de jogo é registrado pelo NEXSTAGE.")}</p>
      <div className="steam-grid" data-nav-group>
        {!list && <p className="muted">{t("Carregando…")}</p>}
        {list && list.length === 0 && <p className="muted">{t("Nenhum jogo do Xbox instalado foi encontrado neste PC.")}</p>}
        {list?.map((g) => (
          <button key={g.aumid} data-focus className={`steam-card ${sel.has(g.aumid) ? "on" : ""}`} disabled={g.inLibrary}
            onClick={() => toggle(g.aumid)} aria-pressed={sel.has(g.aumid)}>
            <div className="steam-cover">
              {g.image ? <img src={g.image} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : null}
              <i className="steam-check" aria-hidden="true">✓</i>
            </div>
            <strong>{g.name}</strong>
            <small>{t("Xbox · PC")}</small>
            {g.inLibrary && <em className="steam-lib">{t("Já na biblioteca")}</em>}
          </button>
        ))}
      </div>
      {missing?.connected && (
        <>
          <h3 className="steam-h3">{t("Jogados no Xbox e não instalados neste PC")}</h3>
          <p className="muted note">{t("Jogos de PC do histórico da sua conta Xbox (o Xbox não informa jogos comprados e nunca abertos). A instalação é feita pela Microsoft Store, inclusive os do Game Pass enquanto estiverem no catálogo.")}</p>
          {missing.items.length === 0 && (
            <p className="muted">{missing.pending > 0
              ? t("Sincronize a conta Xbox em Configurações → Contas e plataformas para listar os jogos jogados no PC.")
              : t("Todos os jogos de PC do seu histórico do Xbox já estão instalados.")}</p>
          )}
          {missing.items.length > 0 && (
            <div className="steam-grid xni" data-nav-group>
              {missing.items.map((g) => (
                <button key={g.titleId} data-focus data-focus-key={`xni-${g.titleId}`} className="steam-card xbox-missing" onClick={() => openStore(g.pfn)}
                  aria-label={t("{name}: abrir na Microsoft Store para instalar", { name: g.name })}>
                  <div className="steam-cover">
                    {g.image ? <img src={g.image} alt="" loading="lazy" referrerPolicy="no-referrer" onError={(e) => (e.currentTarget.style.visibility = "hidden")} /> : null}
                  </div>
                  <strong>{g.name}</strong>
                  <small>{t("Não instalado")}</small>
                  <em className="steam-lib">{t("Instalar pela Microsoft Store")}</em>
                </button>
              ))}
            </div>
          )}
        </>
      )}
      <div className="row end">
        <span className="grow muted">{t("{n} selecionado(s)", { n: sel.size })}</span>
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" disabled={!sel.size || busy} onClick={run}>{busy ? t("Importando…") : t("Importar")}</Button>
      </div>
    </Modal>
  );
}
