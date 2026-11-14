/** Utilitários da importação de ISO (somente texto; o arquivo nunca é lido, copiado ou alterado aqui). */
export const isIsoPath = (p: string) => /\.iso$/i.test(p.trim());

/** Título inicial = nome do arquivo sem a extensão (editável depois). */
export function titleFromPath(p: string): string {
  const base = p.split(/[\\/]/).pop() ?? p;
  return base.replace(/\.[^.]+$/, "").trim() || base;
}

export const fandomSearchUrl = (title: string) => `https://www.fandom.com/?s=${encodeURIComponent(title.trim()).replace(/%20/g, "+")}`;
