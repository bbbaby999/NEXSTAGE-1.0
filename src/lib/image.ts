import { t } from "./i18n";
/** Redimensiona uma imagem (de qualquer origem que o <img> carregue) e devolve um data URL JPEG. */
export function srcToDataUrl(src: string, maxW: number, maxH: number): Promise<string> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => {
      const s = Math.min(1, maxW / img.width, maxH / img.height);
      const c = document.createElement("canvas");
      c.width = Math.max(1, Math.round(img.width * s));
      c.height = Math.max(1, Math.round(img.height * s));
      c.getContext("2d")!.drawImage(img, 0, 0, c.width, c.height);
      resolve(c.toDataURL("image/jpeg", 0.85));
    };
    img.onerror = () => reject(new Error(t("Não foi possível ler essa imagem.")));
    img.src = src;
  });
}

/** Redimensiona uma imagem escolhida pelo usuário e devolve um data URL JPEG (guardado no SQLite). */
export async function fileToDataUrl(file: File, maxW: number, maxH: number): Promise<string> {
  const url = URL.createObjectURL(file);
  try {
    return await srcToDataUrl(url, maxW, maxH);
  } finally {
    URL.revokeObjectURL(url);
  }
}
