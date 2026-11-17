/**
 * Escolha de arquivos e pastas: navegador próprio do NEXSTAGE (com controle) ou o seletor do Windows,
 * conforme Configurações → Controles e atalhos → "Navegador de arquivos" (`app_settings.file_picker`).
 *
 * As funções seguem a forma do seletor do Tauri (`open`/`save` de @tauri-apps/plugin-dialog), então
 * cada tela só troca a função chamada. O componente que desenha o navegador (FilePickerHost) fica
 * montado no App e atende um pedido por vez.
 */
import { useSyncExternalStore } from "react";
import { open as sysOpen, save as sysSave } from "@tauri-apps/plugin-dialog";
import { api } from "./api";

export interface PickFilter { name: string; extensions: string[] }
export interface PickOptions {
  title?: string;
  /** Escolher pasta em vez de arquivo. */
  directory?: boolean;
  multiple?: boolean;
  filters?: PickFilter[];
  /** Arquivo ou pasta inicial (no "salvar", também o nome sugerido). */
  defaultPath?: string;
  /** Chave para lembrar a última pasta usada (ex.: "games", "images", "backup"). */
  purpose?: string;
}
export interface PickRequest { id: number; mode: "open" | "save"; opts: PickOptions; resolve: (v: string[] | null) => void }
let seq = 0;

let current: PickRequest | null = null;
const subs = new Set<() => void>();
const notify = () => subs.forEach((f) => f());
export const usePickRequest = () =>
  useSyncExternalStore((cb) => { subs.add(cb); return () => { subs.delete(cb); }; }, () => current);

/** Encerra o pedido aberto (chamado pelo navegador ao confirmar ou cancelar). */
export function finishPick(v: string[] | null) {
  const r = current;
  current = null;
  notify();
  r?.resolve(v);
}

function request(mode: "open" | "save", opts: PickOptions): Promise<string[] | null> {
  // um pedido novo cancela um que tenha ficado aberto
  if (current) finishPick(null);
  return new Promise((resolve) => { current = { id: ++seq, mode, opts, resolve }; notify(); });
}

export type PickerKind = "app" | "system";
export async function pickerKind(): Promise<PickerKind> {
  const v = await api.getAppSetting("file_picker").catch(() => null);
  return v === "system" ? "system" : "app";
}

/** Abrir arquivo(s) ou pasta. Devolve como o seletor do Tauri: string, string[] (multiple) ou null. */
export async function pickOpen(opts: PickOptions & { multiple: true }): Promise<string[] | null>;
export async function pickOpen(opts: PickOptions): Promise<string | null>;
export async function pickOpen(opts: PickOptions): Promise<string | string[] | null> {
  if ((await pickerKind()) === "system") {
    const sel = await sysOpen({ title: opts.title, directory: opts.directory, multiple: opts.multiple, filters: opts.filters, defaultPath: opts.defaultPath }).catch(() => null);
    if (sel === null) return null;
    return opts.multiple ? (Array.isArray(sel) ? sel : [sel]) : (Array.isArray(sel) ? sel[0] ?? null : sel);
  }
  const r = await request("open", opts);
  if (!r?.length) return null;
  return opts.multiple ? r : r[0];
}

/** "Salvar como": caminho do arquivo a gravar (ou null). */
export async function pickSave(opts: PickOptions): Promise<string | null> {
  if ((await pickerKind()) === "system") {
    return sysSave({ title: opts.title, defaultPath: opts.defaultPath, filters: opts.filters }).catch(() => null);
  }
  const r = await request("save", opts);
  return r?.[0] ?? null;
}

export const IMAGE_EXTS = ["png", "jpg", "jpeg", "webp", "bmp", "gif", "avif"];

/**
 * Imagem do PC como `File` (para as telas que antes usavam <input type="file">: capa, fundo, foto de
 * perfil, fundo da Home). A imagem é lida pelo backend (só formatos de imagem, até 40 MB).
 */
export async function pickImageFile(title?: string, purpose = "images"): Promise<File | null> {
  const path = await pickOpen({ title, purpose, filters: [{ name: "Imagens", extensions: IMAGE_EXTS }] });
  if (!path) return null;
  const data = await api.fsReadImage(path);
  const blob = await (await fetch(data)).blob();
  return new File([blob], path.split(/[\\/]/).pop() ?? "imagem", { type: blob.type });
}
