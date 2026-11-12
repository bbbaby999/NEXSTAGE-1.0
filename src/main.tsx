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
import "./controllers.css";
import "./updates.css";
import "./idle.css";
import "./focus.css"; // indicador de foco unificado: sempre por último

// A janela "shot-toast" (notificação de captura por cima do jogo) usa o mesmo bundle, só com o cartão.
let label = "main";
try { label = getCurrentWebviewWindow().label; } catch { /* fora do Tauri */ }
if (label === "shot-toast") document.documentElement.classList.add("toast-window");

// Idioma decidido antes do primeiro quadro (sem "piscar" em português). Nunca bloqueia a abertura.
initLang(api).finally(() => {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      {label === "shot-toast" ? <ShotToast /> : <App />}
    </React.StrictMode>
  );
});
