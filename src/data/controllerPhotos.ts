import type { Visual } from "../lib/controllers";
import { tk } from "../lib/i18n";

/**
 * Créditos das fotos dos controles (src/assets/controllers/*.webp), todas do Wikimedia Commons com
 * licença livre. Adaptação feita pelo NEXSTAGE: fundo de estúdio removido, recorte e redimensionamento.
 * As obras CC BY / CC BY-SA exigem atribuição (exibida na página do controle); as adaptações das
 * CC BY-SA seguem a mesma licença.
 */
export interface PhotoCredit { file: string; author: string; license: string }

export const PHOTO_CREDITS: Partial<Record<Visual, PhotoCredit>> = {
  "xbox-series": { file: "Xbox Series Controller Gold Shadow Special Edition.jpg", author: "Serhio Magpie", license: "CC BY 4.0" },
  "xbox-360": { file: "Xbox-360-Wireless-Controller-White.png", author: "Evan-Amos", license: tk("Domínio público") },
  ps1: { file: "PSX-Original-Controller.png", author: "Evan-Amos", license: tk("Domínio público") },
  ps2: { file: "DualShock 2.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  ps3: { file: "PlayStation3-DualShock3.png", author: "Evan-Amos", license: tk("Domínio público") },
  ps4: { file: "DualShock 4.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  ps5: { file: "Playstation DualSense Controller.png", author: "Alex Cochrane", license: "CC BY-SA 4.0" },
  joycon: { file: "Nintendo Switch Joy-Con Controllers.png", author: "Owen1962", license: tk("Domínio público") },
  "switch-pro": { file: "Nintendo-Switch-Pro-Controller-FL.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  nes: { file: "NES-Controller-Flat.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  snes: { file: "SNES-Controller-Flat.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  n64: { file: "N64-Controller-Gray.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  wii: { file: "Nintendo-Wii-Remote-wNunchuk.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  megadrive: { file: "Sega-Genesis-3But-Cont.jpg", author: "Evan-Amos", license: tk("Domínio público") },
  segacd: { file: "Sega-Genesis-Mk2-6button.jpg", author: "Evan-Amos", license: "CC BY-SA 3.0" },
};

export const commonsUrl = (file: string) => `https://commons.wikimedia.org/wiki/File:${encodeURIComponent(file.replace(/ /g, "_"))}`;
