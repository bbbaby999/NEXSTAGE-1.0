import { useCallback, useEffect, useState } from "react";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { Wordmark } from "../components/Logo";
import Avatar from "../components/Avatar";
import Button from "../components/Button";
import ProfileForm from "../components/ProfileForm";
import type { Profile, View } from "../types";
import { t } from "../lib/i18n";

/** Entrada automática: só na abertura do app (não ao voltar por "Trocar usuário"). */
let autoTried = false;

const START_VIEWS: View[] = ["home", "library", "collection", "gallery", "stats", "settings", "emulators", "achievements", "controllers"];

/** Tela exibida sempre na inicialização e após "Trocar usuário". */
export default function ProfileSelect() {
  const { setProfile, setView, toast, startManage, setStartManage } = useSession();
  const [profiles, setProfiles] = useState<Profile[] | null>(null);
  const [manage, setManage] = useState(startManage);
  useEffect(() => { if (startManage) setStartManage(false); }, [startManage, setStartManage]);
  const [editing, setEditing] = useState<Profile | "new" | null>(null);
  const load = useCallback(async () => {
    try {
      const list = await api.listProfiles();
      setProfiles(list);
      return list;
    } catch (e) {
      toast(errMsg(e), "err");
      setProfiles([]);
      return [] as Profile[];
    }
  }, [toast]);

  // Na abertura, a entrada automática carrega o perfil escolhido.
  useEffect(() => {
    (async () => {
      const list = await load();
      if (autoTried) return;
      autoTried = true;
      if (startManage) return;
      const id = Number(await api.getAppSetting("auto_profile").catch(() => null));
      const p = list.find((x) => x.id === id);
      if (!p) return;
      enter(p);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const pick = (p: Profile) => { if (manage) setEditing(p); else enter(p); };
  const closeEditor = () => setEditing(null);

  /** Entra no perfil e abre a página inicial configurada (Home, Biblioteca, Galeria ou a última usada). */
  async function enter(p: Profile) {
    let v: View = "home";
    try {
      const sp = await api.getAppSetting("start_page");
      if (sp === "library") v = "library";
      else if (sp === "gallery") v = "gallery";
      else if (sp === "last") {
        const lv = await api.getProfileSetting(p.id, "last_view");
        if (lv && START_VIEWS.includes(lv as View)) v = lv as View;
      }
    } catch { /* usa Home */ }
    setProfile(p);
    setView(v);
  }

  useEffect(() => {
    if (!editing && profiles && !document.querySelector(":focus")) {
      document.querySelector<HTMLElement>("[data-focus]")?.focus();
    }
  }, [profiles, editing]);

  if (!profiles) return null;

  return (
    <div className="select">
      <Wordmark size={34} />
      <h1>{profiles.length ? t("Quem está jogando?") : t("Crie seu primeiro perfil")}</h1>
      <div className="profiles">
        {profiles.map((p, i) => (
          <button
            key={p.id}
            data-focus
            className="profile-card"
            style={{ "--i": i } as React.CSSProperties}
            onClick={() => pick(p)}
          >
            <Avatar name={p.displayName} src={p.avatar} size={132} />
            <strong>{p.displayName}</strong>
            <small>@{p.username}</small>
            {manage && <span className="edit-badge">{t("Editar")}</span>}
          </button>
        ))}
        <button data-focus className="profile-card add" style={{ "--i": profiles.length } as React.CSSProperties} onClick={() => setEditing("new")}>
          <div className="avatar plus" style={{ width: 132, height: 132 }}>+</div>
          <strong>{t("Novo perfil")}</strong>
        </button>
      </div>
      {profiles.length > 0 && (
        <Button variant={manage ? "primary" : "ghost"} onClick={() => setManage((m) => !m)}>
          {manage ? t("Concluir") : t("Gerenciar perfis")}
        </Button>
      )}
      <p className="tagline">{t("ALL GAMES. ONE STAGE.")}</p>

      {editing && (
        <ProfileForm
          profile={editing === "new" ? undefined : editing}
          onClose={closeEditor}
          onDone={() => { closeEditor(); load(); }}
        />
      )}
    </div>
  );
}
