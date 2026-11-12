import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import Button from "./Button";
import { AboutCapture, CaptureMenu, ConfirmDelete, Icon, ICON, MenuAction } from "./CaptureUi";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { play } from "../lib/sound";
import { formatDate } from "../lib/format";
import { useInputDevice } from "../lib/inputDevice";
import { onPadButton } from "../lib/nav";
import { GLYPHS } from "./Glyph";
import type { GalleryItem } from "../types";
import { t } from "../lib/i18n";

export const itemSrc = (i: GalleryItem) => convertFileSrc(i.path);

/**
 * Visualizador de capturas: imagem centralizada e proporcional (contain, sem cortar nem esticar),
 * ações em coluna vertical (alcançáveis por controle com cima/baixo), sem tela cheia.
 * ← → (teclado/direcional) e L1/R1 trocam de captura; nos extremos o botão correspondente é
 * desativado (sem "dar a volta"). A navegação fica restrita à lista recebida (ex.: só o mesmo jogo).
 */
export default function Viewer({ items, index, onIndex, onClose, onFavorite, onDelete }: {
  items: GalleryItem[]; index: number; onIndex: (i: number) => void; onClose: () => void;
  onFavorite: (i: GalleryItem) => void; onDelete: (i: GalleryItem) => void;
}) {
  const { profile, toast } = useSession();
  const ref = useRef<HTMLDivElement>(null);
  const [broken, setBroken] = useState(false);
  const [menu, setMenu] = useState<{ x?: number; y?: number } | null>(null);
  const [about, setAbout] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const item = items[index];
  const n = items.length;
  const atFirst = index <= 0, atLast = index >= n - 1;
  const device = useInputDevice();

  // Tela cheia: só a imagem + uma pílula translúcida com os atalhos. A janela também vai para tela cheia
  // (se já não estiver) e volta ao modo anterior ao sair. No overlay em jogo, a janela não é alterada.
  const [full, setFull] = useState(false);
  const wasFull = useRef<boolean | null>(null);
  const fullOn = useRef(false);
  const toggleFull = useRef<(on?: boolean) => void>(() => {});
  toggleFull.current = (on = !full) => {
    if (on === full) return;
    setFull(on);
    fullOn.current = on;
    play(on ? "open" : "close");
    // ao sair, o foco volta para o botão "Tela cheia" (a interface estava inerte e perdeu o foco)
    if (!on) requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>(".v-full")?.focus());
    if (document.documentElement.classList.contains("overlay-mode")) return;
    if (on) {
      // estado real da janela (não a preferência salva): se já estava em tela cheia, continua nela ao sair
      api.isFullscreen().catch(() => true).then((v) => {
        if (!fullOn.current) return; // já saiu antes da resposta: não mexe na janela
        wasFull.current = v;
        if (!wasFull.current) api.setFullscreen(true).catch(() => {});
      });
    } else if (wasFull.current === false) {
      api.setFullscreen(false).catch(() => {});
    }
  };
  // fechar o visualizador em tela cheia também devolve a janela ao modo anterior
  useEffect(() => () => { if (wasFull.current === false) api.setFullscreen(false).catch(() => {}); }, []);
  useEffect(() => { if (!full) wasFull.current = null; }, [full]);

  // Pílula de atalhos: discreta; acende ao mexer o mouse ou usar um comando e volta a apagar sozinha.
  const [awake, setAwake] = useState(true);
  useEffect(() => {
    if (!full) return;
    let t = 0;
    const wake = () => { setAwake(true); clearTimeout(t); t = window.setTimeout(() => setAwake(false), 2200); };
    wake();
    const evs = ["mousemove", "keydown", "nexstage:viewer", "nexstage:tab", "nexstage:button"];
    evs.forEach((e) => window.addEventListener(e, wake));
    return () => { clearTimeout(t); evs.forEach((e) => window.removeEventListener(e, wake)); };
  }, [full]);
  useEffect(() => { if (full) setAwake(true); }, [index, full]);

  useEffect(() => { setBroken(false); }, [index]);

  useEffect(() => {
    const prevFocus = document.activeElement as HTMLElement | null;
    ref.current?.querySelector<HTMLElement>("[data-focus]:not([disabled])")?.focus();
    play("open");
    return () => { play("close"); prevFocus?.focus?.(); };
  }, []);

  useEffect(() => {
    const go = (d: number) => {
      if (document.querySelector(".ctx-root, .pick-root, .kb-root") || menu || about || confirm) return;
      const ni = index + d;
      if (ni >= 0 && ni < n) { onIndex(ni); play("move"); }
    };
    const onNav = (e: Event) => go((e as CustomEvent<number>).detail);
    const onTab = (e: Event) => go((e as CustomEvent<number>).detail); // L1 / R1
    const onCtx = () => { if (!menu && !about && !confirm && !full) setMenu({}); };
    const topmost = () => { const all = document.querySelectorAll(".overlay"); return all[all.length - 1] === ref.current; };
    const onKey = (e: KeyboardEvent) => {
      if (!topmost() || menu || about || confirm) return;
      // F: alterna a tela cheia (fora de campos de texto)
      if ((e.key === "f" || e.key === "F") && !e.ctrlKey && !e.altKey && !e.metaKey && !(e.target instanceof HTMLInputElement)) {
        e.preventDefault(); toggleFull.current(); return;
      }
      if (e.key !== "Escape") return;
      play("back");
      if (full) toggleFull.current(false); // Esc / B / ○ em tela cheia: só sai da tela cheia
      else onClose();
    };
    // Y / △: alterna a tela cheia
    const offPad = onPadButton((b, e) => {
      if (b !== "y" || e.defaultPrevented || !topmost() || menu || about || confirm) return;
      e.preventDefault(); toggleFull.current();
    });
    window.addEventListener("nexstage:viewer", onNav);
    window.addEventListener("nexstage:tab", onTab);
    window.addEventListener("nexstage:context", onCtx);
    window.addEventListener("keydown", onKey);
    return () => {
      offPad();
      window.removeEventListener("nexstage:viewer", onNav);
      window.removeEventListener("nexstage:tab", onTab);
      window.removeEventListener("nexstage:context", onCtx);
      window.removeEventListener("keydown", onKey);
    };
  }, [index, n, onIndex, onClose, menu, about, confirm, full]);

  if (!item) return null;

  const reveal = () =>
    api.revealCapture(profile!.id, item.id).catch((e) => toast(errMsg(e), "err"));

  const actions: MenuAction[] = [
    { label: t("Abrir pasta"), icon: ICON.folder, run: reveal },
    { label: item.favorite ? t("Desfavoritar") : t("Favoritar"), icon: ICON.star, run: () => onFavorite(item) },
    { label: t("Sobre esta captura"), icon: ICON.info, run: () => setAbout(true) },
    { label: t("Excluir captura"), icon: ICON.trash, danger: true, run: () => setConfirm(true) },
  ];

  return (
    <div className="overlay viewer" data-viewer ref={ref} role="dialog" aria-modal="true"
      onContextMenu={(e) => { e.preventDefault(); setMenu({ x: e.clientX, y: e.clientY }); }}>
      {/* com a tela cheia aberta, a interface normal fica inerte (o controle não "clica" no que está escondido) */}
      <div className="viewer-main" {...({ inert: full ? "" : undefined } as Record<string, unknown>)}>
        <div className="viewer-stage">
          {item.exists && !broken ? (
            <img key={item.id} src={itemSrc(item)} alt={item.gameTitle ?? t("Captura")} onError={() => setBroken(true)}
              onDoubleClick={() => toggleFull.current(true)} title={t("Clique duas vezes para tela cheia")} />
          ) : (
            <div className="empty"><p>{t("Arquivo ausente ou corrompido. Você pode excluir este registro.")}</p></div>
          )}
          <div className="viewer-caption">{item.gameTitle ?? t("Sem jogo")} · {formatDate(item.takenAt ?? item.addedAt)} · {index + 1} de {n}</div>
        </div>
        <div className="viewer-rail" aria-label={t("Ações da captura")}>
          <Button onClick={() => onIndex(index - 1)} disabled={atFirst}><Icon d={ICON.prev} /> {t("Anterior")}</Button>
          <Button onClick={() => onIndex(index + 1)} disabled={atLast}>{t("Próxima")}{" "}<Icon d={ICON.next} /></Button>
          <Button onClick={() => onFavorite(item)} className={item.favorite ? "fav-on" : ""}>
            <Icon d={ICON.star} /> {item.favorite ? t("Favorita") : t("Favoritar")}
          </Button>
          <Button className="v-full" onClick={() => toggleFull.current(true)} disabled={!item.exists || broken}>
            <Icon d={ICON.full} /> {t("Tela cheia")}{device !== "keyboard" ? <kbd className="v-kbd">{device === "playstation" ? "△" : device === "nintendo" ? t("X") : t("Y")}</kbd> : <kbd className="v-kbd">{t("F")}</kbd>}
          </Button>
          <Button onClick={reveal}><Icon d={ICON.folder} /> {t("Abrir pasta")}</Button>
          <Button onClick={(e) => { const r = e.currentTarget.getBoundingClientRect(); setMenu({ x: r.left - 250, y: r.top }); }}>
            <Icon d={ICON.more} /> {t("Mais ações")}</Button>
          <Button variant="primary" onClick={onClose}><Icon d={ICON.back} /> {t("Voltar para a Galeria")}</Button>
        </div>
      </div>
      {full && (
        <div className={`viewer-full ${awake ? "awake" : ""}`} onDoubleClick={() => toggleFull.current(false)}>
          {item.exists && !broken
            ? <img key={item.id} src={itemSrc(item)} alt={item.gameTitle ?? t("Captura")} onError={() => setBroken(true)} />
            : <p className="muted">{t("Arquivo ausente ou corrompido.")}</p>}
          {/* única informação na tela: atalhos de anterior / próxima / sair (clicáveis com o mouse) */}
          <div className="vf-hints">
            <button type="button" tabIndex={-1} disabled={atFirst} onClick={() => { onIndex(index - 1); play("move"); }} aria-label={t("Anterior")}>
              <Icon d={ICON.prev} /><kbd>{device === "keyboard" ? "←" : GLYPHS[device].l1}</kbd>
            </button>
            <button type="button" tabIndex={-1} disabled={atLast} onClick={() => { onIndex(index + 1); play("move"); }} aria-label={t("Próxima")}>
              <kbd>{device === "keyboard" ? "→" : GLYPHS[device].r1}</kbd><Icon d={ICON.next} />
            </button>
            <i aria-hidden="true" />
            <button type="button" tabIndex={-1} onClick={() => { play("back"); toggleFull.current(false); }} aria-label={t("Sair da tela cheia")}>
              <Icon d={ICON.exitFull} /><kbd>{GLYPHS[device].back}</kbd>
            </button>
          </div>
        </div>
      )}
      {menu && <CaptureMenu actions={actions} x={menu.x} y={menu.y} onClose={() => setMenu(null)} />}
      {about && <AboutCapture item={item} onClose={() => setAbout(false)} />}
      {confirm && (
        <ConfirmDelete count={1} onClose={() => setConfirm(false)}
          onConfirm={() => { setConfirm(false); if (n === 1) onClose(); else if (atLast) onIndex(index - 1); onDelete(item); }} />
      )}
    </div>
  );
}
