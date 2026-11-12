import Modal from "./Modal";
import Button from "./Button";
import { NOTE_LABEL, PATCH_NOTES } from "../data/patchnotes";
import { CHANNEL } from "../lib/version";
import { t } from "../lib/i18n";

const fmt = (d: string) => (/^\d{4}-\d{2}-\d{2}$/.test(d) ? new Date(d + "T12:00:00").toLocaleDateString("pt-BR", { day: "2-digit", month: "long", year: "numeric" }) : d);

/** Histórico de versões (sempre disponível em Configurações → Patch Notes). */
export default function PatchNotes({ onClose }: { onClose: () => void }) {
  return (
    <Modal title={t("Patch Notes")} onClose={onClose} wide>
      <div className="notes">
        {PATCH_NOTES.map((n, i) => (
          <article key={n.version} className={`note ${i === 0 ? "latest" : ""}`}>
            <header>
              <h3>{/^\d/.test(n.version) && !n.version.includes("–") ? `${CHANNEL} ${n.version}` : n.version}</h3>
              <span className="muted">{fmt(n.date)}{i === 0 ? t(" · versão atual") : ""}</span>
            </header>
            {n.summary && <p className="muted">{n.summary}</p>}
            {n.sections.map((s) => (
              <div key={s.type} className="note-sec">
                <span className={`badge ${s.type}`}>{NOTE_LABEL[s.type]}</span>
                <ul>{s.items.map((t) => <li key={t}>{t}</li>)}</ul>
              </div>
            ))}
          </article>
        ))}
      </div>
      <div className="row end"><Button variant="primary" onClick={onClose}>{t("Fechar")}</Button></div>
    </Modal>
  );
}
