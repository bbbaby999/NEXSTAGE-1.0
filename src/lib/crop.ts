/**
 * Matemática do enquadramento de imagem (zoom + posição). O estado é "cobrir o quadro":
 * a imagem nunca deixa bordas vazias. Offsets em pixels do quadro (canto da imagem em relação ao quadro).
 */
export interface Crop { zoom: number; ox: number; oy: number }

export const baseScale = (iw: number, ih: number, vw: number, vh: number) => Math.max(vw / iw, vh / ih);

export function clampCrop(iw: number, ih: number, vw: number, vh: number, c: Crop): Crop {
  const s = baseScale(iw, ih, vw, vh) * c.zoom;
  return { zoom: c.zoom, ox: Math.min(0, Math.max(vw - iw * s, c.ox)), oy: Math.min(0, Math.max(vh - ih * s, c.oy)) };
}

export function centerCrop(iw: number, ih: number, vw: number, vh: number, zoom = 1): Crop {
  const s = baseScale(iw, ih, vw, vh) * zoom;
  return clampCrop(iw, ih, vw, vh, { zoom, ox: (vw - iw * s) / 2, oy: (vh - ih * s) / 2 });
}

/** Muda o zoom mantendo fixo o ponto que está no centro do quadro. */
export function zoomCrop(iw: number, ih: number, vw: number, vh: number, c: Crop, zoom: number): Crop {
  const z = Math.min(4, Math.max(1, zoom));
  const b = baseScale(iw, ih, vw, vh);
  const cx = (vw / 2 - c.ox) / (b * c.zoom);
  const cy = (vh / 2 - c.oy) / (b * c.zoom);
  return clampCrop(iw, ih, vw, vh, { zoom: z, ox: vw / 2 - cx * b * z, oy: vh / 2 - cy * b * z });
}

/** Região da imagem original que aparece no quadro (para desenhar a prévia e o resultado final). */
export function sourceRect(iw: number, ih: number, vw: number, vh: number, c: Crop) {
  const s = baseScale(iw, ih, vw, vh) * c.zoom;
  return { sx: -c.ox / s, sy: -c.oy / s, sw: vw / s, sh: vh / s };
}
