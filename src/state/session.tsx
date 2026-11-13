/**
 * Estado de sessão em memória. Nada aqui é persistido: ao abrir o app a seleção
 * de perfil é sempre exibida (ASK) e "Trocar usuário" limpa a sessão (EXIT).
 */
import { createContext, ReactNode, useCallback, useContext, useRef, useState } from "react";
import type { Profile, StatsTab, View } from "../types";
import { api } from "../lib/api";

interface Toast { id: number; msg: string; kind: "ok" | "err" }
interface Ctx {
  profile: Profile | null;
  setProfile: (p: Profile | null) => void;
  /** Atualiza os dados do perfil ativo sem sair da tela atual. */
  replaceProfile: (p: Profile) => void;
  view: View;
  /** `returnTo`: para onde o "voltar" leva a partir de `v` (padrão: Home) e o elemento que recebe o foco. */
  setView: (v: View, opts?: { returnTo?: ReturnTo }) => void;
  returnTo: ReturnTo | null;
  /** Página individual de um jogo (sobrepõe a aba atual). */
  openGameId: number | null;
  setOpenGameId: (id: number | null) => void;
  /** Pedido para abrir a Galeria já filtrada por um jogo (e, opcionalmente, uma captura). */
  galleryIntent: { gameId: number; itemId?: number } | null;
  setGalleryIntent: (i: { gameId: number; itemId?: number } | null) => void;
  /** Pedido para abrir o Perfil já na aba indicada das Estatísticas (cards da Home). */
  statsIntent: StatsTab | null;
  setStatsIntent: (t: StatsTab | null) => void;
  /** Ao voltar para "Quem está jogando", abre já no modo de gerenciar perfis. */
  startManage: boolean;
  setStartManage: (v: boolean) => void;
  toast: (msg: string, kind?: "ok" | "err") => void;
}

/** Página de origem de uma página interna (ex.: Configurações → Central de Controles). */
export interface ReturnTo { view: View; focus?: string }

const SessionCtx = createContext<Ctx | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [profile, setProfileState] = useState<Profile | null>(null);
  const [view, setViewState] = useState<View>("home");
  const [returnTo, setReturnTo] = useState<ReturnTo | null>(null);
  const [openGameId, setOpenGameId] = useState<number | null>(null);
  const [galleryIntent, setGalleryIntent] = useState<Ctx["galleryIntent"]>(null);
  const [startManage, setStartManage] = useState(false);
  const [statsIntent, setStatsIntent] = useState<StatsTab | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const toast = useCallback((msg: string, kind: "ok" | "err" = "ok") => {
    // erros exibidos vão para o registro local (anexado ao feedback só com autorização)
    if (kind === "err") api.logClientError("aviso", msg).catch(() => {});
    const id = nextId.current++;
    setToasts((t) => [...t, { id, msg, kind }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "err" ? 6000 : 3000);
  }, []);

  const setView = useCallback((v: View, opts?: { returnTo?: ReturnTo }) => {
    setOpenGameId(null);
    setViewState(v);
    setReturnTo(opts?.returnTo ?? null);
  }, []);

  const setProfile = useCallback((p: Profile | null) => {
    setProfileState(p);
    setOpenGameId(null);
    setViewState("home");
    setReturnTo(null);
  }, []);

  return (
    <SessionCtx.Provider
      value={{ profile, setProfile, replaceProfile: setProfileState, view, setView, returnTo, openGameId, setOpenGameId, galleryIntent, setGalleryIntent, statsIntent, setStatsIntent, startManage, setStartManage, toast }}
    >
      {children}
      <div className="toasts" role="status" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>{t.msg}</div>
        ))}
      </div>
    </SessionCtx.Provider>
  );
}

export function useSession() {
  const c = useContext(SessionCtx);
  if (!c) throw new Error("useSession fora do SessionProvider");
  return c;
}
