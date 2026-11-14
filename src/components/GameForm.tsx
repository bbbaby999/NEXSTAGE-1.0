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

/** Argumentos padrão de um arquivo importado SEM emulador (o comando escolhido recebe o caminho). Com
 *  emulador, os argumentos do jogo ficam vazios para valerem os do emulador (ex.: tela cheia). */
const FILE_ARGS = '"{iso}"';

/** Cria ou edita um jogo: título, plataforma, executável, argumentos e capa. Em "Adicionar jogo", recebe o
 *  arquivo já escolhido e identificado: `exePath` (jogo de PC) ou `isoPath` (ISO/ROM, iniciado por emulador). */
export default function GameForm({ profileId, game, isoPath, exePath, platform, emulatorId, onClose, onSaved }: {
  profileId: number; game?: Game; isoPath?: string; exePath?: string; platform?: string | null; emulatorId?: number | null;
  onClose: () => void; onSaved: () => void;
}) {
  const [emus, setEmus] = useState<Emulator[]>([]);
  const [emu, setEmu] = useState<number | null>(game?.emulatorId ?? emulatorId ?? null);
  useEffect(() => { api.listEmulators().then(setEmus).catch(() => {}); }, []);
  const iso = game?.isoPath ?? game?.romPath ?? isoPath ?? null; // ISO/ROM importado: o arquivo original é só referenciado
  const isIso = !!iso && /\.iso$/i.test(iso);
  const { toast } = useSession();
  const [f, setF] = useState<GameInput>({
    title: game?.title ?? titleFromPath(isoPath ?? exePath ?? ""),
    platform: game?.platform ?? platform ?? "PC",
    executablePath: game?.executablePath ?? exePath ?? "",
    launchArgs: game?.launchArgs ?? (isoPath ? (emulatorId != null ? "" : FILE_ARGS) : ""),
    cover: game?.cover ?? null,
    background: game?.background ?? null,
    bgX: game?.bgX ?? 50,
    bgY: game?.bgY ?? 50,
    // arquivo do jogo (ISO/ROM); ao editar, pode ser trocado pelo botão "Selecionar"
    isoPath: game?.isoPath ?? game?.romPath ?? isoPath ?? null,
  });
  const [busy, setBusy] = useState(false);
  /** Jogo sem ISO/ROM associado a um emulador: o arquivo escolhido abaixo é passado a ele como {iso}/{rom}. */
  const viaEmu = !iso && emu !== null;
  const set = <K extends keyof GameInput>(k: K, v: GameInput[K]) => setF((p) => ({ ...p, [k]: v }));

  async function pickExe() {
    try {
      const sel = await open({
        multiple: false,
        directory: false,
        // com emulador e sem ISO/ROM, o "executável" é o arquivo do jogo entregue ao emulador (ex.: EBOOT.BIN, .xex)
        filters: viaEmu
          ? [{ name: t("Todos os arquivos"), extensions: ["*"] }, { name: t("Executáveis"), extensions: ["exe", "bat", "cmd"] }]
          : [{ name: t("Executáveis"), extensions: ["exe", "bat", "cmd"] }],
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
      if (id !== undefined && emu !== (game?.emulatorId ?? null)) await api.setGameEmulator(profileId, id, emu);
      // Editar um jogo existente sempre confirma a edição. O aviso de "configure um emulador" só vale
      // para um ISO recém-importado que ainda não tem como ser iniciado (sem emulador e sem comando).
      if (game) toast(t("Jogo atualizado."));
      else if (iso && emu === null && !f.executablePath.trim()) toast(isIso
        ? t("ISO importado. Para iniciá-lo, configure um emulador ou comando em “Editar jogo”.")
        : t("Jogo importado. Para iniciá-lo, configure um emulador ou comando em “Editar jogo”."));
      else toast(isIso ? t("ISO importado.") : iso ? t("Jogo importado.") : t("Jogo adicionado."));
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
            <label>{isIso ? t("Arquivo ISO (mantido no local original)") : t("Arquivo do jogo (mantido no local original)")}
              <div className="row">
                <input value={f.isoPath ?? iso} readOnly title={f.isoPath ?? iso} />
                {game && <Button onClick={pickFile}>{t("Selecionar")}</Button>}
              </div>
            </label>
          )}
          {(iso || emus.length > 0 || emu !== null) && (
            <label>{t("Emulador cadastrado")}<Select<number | ""> value={emu ?? ""} width="100%" title={t("Emulador")} onChange={(v) => {
                  setEmu(v === "" ? null : v);
                  // novo arquivo, argumentos ainda no padrão: com emulador valem os dele; sem, o comando recebe o caminho
                  if (!game && (f.launchArgs === "" || f.launchArgs === FILE_ARGS)) set("launchArgs", v === "" ? FILE_ARGS : "");
                }}
                options={[[t("Nenhum (usar o executável e os argumentos abaixo)"), ""], ...emus.map((e): [string, number] => [`${e.name}${e.platforms.includes(f.platform) ? t(" ✓ compatível") : ""}`, e.id])]} />
            </label>
          )}
          <label>{iso ? t("Emulador ou comando (pode ser configurado depois)") : viaEmu ? t("Arquivo do jogo (enviado ao emulador)") : t("Executável")}
            <div className="row">
              <input value={f.executablePath} readOnly placeholder={t("Nenhum arquivo selecionado")} />
              <Button onClick={pickExe}>{t("Selecionar")}</Button>
            </div>
          </label>
          <label>{t("Argumentos de inicialização")}<input
              value={f.launchArgs}
              placeholder={iso || viaEmu ? t("Use {iso} para o caminho do ISO. Ex.: -fullscreen \"{iso}\"") : t("Ex.: -fullscreen \"D:\\Roms\\jogo.iso\"")}
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
