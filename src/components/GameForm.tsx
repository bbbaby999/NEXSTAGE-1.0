import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import Modal from "./Modal";
import Button from "./Button";
import ImagePicker from "./ImagePicker";
import Seg from "./Seg";
import type { Emulator } from "../types";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { Game, GameInput, PLATFORMS } from "../types";
import { titleFromPath } from "../lib/iso";
import { t } from "../lib/i18n";

/** Cria ou edita um jogo manual: título, plataforma, executável, argumentos e capa. */
export default function GameForm({ profileId, game, isoPath, onClose, onSaved }: {
  profileId: number; game?: Game; isoPath?: string; onClose: () => void; onSaved: () => void;
}) {
  const [emus, setEmus] = useState<Emulator[]>([]);
  const [emu, setEmu] = useState<number | null>(game?.emulatorId ?? null);
  useEffect(() => { api.listEmulators().then(setEmus).catch(() => {}); }, []);
  const iso = game?.isoPath ?? game?.romPath ?? isoPath ?? null; // ISO importado: o arquivo original é só referenciado
  const { toast } = useSession();
  const [f, setF] = useState<GameInput>({
    title: game?.title ?? (isoPath ? titleFromPath(isoPath) : ""),
    platform: game?.platform ?? "PC",
    executablePath: game?.executablePath ?? "",
    launchArgs: game?.launchArgs ?? (isoPath ? '"{iso}"' : ""),
    cover: game?.cover ?? null,
    background: game?.background ?? null,
    bgX: game?.bgX ?? 50,
    bgY: game?.bgY ?? 50,
    // arquivo do jogo (ISO/ROM); ao editar, pode ser trocado pelo botão "Selecionar"
    isoPath: game?.isoPath ?? game?.romPath ?? isoPath ?? null,
  });
  const [busy, setBusy] = useState(false);
  const set = <K extends keyof GameInput>(k: K, v: GameInput[K]) => setF((p) => ({ ...p, [k]: v }));

  async function pickExe() {
    try {
      const sel = await open({
        multiple: false,
        directory: false,
        filters: [{ name: t("Executáveis"), extensions: ["exe", "bat", "cmd"] }],
      });
      if (typeof sel === "string") {
        set("executablePath", sel);
        if (!f.title.trim()) {
          const base = sel.split(/[\\/]/).pop() ?? "";
          set("title", base.replace(/\.[^.]+$/, ""));
        }
      }
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  /** Novo local do arquivo do jogo (ex.: foi movido de pasta). Só o caminho é guardado. */
  async function pickFile() {
    try {
      const cur = f.isoPath ?? "";
      const ext = cur.split(".").pop()?.toLowerCase();
      const sel = await open({
        multiple: false,
        directory: false,
        defaultPath: cur || undefined,
        filters: [
          { name: t("Arquivos de jogo"), extensions: ["iso", "bin", "cue", "nsp", "nup", "xci", "pkg", "vpk", "chd", "cso"] },
          ...(ext && cur.includes(".") ? [{ name: "." + ext, extensions: [ext] }] : []),
          { name: t("Todos os arquivos"), extensions: ["*"] },
        ],
      });
      if (typeof sel === "string") set("isoPath", sel);
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  async function save() {
    setBusy(true);
    try {
      let id = game?.id;
      if (game) await api.updateGame(profileId, game.id, f);
      else id = (await api.addGame(profileId, f)).id;
      if (iso && id !== undefined && emu !== (game?.emulatorId ?? null)) await api.setGameEmulator(profileId, id, emu);
      // Editar um jogo existente sempre confirma a edição. O aviso de "configure um emulador" só vale
      // para um ISO recém-importado que ainda não tem como ser iniciado (sem emulador e sem comando).
      if (game) toast(t("Jogo atualizado."));
      else if (iso && emu === null && !f.executablePath.trim()) toast(t("ISO importado. Para iniciá-lo, configure um emulador ou comando em “Editar jogo”."));
      else toast(iso ? t("ISO importado.") : t("Jogo adicionado."));
      onSaved();
    } catch (e) {
      toast(errMsg(e), "err");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal title={game ? t("Editar jogo") : t("Adicionar jogo")} onClose={onClose} wide>
      <div className="form-grid">
        <ImagePicker label={t("Capa")} value={f.cover} onChange={(v) => set("cover", v)} maxW={600} maxH={800} shape="tall"
          search={{ term: f.title, platform: f.platform, kind: "cover" }} />
        <div className="fields">
          <label>{t("Título")}<input value={f.title} maxLength={80} onChange={(e) => set("title", e.target.value)} />
          </label>
          <label>{t("Plataforma")}<Select<string> value={f.platform} width="100%" title={t("Plataforma")}
              onChange={(v) => set("platform", v)} options={PLATFORMS.map((p) => [p, p])} />
          </label>
          {iso && (
            <label>{t("Arquivo ISO (mantido no local original)")}
              <div className="row">
                <input value={f.isoPath ?? iso} readOnly title={f.isoPath ?? iso} />
                {game && <Button onClick={pickFile}>{t("Selecionar")}</Button>}
              </div>
            </label>
          )}
          {iso && (
            <label>{t("Emulador cadastrado")}<Select<number | ""> value={emu ?? ""} width="100%" title={t("Emulador")} onChange={(v) => setEmu(v === "" ? null : v)}
                options={[[t("Nenhum (usar o executável e os argumentos abaixo)"), ""], ...emus.map((e): [string, number] => [`${e.name}${e.platforms.includes(f.platform) ? t(" ✓ compatível") : ""}`, e.id])]} />
            </label>
          )}
          <label>{iso ? t("Emulador ou comando (pode ser configurado depois)") : t("Executável")}
            <div className="row">
              <input value={f.executablePath} readOnly placeholder={t("Nenhum arquivo selecionado")} />
              <Button onClick={pickExe}>{t("Selecionar")}</Button>
            </div>
          </label>
          <label>{t("Argumentos de inicialização")}<input
              value={f.launchArgs}
              placeholder={iso ? t("Use {iso} para o caminho do ISO. Ex.: -fullscreen \"{iso}\"") : t("Ex.: -fullscreen \"D:\\Roms\\jogo.iso\"")}
              onChange={(e) => set("launchArgs", e.target.value)}
            />
          </label>
          <div className="bg-edit">
            <span className="muted">{t("Plano de fundo da Home (opcional)")}</span>
            <ImagePicker label={t("Sem imagem personalizada: usa a arte padrão do jogo")} value={f.background}
              onChange={(v) => set("background", v)} maxW={1920} maxH={1080} shape="wide"
              search={{ term: f.title, platform: f.platform, kind: "background" }} />
            {f.background && (
              <div className="row">
                <Seg value={f.bgY} onChange={(v) => set("bgY", v)} opts={[[t("Topo"), 0], [t("Centro"), 50], [t("Base"), 100]]} />
                <Seg value={f.bgX} onChange={(v) => set("bgX", v)} opts={[[t("Esquerda"), 0], [t("Centro"), 50], [t("Direita"), 100]]} />
              </div>
            )}
          </div>
        </div>
      </div>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" onClick={save} disabled={busy}>{t("Salvar")}</Button>
      </div>
    </Modal>
  );
}
