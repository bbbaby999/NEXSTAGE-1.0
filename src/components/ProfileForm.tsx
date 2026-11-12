import { useState } from "react";
import Modal from "./Modal";
import Button from "./Button";
import ImagePicker from "./ImagePicker";
import Select from "./Select";
import { api, errMsg } from "../lib/api";
import { useSession } from "../state/session";
import { PLATFORMS, Profile, ProfileInput } from "../types";
import { t } from "../lib/i18n";

/** Criação, edição e exclusão de perfil. `onDone(true)` indica que o perfil foi excluído. */
export default function ProfileForm({ profile, onClose, onDone }: {
  profile?: Profile; onClose: () => void; onDone: (deleted: boolean) => void;
}) {
  const { toast } = useSession();
  const [f, setF] = useState<ProfileInput>({
    username: profile?.username ?? "",
    displayName: profile?.displayName ?? "",
    bio: profile?.bio ?? "",
    avatar: profile?.avatar ?? null,
    banner: profile?.banner ?? null,
    favoriteGame: profile?.favoriteGame ?? null,
    favoritePlatform: profile?.favoritePlatform ?? null,
  });
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const set = <K extends keyof ProfileInput>(k: K, v: ProfileInput[K]) => setF((p) => ({ ...p, [k]: v }));

  async function save() {
    setBusy(true);
    try {
      if (profile) await api.updateProfile(profile.id, f);
      else await api.createProfile(f);
      toast(t("Perfil salvo."));
      onDone(false);
    } catch (e) {
      toast(errMsg(e), "err");
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!profile) return;
    try {
      await api.deleteProfile(profile.id);
      toast(t("Perfil excluído."));
      onDone(true);
    } catch (e) {
      toast(errMsg(e), "err");
    }
  }

  return (
    <Modal title={profile ? t("Editar perfil") : t("Novo perfil")} onClose={onClose} wide>
      <div className="form-grid">
        <div className="col">
          <ImagePicker label={t("Foto")} value={f.avatar} onChange={(v) => set("avatar", v)} maxW={512} maxH={512} shape="round" crop={{ aspectW: 1, aspectH: 1, outW: 512, outH: 512 }} />
          <ImagePicker label={t("Banner")} value={f.banner} onChange={(v) => set("banner", v)} maxW={1600} maxH={500} shape="wide" crop={{ aspectW: 16, aspectH: 5, outW: 1600, outH: 500 }} />
        </div>
        <div className="fields">
          <label>{t("Nome de exibição")}<input value={f.displayName} maxLength={32} onChange={(e) => set("displayName", e.target.value)} />
          </label>
          <label>{t("Nome de usuário")}<input value={f.username} maxLength={24} onChange={(e) => set("username", e.target.value)} />
          </label>
          <label>{t("Biografia")}<textarea rows={3} value={f.bio} maxLength={300} onChange={(e) => set("bio", e.target.value)} />
          </label>
          <label>{t("Jogo favorito")}<input value={f.favoriteGame ?? ""} onChange={(e) => set("favoriteGame", e.target.value || null)} />
          </label>
          <label>{t("Plataforma favorita")}<Select<string> value={f.favoritePlatform ?? ""} width="100%" title={t("Plataforma favorita")}
              onChange={(v) => set("favoritePlatform", v || null)} options={[[t("Nenhuma"), ""], ...PLATFORMS.map((p): [string, string] => [p, p])]} />
          </label>
        </div>
      </div>
      <div className="row end">
        {profile && !confirm && <Button variant="danger" onClick={() => setConfirm(true)}>{t("Excluir perfil")}</Button>}
        {profile && confirm && <Button variant="danger" onClick={remove}>{t("Confirmar exclusão de todos os dados")}</Button>}
        <Button variant="ghost" onClick={onClose}>{t("Cancelar")}</Button>
        <Button variant="primary" onClick={save} disabled={busy}>{t("Salvar")}</Button>
      </div>
    </Modal>
  );
}
