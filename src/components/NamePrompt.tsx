import { useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import { t } from "../lib/i18n";

/** Pergunta um nome (criar/renomear coleção). Enter confirma. */
export default function NamePrompt({ title, initial = "", onSubmit, onClose }: {
  title: string; initial?: string; onSubmit: (name: string) => Promise<void>; onClose: () => void;
}) {
  const [name, setName] = useState(initial);
  const [busy, setBusy] = useState(false);
  async function go() {
    setBusy(true);
    try { await onSubmit(name); } finally { setBusy(false); }
  }
  return (
    <Modal title={title} onClose={onClose}>
      <label>{t("Nome")}<input value={name} maxLength={40} onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && go()} />
      </label>
      <div className="row end">
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" onClick={go} disabled={busy}>{t("Salvar")}</Button>
      </div>
    </Modal>
  );
}
