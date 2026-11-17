/** Camada fina sobre `invoke`: um único lugar que conhece os comandos do backend Rust. */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { currentLang, t } from "./i18n";
import { idleBusy } from "./idle";
import type { ScanItem, GameHit, ImageSet, SourceRef, WebShot, Emulator, FolderRow, ImportSummary, Activity, FandomLink, PadCombos, StartupInfo, ClassifyResult, Controller, Drive, GalleryItem, ImportResult, ShortcutInfo, Game, GameCollection, GameInfo, GameInput, Profile, ProfileInput, RaAccount, RaOverview, RaAchievement, RaGameDetail, RaSyncReport, RaLinkInfo, RaGame, RaScanReport, RaEmuSupport, SteamAccount, SteamOwnedList, SteamPreviewItem, SteamImportReport, SteamSyncReport, SteamAppStatus, SteamGameData, SteamClientStatus, SteamAchOverview, SteamAchEntry, SteamNewsOverview, BackupInfo, BackupTarget, BackupReport, BackupInspect, BackupVerify, BackupRestoreReport, BackupAuto, BackupAutoMode, GamerStats, StatsVisibility, MetacriticResult, StoreHit, SourceView, SourceId, IgdbResult, NexusLinkStatus, NexusGame, NexusModsPage, NexusModDetail, NexusMod, ModSort, ZereiSearch, ZereiArt, ZereiEntry, ZereiInput, ZereiImport, LogoGames, MonitorState, FsPlaces, FsListing, ZereiRefreshReport, ZereiStatus } from "../types";

/**
 * Capas e fundos ficam em arquivos na pasta de dados do app (ver src-tauri/src/images.rs): o backend
 * devolve o caminho e aqui ele vira URL do protocolo de arquivos do Tauri. Data URLs (formato antigo)
 * e endereços https passam sem mudança. Parênteses são codificados para funcionar dentro de `url(...)`.
 */
const isPath = (v: string) => /^[a-zA-Z]:[\\/]/.test(v) || v.startsWith("\\\\");
export const imgUrl = (v: string | null): string | null =>
  v && isPath(v) ? convertFileSrc(v).replace(/\(/g, "%28").replace(/\)/g, "%29") : v;
const withGameImages = (g: Game): Game => ({ ...g, cover: imgUrl(g.cover), background: imgUrl(g.background), logo: imgUrl(g.logo) });
const withZereiImages = (e: ZereiEntry): ZereiEntry => ({ ...e, cover: imgUrl(e.cover), logo: imgUrl(e.logo), background: imgUrl(e.background) });
const withEmuImage = (e: Emulator): Emulator => ({ ...e, cover: imgUrl(e.cover) });

export type FeedbackCategory = "improvement" | "feature" | "bug" | "general";
export interface FeedbackInfo { configured: boolean; version: string; logDir: string }
export interface FeedbackAttachment { name: string; description: string; content: string; bytes: number }
export interface FeedbackInput {
  category: FeedbackCategory; title: string; comments: string; problem: string; sentAt: string; language: string;
  attachments: { name: string; content: string }[];
}

export const api = {
  listProfiles: () => invoke<Profile[]>("list_profiles"),
  createProfile: (input: ProfileInput) => invoke<Profile>("create_profile", { input }),
  updateProfile: (id: number, input: ProfileInput) => invoke<Profile>("update_profile", { id, input }),
  deleteProfile: (id: number) => invoke<void>("delete_profile", { id }),

  listLibrary: (profileId: number) => invoke<Game[]>("list_library", { profileId }).then((l) => l.map(withGameImages)),
  addGame: (profileId: number, input: GameInput) => invoke<Game>("add_game", { profileId, input }).then(withGameImages),
  updateGame: (profileId: number, gameId: number, input: GameInput) =>
    invoke<Game>("update_game", { profileId, gameId, input }).then(withGameImages),
  removeGame: (profileId: number, gameId: number) => invoke<void>("remove_game", { profileId, gameId }),
  setFavorite: (profileId: number, gameId: number, favorite: boolean) =>
    invoke<void>("set_favorite", { profileId, gameId, favorite }),
  launchGame: (profileId: number, gameId: number) => invoke<void>("launch_game", { profileId, gameId }),
  /** `color`: opção geral ("#RRGGBB" ou "off"); `theme`: cor de destaque (controles em "Cor do tema"). */
  setPadLed: (color: string, theme?: string) => invoke<void>("set_pad_led", { color, theme }),
  checkLaunch: (profileId: number, gameId: number) => invoke<void>("check_launch", { profileId, gameId }),
  gamebootWindow: (on: boolean) => invoke<void>("gameboot_window", { on }),

  getGameInfo: (profileId: number, gameId: number) =>
    invoke<GameInfo | null>("get_game_info", { profileId, gameId }),
  /** Lista de jogos da loja da Steam para o nome (o usuário escolhe um). */
  steamStoreSearch: (term: string) => invoke<StoreHit[]>("steam_store_search", { term }),
  /** Informações do jogo escolhido (appid) na loja da Steam. */
  fetchGameInfo: (profileId: number, gameId: number, term: string, appid: number | null = null) =>
    invoke<GameInfo>("fetch_game_info", { profileId, gameId, term, appid }),
  applySteamCover: (profileId: number, gameId: number) =>
    invoke<void>("apply_steam_cover", { profileId, gameId }),
  saveManualHltb: (profileId: number, gameId: number, main: number | null, extra: number | null, completionist: number | null, all: number | null) =>
    invoke<GameInfo>("save_manual_hltb", { profileId, gameId, main, extra, completionist, all }),
  // Metacritic (via loja da Steam e RAWG; o site do Metacritic não é lido)
  fetchMetacritic: (profileId: number, gameId: number, term: string, rawgId: number | null = null, reidentify = false) =>
    invoke<MetacriticResult>("fetch_metacritic", { profileId, gameId, term, rawgId, reidentify }),
  saveManualMetacritic: (profileId: number, gameId: number, score: number | null) =>
    invoke<GameInfo>("save_manual_metacritic", { profileId, gameId, score }),
  // Central de fontes de dados (credenciais só vão ao backend, cifradas pelo Windows; nunca voltam)
  sourcesList: () => invoke<SourceView[]>("sources_list"),
  sourceSetEnabled: (id: SourceId, enabled: boolean) => invoke<SourceView>("source_set_enabled", { id, enabled }),
  sourceConnect: (id: SourceId, fields: Record<string, string>) => invoke<SourceView>("source_connect", { id, fields }),
  sourceTest: (id: SourceId) => invoke<SourceView>("source_test", { id }),
  sourceDisconnect: (id: SourceId) => invoke<SourceView>("source_disconnect", { id }),
  sourceClearCache: (id: SourceId) => invoke<SourceView>("source_clear_cache", { id }),
  igdbFetch: (profileId: number, gameId: number, term: string, igdbId: number | null = null, reidentify = false) =>
    invoke<IgdbResult>("igdb_fetch", { profileId, gameId, term, igdbId, reidentify }),
  igdbApplyImage: (profileId: number, gameId: number, kind: "cover" | "background", url: string) =>
    invoke<void>("igdb_apply_image", { profileId, gameId, kind, url }),
  nexusLinkStatus: (profileId: number, gameId: number) => invoke<NexusLinkStatus>("nexus_link_status", { profileId, gameId }),
  nexusSearchGames: (term: string) => invoke<NexusGame[]>("nexus_search_games", { term }),
  nexusSetLink: (profileId: number, gameId: number, g: NexusGame | null) =>
    invoke<void>("nexus_set_link", { profileId, gameId, domain: g?.domain ?? null, nexusGameId: g?.id ?? null, name: g?.name ?? null }),
  nexusMods: (profileId: number, gameId: number, query: string, sort: ModSort, category: string, offset: number) =>
    invoke<NexusModsPage>("nexus_mods", { profileId, gameId, query, sort, category, offset }),
  nexusModDetail: (nexusGameId: number, modId: number) => invoke<NexusModDetail>("nexus_mod_detail", { nexusGameId, modId }),
  nexusCategories: (domain: string) => invoke<string[]>("nexus_categories", { domain }),
  nexusFavorites: (profileId: number, domain: string | null) => invoke<NexusMod[]>("nexus_favorites", { profileId, domain }),
  nexusToggleFavorite: (profileId: number, item: NexusMod) => invoke<boolean>("nexus_toggle_favorite", { profileId, item }),
  autoClassify: (profileId: number, gameId: number, term: string) =>
    invoke<ClassifyResult>("auto_classify", { profileId, gameId, term }),
  saveManualInfo: (profileId: number, gameId: number, description: string, genres: string[]) =>
    invoke<GameInfo>("save_manual_info", { profileId, gameId, description, genres }),
  searchGames: (term: string, platform: string) => invoke<GameHit[]>("search_games", { term, platform }),
  gameImages: (sources: SourceRef[]) => invoke<ImageSet>("game_images", { sources }),
  addWebScreenshots: (profileId: number, gameId: number, shots: WebShot[]) =>
    invoke<GameInfo>("add_web_screenshots", { profileId, gameId, shots }),
  fetchImageData: (url: string) => invoke<string>("fetch_image_data", { url }),

  // Zerei!
  zereiSearch: (term: string) => invoke<ZereiSearch>("zerei_search", { term }),
  /** Jogos do SteamGridDB para um nome (escolha do jogo antes das logos). */
  sgdbGames: (term: string) => invoke<LogoGames>("sgdb_games", { term }),
  zereiArtwork: (title: string, year: number | null, sgdbId: number | null) => invoke<ZereiArt>("zerei_artwork", { title, year, sgdbId }),
  zereiList: (profileId: number) => invoke<ZereiEntry[]>("zerei_list", { profileId }).then((l) => l.map(withZereiImages)),
  /** "Marcar como zerado": inclui o jogo da biblioteca no Zerei! como zerado (ou marca o registro existente). */
  zereiMarkGame: (profileId: number, gameId: number) => invoke<ZereiEntry>("zerei_mark_game", { profileId, gameId }),
  /** Logos automáticas (SteamGridDB) para jogos sem logo; `retry` pesquisa de novo os que ficaram sem resultado. */
  autoLogos: (profileId: number, retry: boolean) => invoke<{ filled: number; notFound: number; interrupted: boolean }>("auto_logos", { profileId, retry }),
  zereiSave: (profileId: number, id: number | null, input: ZereiInput, newRun = false) =>
    invoke<ZereiEntry>("zerei_save", { profileId, id, input, newRun }).then(withZereiImages),
  zereiDelete: (profileId: number, id: number) => invoke<void>("zerei_delete", { profileId, id }),
  /** Capa/logo/fundo automáticos (1º resultado). ids null = todos; overwrite=false só preenche o que falta. */
  zereiRefreshImages: (profileId: number, ids: number[] | null, overwrite: boolean) =>
    invoke<ZereiRefreshReport>("zerei_refresh_images", { profileId, ids, overwrite }),
  zereiRefreshCancel: () => invoke<void>("zerei_refresh_cancel"),
  zereiBulkDelete: (profileId: number, ids: number[]) => invoke<number>("zerei_bulk_delete", { profileId, ids }),
  zereiBulkStatus: (profileId: number, ids: number[], status: ZereiStatus) => invoke<number>("zerei_bulk_status", { profileId, ids, status }),
  zereiExportCsv: (profileId: number, path: string) => invoke<number>("zerei_export_csv", { profileId, path }),
  zereiImportCsv: (profileId: number, path: string) => invoke<ZereiImport>("zerei_import_csv", {
    profileId, path,
    // anotações que a importação acrescenta aos comentários, no idioma da interface
    labels: { howTo: t("Como zerar"), difficulty: t("Dificuldade"), genre: t("Gênero"), started: t("Data de início informada") },
  }),
  removeGameScreenshot: (profileId: number, gameId: number, url: string) =>
    invoke<GameInfo>("remove_game_screenshot", { profileId, gameId, url }),
  restoreRemovedScreenshots: (profileId: number, gameId: number) =>
    invoke<GameInfo>("restore_removed_screenshots", { profileId, gameId }),

  listCollections: (profileId: number) => invoke<GameCollection[]>("list_collections", { profileId }),
  createCollection: (profileId: number, name: string) =>
    invoke<GameCollection>("create_collection", { profileId, name }),
  renameCollection: (profileId: number, id: number, name: string) =>
    invoke<void>("rename_collection", { profileId, id, name }),
  deleteCollection: (profileId: number, id: number) => invoke<void>("delete_collection", { profileId, id }),
  setInCollection: (profileId: number, collectionId: number, gameId: number, member: boolean) =>
    invoke<void>("set_in_collection", { profileId, collectionId, gameId, member }),

  getControllers: () => invoke<Controller[]>("get_controllers"),
  /** Ajustes individuais por dispositivo (Central de Controles). Só a navegação do NEXSTAGE usa. */
  setPadConfigs: (configs: Record<string, { dzLeft: number; dzRight: number; map: number[]; rumble: number; rumbleNav: boolean; led: number }>) =>
    invoke<void>("set_pad_configs", { configs }),
  /** Testador: liga o envio do estado bruto (pad-state) e pausa a navegação pelos controles nativos. */
  setPadMonitor: (on: boolean) => invoke<void>("set_pad_monitor", { on }),
  /** Teclado virtual: segurar confirmar/X/Y/L1/R1 repete o comando (controles nativos). */
  setPadButtonRepeat: (on: boolean) => invoke<void>("set_pad_button_repeat", { on }),
  padRumble: (key: string, strength: number, ms: number) => invoke<void>("pad_rumble", { key, strength, ms }),
  /** Pares de Joy-Cons gravados ([esquerdo, direito]): o backend junta os dois num controle só. */
  /** Nome personalizado de cada controle cadastrado (avisos de conexão e bateria). */
  setPadNames: (names: Record<string, string>) => invoke<void>("set_pad_names", { names }),
  setJoyconPairs: (pairs: [string, string][]) => invoke<void>("set_joycon_pairs", { pairs }),

  getOverlayShortcut: () => invoke<ShortcutInfo>("get_overlay_shortcut"),
  setOverlayShortcut: (accelerator: string) => invoke<ShortcutInfo>("set_overlay_shortcut", { accelerator }),
  resetOverlayShortcut: () => invoke<ShortcutInfo>("reset_overlay_shortcut"),
  /** Sair/reiniciar/desligar fazem antes o backup automático "antes de encerrar" (se ativo); `skipBackup` pula. */
  powerAction: (action: "exit" | "minimize" | "restart" | "shutdown", skipBackup = false) => invoke<void>("power_action", { action, skipBackup }),
  getStorage: () => invoke<Drive[]>("get_storage"),
  importScreenshots: (profileId: number, gameId: number | null, paths: string[]) =>
    idleBusy(invoke<ImportResult>("import_screenshots", { profileId, gameId, paths })),
  listGallery: (profileId: number) => invoke<GalleryItem[]>("list_gallery", { profileId }),
  setGalleryFavorite: (profileId: number, id: number, favorite: boolean) =>
    invoke<void>("set_gallery_favorite", { profileId, id, favorite }),
  deleteGalleryItem: (profileId: number, id: number) => invoke<void>("delete_gallery_item", { profileId, id }),

  profileActivity: (profileId: number) => invoke<Activity>("profile_activity", { profileId }),
  deleteGalleryItems: (profileId: number, ids: number[]) =>
    invoke<number>("delete_gallery_items", { profileId, ids }),
  revealCapture: (profileId: number, id: number) => invoke<void>("reveal_capture", { profileId, id }),
  fandomLink: (title: string) => invoke<FandomLink>("fandom_link", { title }),
  getStartupInfo: () => invoke<StartupInfo>("get_startup_info"),
  setStartupMode: (mode: string) => invoke<StartupInfo>("set_startup_mode", { mode }),
  /** Monitores conectados + monitor preferido salvo (pode estar desconectado). */
  // Navegador de arquivos do app (só leitura)
  fsPlaces: () => invoke<FsPlaces>("fs_places"),
  fsList: (path: string, exts: string[], dirsOnly: boolean, showHidden: boolean) => invoke<FsListing>("fs_list", { path, exts, dirsOnly, showHidden }),
  fsPathInfo: (path: string) => invoke<{ exists: boolean; dir: boolean }>("fs_path_info", { path }),
  fsThumb: (path: string, size: number) => invoke<string>("fs_thumb", { path, size }),
  fsReadImage: (path: string) => invoke<string>("fs_read_image", { path }),
  /** Abre o Explorador na pasta do jogo (arquivo selecionado; atalho → original; Steam → pasta de instalação). */
  revealGameFolder: (profileId: number, gameId: number) => invoke<void>("reveal_game_folder", { profileId, gameId }),
  getMonitors: () => invoke<MonitorState>("get_monitors"),
  /** null: sem preferência. Com id: salva e leva a janela para ele. */
  setPreferredMonitor: (id: string | null) => invoke<MonitorState>("set_preferred_monitor", { id }),
  getScreenshotShortcut: () => invoke<ShortcutInfo>("get_screenshot_shortcut"),
  setScreenshotShortcut: (accelerator: string) => invoke<ShortcutInfo>("set_screenshot_shortcut", { accelerator }),
  resetScreenshotShortcut: () => invoke<ShortcutInfo>("reset_screenshot_shortcut"),
  setActiveProfile: (id: number) => invoke<void>("set_active_profile", { id }),
  getPadCombos: () => invoke<PadCombos>("get_pad_combos"),
  setPadCombo: (slot: "overlay" | "screenshot" | "summon", mask: number) => invoke<PadCombos>("set_pad_combo", { slot, mask }),
  resetPadCombo: (slot: "overlay" | "screenshot" | "summon") => invoke<PadCombos>("reset_pad_combo", { slot }),
  setPadCapture: (on: boolean) => invoke<void>("set_pad_capture", { on }),
  takeScreenshot: () => invoke<string>("take_screenshot"),
  capturesFolder: () => invoke<string>("captures_folder"),
  openCapturesFolder: () => invoke<string>("open_captures_folder"),
  listEmulators: () => invoke<Emulator[]>("list_emulators").then((l) => l.map(withEmuImage)),
  saveEmulator: (input: { id?: number; name: string; exePath: string; platforms: string[]; formats: string[]; args: string; cover: string | null }) =>
    invoke<Emulator>("save_emulator", { input }).then(withEmuImage),
  deleteEmulator: (id: number) => invoke<void>("delete_emulator", { id }),
  emulatorFullscreenSupported: (name: string, exePath: string) =>
    invoke<boolean>("emulator_fullscreen_supported", { name, exePath }),
  gameTrophies: (profileId: number, gameId: number) =>
    invoke<import("../types").GameTrophies | null>("game_trophies", { profileId, gameId }),
  trophyOverview: (profileId: number) =>
    invoke<import("../types").TrophySetSummary[]>("trophy_overview", { profileId }),
  profileScore: (profileId: number) => invoke<import("../types").ProfileScore>("profile_score", { profileId }),
  setGameEmulator: (profileId: number, gameId: number, emulatorId: number | null) =>
    invoke<void>("set_game_emulator", { profileId, gameId, emulatorId }),
  scanFolder: (profileId: number, path: string, folderId: number | null) =>
    invoke<void>("scan_folder", { profileId, path, folderId }),
  cancelScan: () => invoke<void>("cancel_scan"),
  /** Identifica um arquivo escolhido em "Adicionar jogo" (mesma detecção da varredura; não importa nada). */
  inspectGameFile: (profileId: number, path: string) => invoke<ScanItem>("inspect_game_file", { profileId, path }),
  importScanItems: (profileId: number, folderPath: string, items: { path: string; ext: string; platform: string; title: string; emulatorId: number | null }[]) =>
    idleBusy(invoke<ImportSummary>("import_scan_items", { profileId, folderPath, items })),
  listLibraryFolders: (profileId: number) => invoke<FolderRow[]>("list_library_folders", { profileId }),
  removeLibraryFolder: (profileId: number, id: number) => invoke<void>("remove_library_folder", { profileId, id }),
  // Estatísticas consolidadas (somente cache local; nenhuma chamada de rede)
  gamerStats: (profileId: number) => invoke<GamerStats>("gamer_stats", { profileId }),
  publicGamerStats: (profileId: number) => invoke<GamerStats | null>("public_gamer_stats", { profileId }),
  getStatsVisibility: (profileId: number) => invoke<StatsVisibility>("get_stats_visibility", { profileId }),
  setStatsVisibility: (profileId: number, visibility: StatsVisibility) => invoke<StatsVisibility>("set_stats_visibility", { profileId, visibility }),

  getAppSetting: (key: string) => invoke<string | null>("get_app_setting", { key }),
  setAppSetting: (key: string, value: string) => invoke<void>("set_app_setting", { key, value }),
  getProfileSetting: (profileId: number, key: string) =>
    invoke<string | null>("get_profile_setting", { profileId, key }),
  setProfileSetting: (profileId: number, key: string, value: string) =>
    invoke<void>("set_profile_setting", { profileId, key, value }),
  /** Fundos da Home (por perfil): imagem própria e captura da Galeria, como URLs prontas (ou null). */
  getHomeBackground: (profileId: number) =>
    invoke<{ image: string | null; capture: string | null }>("get_home_background", { profileId })
      .then((r) => ({ image: imgUrl(r.image), capture: imgUrl(r.capture) })),
  /** Grava a imagem própria (data URL) ou remove (null); `gallery` remove a da Galeria. Devolve a URL gravada. */
  setHomeBackground: (profileId: number, image: string | null, gallery = false) =>
    invoke<string | null>("set_home_background", { profileId, image, gallery }).then(imgUrl),
  /** Usa uma captura da Galeria (cópia) como fundo da Home; devolve a URL da imagem gravada. */
  setHomeBackgroundFromCapture: (profileId: number, captureId: number) =>
    invoke<string | null>("set_home_background_from_capture", { profileId, captureId }).then(imgUrl),
  runningGames: () => invoke<number[]>("running_games"),
  quitGame: (gameId: number) => invoke<void>("quit_game", { gameId }),
  closeOverlay: () => invoke<void>("close_overlay"),
  setFullscreen: (enabled: boolean) => invoke<void>("set_fullscreen", { enabled }),
  /** Alterna tela cheia ⇄ janela (Alt+Enter) e salva como preferência; avisa as telas abertas. */
  toggleFullscreen: async () => {
    const next = !(await getCurrentWindow().isFullscreen());
    await invoke<void>("set_fullscreen", { enabled: next });
    await invoke<void>("set_app_setting", { key: "fullscreen", value: next ? "1" : "0" });
    window.dispatchEvent(new CustomEvent("nexstage:fullscreen", { detail: next }));
    return next;
  },
  /** Estado REAL da janela (a configuração "fullscreen" é só a preferência salva; o app pode estar em
   * tela cheia pelo modo de abertura, pelo Gameboot etc.). */
  isFullscreen: () => getCurrentWindow().isFullscreen(),
  installerLanguage: () => invoke<string | null>("installer_language"),

  // RetroAchievements (a chave da API só é enviada em raConnect e nunca volta do backend)
  raGetAccount: (profileId: number) => invoke<RaAccount | null>("ra_get_account", { profileId }),
  raConnect: (profileId: number, username: string, apiKey: string) => invoke<RaAccount>("ra_connect", { profileId, username, apiKey }),
  raTest: (profileId: number) => invoke<RaAccount>("ra_test", { profileId }),
  raDisconnect: (profileId: number, clearCache: boolean) => invoke<void>("ra_disconnect", { profileId, clearCache }),
  raSync: (profileId: number) => idleBusy(invoke<RaSyncReport>("ra_sync", { profileId })),
  raOverview: (profileId: number) => invoke<RaOverview>("ra_overview", { profileId }),
  raListAchievements: (profileId: number, filter: "recent" | "earned" | "pending" | "hardcore", query: string, raGameId: number | null = null) =>
    invoke<RaAchievement[]>("ra_list_achievements", { profileId, filter, raGameId, query }),
  raGame: (profileId: number, raGameId: number, refresh = false) => invoke<RaGameDetail>("ra_game", { profileId, raGameId, refresh }),
  raLinkStatus: (profileId: number, gameId: number) => invoke<RaLinkInfo>("ra_link_status", { profileId, gameId }),
  raSetLink: (profileId: number, gameId: number, raGameId: number | null, unsupported = false) =>
    invoke<void>("ra_set_link", { profileId, gameId, raGameId, unsupported }),
  raSearchGames: (profileId: number, platform: string, query: string) => invoke<RaGame[]>("ra_search_games", { profileId, platform, query }),
  raScanLibrary: (profileId: number) => idleBusy(invoke<RaScanReport>("ra_scan_library", { profileId })),
  raEmulatorSupport: () => invoke<RaEmuSupport[]>("ra_emulator_support"),

  // Steam (login oficial OpenID; nenhuma senha/cookie/token passa pelo NEXSTAGE)
  steamGetAccount: (profileId: number) => invoke<SteamAccount | null>("steam_get_account", { profileId }),
  steamLogin: (profileId: number) => invoke<SteamAccount>("steam_login", { profileId }),
  steamCancelLogin: () => invoke<void>("steam_cancel_login"),
  // Xbox (o refresh token nunca volta do backend)
  xboxStatus: (profileId: number) => invoke<import("../types").XboxStatus>("xbox_status", { profileId }),
  xboxSetClientId: (clientId: string | null) => invoke<void>("xbox_set_client_id", { clientId }),
  xboxLogin: (profileId: number) => invoke<import("../types").XboxAccount>("xbox_login", { profileId }),
  xboxCancelLogin: () => invoke<void>("xbox_cancel_login"),
  xboxDisconnect: (profileId: number, clearCache: boolean) => invoke<void>("xbox_disconnect", { profileId, clearCache }),
  xboxSync: (profileId: number) => idleBusy(invoke<{ titles: number }>("xbox_sync", { profileId, lang: currentLang() })),
  xboxGame: (profileId: number, gameId: number, refresh = false) =>
    invoke<import("../types").XboxGameData | null>("xbox_game", { profileId, gameId, refresh, lang: currentLang() }),
  xboxInstalled: (profileId: number) => invoke<import("../types").XboxInstalled[]>("xbox_installed", { profileId }),
  xboxImport: (profileId: number, aumids: string[]) => invoke<{ added: number; skipped: number }>("xbox_import", { profileId, aumids }),
  steamSetKey: (profileId: number, key: string | null) => invoke<SteamAccount>("steam_set_key", { profileId, key }),
  steamDisconnect: (profileId: number, clearCache: boolean) => invoke<void>("steam_disconnect", { profileId, clearCache }),
  steamSync: (profileId: number) => idleBusy(invoke<SteamSyncReport>("steam_sync", { profileId, lang: currentLang() })),
  steamOwned: (profileId: number) => invoke<SteamOwnedList>("steam_owned", { profileId }),
  steamPreview: (profileId: number, appids: number[]) => invoke<SteamPreviewItem[]>("steam_preview", { profileId, appids }),
  steamImport: (profileId: number, appids: number[], linkTitleMatches: boolean) =>
    idleBusy(invoke<SteamImportReport>("steam_import", { profileId, appids, linkTitleMatches })),
  steamSetLink: (profileId: number, gameId: number, appid: number | null) => invoke<void>("steam_set_link", { profileId, gameId, appid }),
  steamAppStatus: (appid: number) => invoke<SteamAppStatus>("steam_app_status", { appid }),
  steamInstall: (appid: number) => invoke<"client" | "store">("steam_install", { appid }),
  steamOpenPage: (appid: number) => invoke<void>("steam_open_page", { appid }),
  steamGame: (profileId: number, appid: number, refresh = false) =>
    invoke<[SteamGameData, string | null]>("steam_game", { profileId, appid, refresh, lang: currentLang() }),
  steamClientStatus: () => invoke<SteamClientStatus>("steam_client_status"),
  previewPadToast: (event: "connected" | "disconnected" | "battery", name: string) => invoke<void>("preview_pad_toast", { event, name }),
  // Feedback e relatórios (Configurações → Atualizações)
  logClientError: (source: string, message: string) => invoke<void>("log_client_error", { source, message }),
  feedbackInfo: () => invoke<FeedbackInfo>("feedback_info"),
  feedbackAttachments: () => invoke<FeedbackAttachment[]>("feedback_attachments"),
  feedbackSend: (input: FeedbackInput) => invoke<{ id: number | null }>("feedback_send", { input }),
  previewAchievementToast: (rare: boolean, title: string, game: string) => invoke<void>("preview_achievement_toast", { rare, title, game }),
  // Últimas atualizações (notas oficiais da Steam; leitura só do cache, verificação em segundo plano)
  steamNewsOverview: (profileId: number) =>
    invoke<SteamNewsOverview>("steam_news_overview", { profileId }).then((o) => ({ ...o, items: o.items.map((i) => ({ ...i, cover: imgUrl(i.cover) })) })),
  steamNewsRefresh: (profileId: number, manual: boolean) => invoke<boolean>("steam_news_refresh", { profileId, manual }),
  steamNewsSeen: (profileId: number, gid: string) => invoke<void>("steam_news_seen", { profileId, gid }),
  steamAchOverview: (profileId: number) => invoke<SteamAchOverview>("steam_ach_overview", { profileId }),
  steamAchList: (profileId: number, tab: "recent" | "earned" | "pending" | "rare", query: string) =>
    invoke<SteamAchEntry[]>("steam_ach_list", { profileId, tab, query }),

  // Backup e restauração (ver src-tauri/src/backup)
  backupInfo: () => invoke<BackupInfo>("backup_info"),
  backupTargetStatus: (dir: string) => invoke<BackupTarget>("backup_target_status", { dir }),
  backupCreate: (dir: string, password: string | null, includeCaptures: boolean, replace: boolean) =>
    idleBusy(invoke<BackupReport>("backup_create", { dir, password, includeCaptures, replace })),
  backupCancel: () => invoke<boolean>("backup_cancel"),
  backupInspect: (path: string, password: string | null) => invoke<BackupInspect>("backup_inspect", { path, password }),
  backupVerify: (path: string) => idleBusy(invoke<BackupVerify>("backup_verify", { path })),
  backupRestore: (path: string, password: string | null) =>
    idleBusy(invoke<BackupRestoreReport>("backup_restore", { path, password })),
  backupRestart: () => invoke<void>("backup_restart"),
  backupAutoGet: () => invoke<BackupAuto>("backup_auto_get"),
  backupAutoSet: (mode: BackupAutoMode, dir: string | null, keep: number, includeCaptures: boolean) =>
    invoke<BackupAuto>("backup_auto_set", { mode, dir, keep, includeCaptures }),
  backupAutoPassword: (password: string | null) => invoke<BackupAuto>("backup_auto_password", { password }),
  backupAutoRun: () => idleBusy(invoke<BackupReport>("backup_auto_run")),
  backupReveal: (path: string) => invoke<void>("backup_reveal", { path }),
};

/** Erros do backend chegam como string; outros erros como Error. */
export function errMsg(e: unknown): string {
  if (typeof e === "string") return t(e); // mensagens fixas do backend têm tradução nos dicionários
  if (e instanceof Error) return e.message;
  return t("Ocorreu um erro inesperado.");
}
