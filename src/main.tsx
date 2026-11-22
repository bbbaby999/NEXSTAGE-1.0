import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { initLang } from "./lib/i18n";
import { api } from "./lib/api";
import ShotToast from "./components/ShotToast";
import "@fontsource-variable/sora";
import "./styles.css";
import "./design.css";
import "./stage.css";
import "./depth.css";
import "./ra.css";
import "./steam.css";
import "./stats.css";
import "./profile.css";
import "./controllers.css";
import "./updates.css";
import "./idle.css";
import "./zerei.css";
import "./filepicker.css";
import "./buttons.css"; // cor de destaque só em Jogar/Instalar, Remover e estados; aumento no foco
import "./highlight.css"; // a mesma cor de foco/mouse no resto do app (abas, itens, capas...)
import "./focus.css"; // indicador de foco unificado: sempre por último

// A janela "shot-toast" (notificação de captura por cima do jogo) usa o mesmo bundle, só com o cartão.
let label = "main";
try { label = getCurrentWebviewWindow().label; } catch { /* fora do Tauri */ }
if (label === "shot-toast") document.documentElement.classList.add("toast-window");

// Botão direito: nunca o menu do navegador/Windows (Voltar, Atualizar, Imprimir, Copiar...). As telas abrem
// os próprios menus: capa de jogo → "Mais opções", Galeria, visualizador e campos de texto (TextMenu).
window.addEventListener("contextmenu", (e) => e.preventDefault());

// Erros de script da interface vão para o registro local de erros (Feedback e relatórios).
if (label === "main") {
  const report = (m: string) => { api.logClientError("script", m.slice(0, 1500)).catch(() => {}); };
  window.addEventListener("error", (e) => report(`${e.message} @ ${(e.filename || "").split("/").pop()}:${e.lineno}`));
  window.addEventListener("unhandledrejection", (e) => {
    const r = e.reason;
    report(r instanceof Error ? `${r.name}: ${r.message}` : typeof r === "string" ? r : "Promise rejeitada");
  });
}

// Idioma decidido antes do primeiro quadro (sem "piscar" em português). Nunca bloqueia a abertura.
initLang(api).finally(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      {label === "shot-toast" ? <ShotToast /> : <App />}
    </React.StrictMode>
  );
});
