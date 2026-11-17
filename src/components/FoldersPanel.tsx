import { useCallback, useEffect, useState } from "react";
import { pickOpen as open } from "../lib/filePicker";
import Modal from "./Modal";
import Button from "./Button";
import FolderImport from "./FolderImport";
import { api, errMsg } from "../lib/api";
import { formatDateTime } from "../lib/format";
import { useSession } from "../state/session";
import type { FolderRow } from "../types";
import { t } from "../lib/i18n";

/** Pastas de jogos do perfil (botão "Pastas de jogos" da Biblioteca): adicionar pasta, atualizar a varredura
 *  (só novos aparecem como importáveis) ou remover o vínculo. */
export default function FoldersPanel({ profileId, onClose, onChanged }: { profileId: number; onClose: () => void; onChanged: () => void }) {
  const { toast } = useSession();
  const [rows, setRows] = useState<FolderRow[]>([]);
  const [scan, setScan] = useState<{ path: string; id: number | null } | null>(null);
  const [confirm, setConfirm] = useState<number | null>(null);

  const load = useCallback(() => api.listLibraryFolders(profileId).then(setRows).catch((e) => toast(errMsg(e), "err")), [profileId, toast]);
  useEffect(() => { load(); }, [load]);

  async function add() {
    const sel = await open({ purpose: "folders", directory: true, multiple: false, title: t("Escolha a pasta de jogos") }).catch(() => null);
    if (typeof sel === "string") setScan({ path: sel, id: null });
  }

  return (
    <>
      <Modal title={t("Pastas de jogos")} onClose={onClose} wide>
        {rows.length === 0 ? (
          <div className="empty"><p>{t("Nenhuma pasta adicionada. Use “Adicionar pasta de jogos” para o NEXSTAGE procurar seus jogos.")}</p></div>
        ) : (
          <ul className="folder-list">
            {rows.map((f) => (
              <li key={f.id}>
                <div>
                  <b className="path">{f.path}</b>
                  <small>{t("{n} jogo(s) · última varredura: {date}", { n: f.games, date: formatDateTime(f.lastScanAt) })}{!f.exists && t(" · pasta não encontrada (unidade desconectada?)")}</small>
                </div>
                <div className="row">
                  <Button disabled={!f.exists} onClick={() => setScan({ path: f.path, id: f.id })}>{t("Atualizar varredura")}</Button>
                  {confirm === f.id ? (
                    <Button variant="danger" onClick={async () => {
                      try { await api.removeLibraryFolder(profileId, f.id); setConfirm(null); toast(t("Pasta removida da lista. Os jogos e os arquivos foram mantidos.")); load(); }
                      catch (e) { toast(errMsg(e), "err"); }
                    }}>{t("Confirmar: só remover o vínculo")}</Button>
                  ) : <Button variant="danger" onClick={() => setConfirm(f.id)}>{t("Remover pasta")}</Button>}
                </div>
              </li>
            ))}
          </ul>
        )}
        <p className="muted note">{t("Remover uma pasta apenas a tira desta lista: os jogos já importados continuam na biblioteca e nenhum arquivo é apagado.")}</p>
        <div className="row end">
          <Button variant="ghost" onClick={onClose}>{t("Fechar")}</Button>
          {/* sem pastas ainda, a ação principal recebe o foco (controle/teclado); com pastas, a 1ª "Atualizar varredura" */}
          <Button variant="primary" data-autofocus={rows.length === 0 ? "" : undefined} onClick={add}>{t("Adicionar pasta de jogos")}</Button>
        </div>
      </Modal>
      {scan && <FolderImport profileId={profileId} folder={scan.path} folderId={scan.id} onClose={() => setScan(null)}
        onImported={() => { onChanged(); load(); }} />}
    </>
  );
}
