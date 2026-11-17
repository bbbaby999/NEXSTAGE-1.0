import { useState } from "react";
import Button from "./Button";
import ImageCropper from "./ImageCropper";
import ImageSearch from "./ImageSearch";
import { fileToDataUrl, srcToDataUrl } from "../lib/image";
import { api, errMsg } from "../lib/api";
import type { WebShot } from "../types";
import { useSession } from "../state/session";
import { t } from "../lib/i18n";
import { pickImageFile } from "../lib/filePicker";

export interface CropSpec { aspectW: number; aspectH: number; outW: number; outH: number }

/**
 * Escolhe uma imagem. Com `crop`, abre o editor de enquadramento (zoom/posição/prévia) antes de salvar.
 * Com `search`, oferece "Pesquisar na internet": o usuário escolhe o jogo e a imagem; ela entra só no
 * formulário (prévia) e substitui a atual apenas quando o formulário é salvo.
 */
export default function ImagePicker({ label, value, onChange, maxW, maxH, shape, crop, search }: {
  label: string; value: string | null; onChange: (v: string | null) => void;
  maxW: number; maxH: number; shape: "round" | "wide" | "tall"; crop?: CropSpec;
  /** Pesquisa na internet: `term` vazio abre a janela com a caixa de busca para digitar o nome do jogo. */
  search?: { term: string; platform: string; kind: "cover" | "background"; title?: string };
}) {
  const { toast } = useSession();
  const [editing, setEditing] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);

  async function fromWeb([s]: WebShot[]) {
    const data = await api.fetchImageData(s.url); // baixada pelo app (só das fontes de jogos permitidas)
    // com recorte (foto redonda, banner): abre o enquadramento antes de aplicar
    if (crop) { setEditing(await srcToDataUrl(data, 2400, 2400)); return; }
    onChange(await srcToDataUrl(data, maxW, maxH));
    toast(t("{v0} escolhido. Salve para aplicar.", { v0: search?.kind === "background" ? "Fundo" : "Capa" }));
  }

  async function pick(file?: File) {
    if (!file) return;
    try {
      if (crop) setEditing(await fileToDataUrl(file, 2400, 2400)); // reduz fotos enormes antes de editar
      else onChange(await fileToDataUrl(file, maxW, maxH));
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  return (
    <div className="picker">
      <div className={`picker-preview ${shape}`} style={value ? { backgroundImage: `url(${value})` } : undefined}>
        {!value && <span>{label}</span>}
      </div>
      <div className="row">
        <Button onClick={() => pickImageFile(label, `image-${search?.kind ?? shape}`).then((f) => pick(f ?? undefined), (e) => toast(errMsg(e), "err"))}>{value ? t("Trocar") : t("Escolher")} imagem</Button>
        {search && <Button onClick={() => setSearching(true)}>{t("Pesquisar na internet")}</Button>}
        {crop && value && <Button onClick={() => setEditing(value)}>{t("Ajustar")}</Button>}
        {value && <Button variant="danger" onClick={() => onChange(null)}>{t("Remover")}</Button>}
      </div>
      {search && searching && (
        <ImageSearch mode={search.kind} term={search.term} platform={search.platform} title={search.title} onConfirm={fromWeb} onClose={() => setSearching(false)} />
      )}
      {crop && editing && (
        <ImageCropper src={editing} aspectW={crop.aspectW} aspectH={crop.aspectH} outW={crop.outW} outH={crop.outH}
          round={shape === "round"} title={shape === "round" ? t("Ajustar foto de perfil") : t("Ajustar banner")}
          onApply={(d) => { onChange(d); setEditing(null); }} onClose={() => setEditing(null)} />
      )}
    </div>
  );
}
