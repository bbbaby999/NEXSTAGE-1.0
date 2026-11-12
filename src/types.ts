export interface Profile {
  id: number;
  username: string;
  displayName: string;
  bio: string;
  avatar: string | null;
  banner: string | null;
  favoriteGame: string | null;
  favoritePlatform: string | null;
  createdAt: string;
}
export type ProfileInput = Omit<Profile, "id" | "createdAt">;

export interface Game {
  id: number;
  title: string;
  platform: string;
  executablePath: string;
  launchArgs: string;
  cover: string | null;
  favorite: boolean;
  playtimeSeconds: number;
  lastPlayedAt: string | null;
  addedAt: string;
  background: string | null;
  bgX: number;
  bgY: number;
  steamId: string | null;
  releaseDate: string | null;
  releaseIso: string | null;
  genre: string | null;
  genres: string[];
  completed: boolean;
  completedAt: string | null;
  isoPath: string | null;
  romPath: string | null;
  emulatorId: number | null;
  emulatorName: string | null;
  /** Jogo do RetroAchievements associado (por hash oficial ou confirmado pelo usuário). */
  raGameId: number | null;
  /** Origem: "manual", "iso", "folder", "steam"... */
  source: string;
  /** AppID da Steam (importado da Steam ou vinculado). */
  steamAppId: number | null;
  /** Pronto para jogar neste PC (Steam: instalado pelo cliente; demais: arquivo presente). */
  installed: boolean;
}
export type GameInput = Pick<Game, "title" | "platform" | "executablePath" | "launchArgs" | "cover" | "background" | "bgX" | "bgY" | "isoPath">;


export interface Hltb {
  source: string;
  gameId: number | null;
  name: string;
  mainSeconds: number | null;
  extraSeconds: number | null;
  completionistSeconds: number | null;
  allSeconds: number | null;
  updatedAt?: string | null;
}

export interface GameInfo {
  hltb: Hltb | null;
  source: string;
  externalId: string | null;
  matchedName: string;
  description: string;
  genres: string[];
  developers: string[];
  releaseDate: string | null;
  metacriticScore: number | null;
  metacriticUrl: string | null;
  /** "steam" | "rawg" | "manual" */
  metacriticSource?: string | null;
  metacriticPlatform?: string | null;
  metacriticCheckedAt?: string | null;
  /** Jogo confirmado no RAWG. */
  rawgId?: number | null;
  metacriticOverall?: number | null;
  metacriticOverallUrl?: string | null;
  /** Metascore de cada versão (plataforma) do jogo. */
  metacriticPlatforms?: { slug: string; name: string; score: number; url: string | null }[];
  /** Dados do IGDB (cache; não sobrescrevem campos personalizados). */
  igdb?: IgdbData | null;
  reviewSummary: string | null;
  reviewPositive: number | null;
  reviewNegative: number | null;
  screenshots: string[];
  headerImage: string | null;
  storeUrl: string | null;
  /** Screenshots de outras fontes (libretro, Wikimedia Commons); vazio se nunca pesquisado. */
  webScreenshots?: WebShot[];
  /** URLs removidas da galeria do NEXSTAGE (não voltam em novas buscas). */
  removedScreenshots?: string[];
}

/** Jogo encontrado na loja da Steam (para o usuário escolher). */
export interface StoreHit { appid: number; name: string; image: string | null; price: string | null; platforms: string[] }
export interface MetacriticCandidate { id: number; name: string; year: number | null; platforms: string[]; metacritic: number | null; image: string | null; onPlatform: boolean }
export interface MetacriticResult { status: "found" | "no_score" | "choose" | "not_found" | "no_source"; info: GameInfo; matchedName: string | null; candidates: MetacriticCandidate[] }

export interface WebShot {
  url: string;
  thumb: string;
  source: "libretro" | "wikipedia" | "wikimedia" | string;
  page: string | null;
  title: string;
  kind: "snap" | "title" | "image" | string;
}

export interface GameCollection { id: number; name: string; gameIds: number[] }

export interface Controller {
  /**
   * Controle FÍSICO (persistente entre reconexões): "xbox:<id>" (XInput; MAC/serial/ContainerId do
   * aparelho no slot — "xbox:<slot>" só sem identificação), "ps:<mac>" / "ns:<mac>" (Sony / Nintendo por
   * HID), "<prefixo>:v<hash>" (virtuais) ou "gp:<id da Gamepad API>[#n]" (genéricos; #n = 2º, 3º igual).
   */
  id: string;
  name: string;
  kind: string;
  batteryPercent: number | null;
  batteryLabel: string | null;
  charging: boolean;
  wired: boolean;
  /** "usb" | "bluetooth" | "wireless"; null quando não se sabe. */
  connection?: string | null;
  /** Modelo pelo VID/PID ("dualsense", "dualshock4", "xbox-series", "sony-unknown", "virtual"...). */
  model?: string | null;
  brand?: string | null;
  vid?: number | null;
  pid?: number | null;
  /** ContainerId do Windows: o mesmo para todas as interfaces do aparelho físico. */
  physicalId?: string | null;
  /** Origem do identificador: "mac" | "serial" | "container" | "path" | "slot" | "virtual". */
  idSource?: string | null;
  /** Programa que criou o dispositivo, quando é virtual (ViGEm, Logitech G HUB...). */
  virtualSource?: string | null;
  /** Chave de versões anteriores ("xbox:<slot>"), para migrar cadastros. */
  legacyKey?: string | null;
}

export type View =
  | "home" | "library" | "emulators" | "collection"
  | "stats" | "settings" | "gallery" | "profile" | "achievements" | "controllers";

/** Plataformas oficiais (mesma ordem do backend). */
export const PLATFORMS = [
  "PC", "XBOX", "Xbox 360", "Xbox Series", "Playstation 1", "Playstation 2", "Playstation 3", "Playstation 4",
  "Playstation 5", "PSP", "Ps Vita", "Nintendo Switch", "Nintendo Switch 2", "Super Nintendo", "Nintendinho",
];

export interface GalleryItem {
  id: number;
  gameId: number | null;
  gameTitle: string | null;
  path: string;
  favorite: boolean;
  takenAt: string | null;
  addedAt: string;
  exists: boolean;
  originalName: string;
}
export interface ImportResult { imported: number; duplicates: number; failed: string[] }
export interface Drive {
  name: string; mount: string; kind: string; fileSystem: string; removable: boolean; accessible: boolean;
  totalBytes: number; freeBytes: number; usedBytes: number; nexstageBytes: number; screenshotsBytes: number;
}
export interface ShortcutInfo { accelerator: string; isDefault: boolean; active: boolean }

export interface ClassifyResult { status: "updated" | "skipped" | "uncertain"; matchedName: string | null; genres: string[] }

export interface SessionRow { gameId: number; title: string; startedAt: string; durationSeconds: number }
export interface CompletedRow { gameId: number; title: string; completedAt: string }
export interface Activity {
  sessions: SessionRow[];
  completed: CompletedRow[];
  month: { sessions: number; seconds: number; games: number };
}

export interface StartupInfo { mode: "none" | "minimized" | "window" | "fullscreen"; autostartEnabled: boolean; devBuild: boolean }
export interface PadCombos { overlay: number; screenshot: number; overlayDefault: number; screenshotDefault: number }
export interface FandomLink { url: string; direct: boolean }

export interface Emulator { id: number; name: string; exePath: string; platforms: string[]; formats: string[]; args: string; cover: string | null; exeExists: boolean; games: number }
export interface ScanItem {
  path: string; name: string; ext: string; kind: "windows" | "disc" | "switch" | "pkg" | "vpk";
  platform: string | null; status: "new" | "secondary" | "duplicate" | "invalid" | "unrecognized";
  note: string; size: number; compatible: number[]; assigned: number | null;
}
export interface ScanProgress { phase: "walking" | "analyzing"; scanned: number; found: number; current: string }
export interface ScanDone {
  folder: string; folderId: number | null; items: ScanItem[]; scanned: number; ignored: number;
  errors: string[]; cancelled: boolean; missing: { gameId: number; title: string; path: string }[];
}
export interface ImportSummary { imported: number; duplicates: number; failed: string[]; folderId: number }
export interface FolderRow { id: number; path: string; addedAt: string; lastScanAt: string | null; games: number; exists: boolean }

/** Pesquisa de imagens: onde buscar as imagens de um jogo encontrado. */
export interface SourceRef { source: string; id: string }
/** Jogo encontrado na pesquisa (pode reunir várias fontes). */
export interface GameHit { title: string; platform: string | null; year: number | null; cover: string | null; sources: SourceRef[] }
export interface ImageSet { covers: WebShot[]; backgrounds: WebShot[]; screenshots: WebShot[] }

/* ---------- RetroAchievements ---------- */
export interface RaProfile {
  user: string; userPic: string | null; memberSince: string | null; motto: string | null; richPresence: string | null;
  lastGameId: number; totalPoints: number; totalSoftcorePoints: number; totalTruePoints: number;
}
export interface RaAccount {
  username: string; hasKey: boolean; profile: RaProfile | null; syncedAt: string | null; syncError: string | null;
  syncing: boolean; blockedSecs: number | null; profileUrl: string;
}
export interface RaAchievement {
  id: number; gameId: number; gameTitle: string; consoleName: string; title: string; description: string; points: number;
  trueRatio: number; badgeUrl: string | null; kind: string | null; unlockedAt: string | null; unlockedHcAt: string | null; url: string;
  /** Só nos avisos de desbloqueio. */
  hardcore?: boolean;
}
export interface RaGame {
  id: number; title: string; consoleId: number; consoleName: string; iconUrl: string | null; numAchievements: number;
  pointsTotal: number; awarded: number; awardedHc: number; pointsEarned: number; lastAwardAt: string | null;
  awardKind: string | null; hasDetails: boolean; libraryGameId: number | null; url: string;
}
export interface RaTotals { unlocked: number; unlockedHc: number; games: number; completed: number; mastered: number; beaten: number; gamesWithoutDetails: number }
export interface RaOverview { account: RaAccount | null; totals: RaTotals; recent: RaAchievement[]; games: RaGame[] }
export interface RaGameDetail { game: RaGame | null; achievements: RaAchievement[]; warning: string | null }
export interface RaSyncReport { newUnlocks: number; notified: number; gamesChanged: number; detailsFetched: number; baseline: boolean; syncedAt: string; warning: string | null }
export interface RaLinkInfo {
  gameId: number; status: "linked" | "suggested" | "none" | "unsupported" | "platform"; method: "hash" | "manual" | null;
  consoleId: number | null; raGame: RaGame | null; hash: string | null; hashError: string | null; hashMismatch: boolean;
  candidates: RaGame[]; catalogReady: boolean; catalogError: string | null;
}
export interface RaScanReport { checked: number; linked: number; suggested: number; notFound: number; errors: string[] }
export interface RaEmuSupport {
  id: number; name: string; platforms: string[]; raCapable: boolean; raEnabled: boolean | null; raUser: string | null;
  eventLog: string | null; mode: "realtime" | "polling" | "none"; note: string;
}

/* ---------- Steam ---------- */
export interface SteamProfile {
  name?: string; avatar?: string | null; profileUrl?: string | null; visibility: "public" | "friends" | "private" | "unknown";
  configured?: boolean; createdAt?: string | null; lastLogoff?: string | null; source?: "api" | "community";
}
export interface SteamAccount {
  steamId: string; name: string; profile: SteamProfile | null; profileUrl: string; hasKey: boolean; keyUnreadable: boolean;
  syncedAt: string | null; syncError: string | null; syncing: boolean; ownedCount: number; ownedSource: "api" | "local" | null;
}
export type SteamInstall = "installed" | "updating" | "installing" | "not_installed" | "unknown";
export interface SteamOwned {
  appid: number; name: string; iconUrl: string | null; coverUrl: string; headerUrl: string; playtimeForever: number | null;
  playtime2weeks: number | null; lastPlayed: string | null; recent: boolean; hasStats: boolean; source: "api" | "local";
  install: SteamInstall; inLibrary: { gameId: number; title: string; how: "linked" | "appid" | "title" } | null;
}
export interface SteamOwnedList { games: SteamOwned[]; clientFound: boolean }
export interface SteamPreviewItem { appid: number; name: string; status: "new" | "existing" | "title"; gameId: number | null; gameTitle: string | null; updates: ("link" | "playtime" | "cover" | "launch")[] }
export interface SteamImportReport { added: number; linked: number; updated: number; skipped: number }
export interface SteamSyncReport { owned: number | null; recent: number | null; libraryPrivate: boolean; achievementsUpdated: number; usedKey: boolean; warning: string | null; syncedAt: string }
export interface SteamAppStatus { appid: number; clientFound: boolean; protocol: boolean; install: SteamInstall; sizeOnDisk: number }
export interface SteamAch { apiname: string; name: string; description: string; icon: string | null; hidden: boolean; globalPct: number | null; unlockedAt: string | null }
export interface SteamGameData {
  appid: number; owned: SteamOwned | null; achievements: SteamAch[]; unlocked: number; total: number;
  stats: { name: string; label: string | null; value: number }[]; error: string | null; fetchedAt: string | null;
}
/** Conquista na central (aba Conquistas → Steam). unlockedAt: null = pendente; "" = obtida sem data. */
export interface SteamAchEntry { appid: number; gameName: string; apiname: string; name: string; description: string; icon: string | null; hidden: boolean; globalPct: number | null; unlockedAt: string | null }
export interface SteamAchGame { appid: number; name: string; iconUrl: string | null; unlocked: number; total: number; lastUnlock: string | null; libraryGameId: number | null }
export interface SteamAchTotals { unlocked: number; total: number; games: number; perfect: number; pendingGames: number }
export interface SteamAchOverview { account: SteamAccount | null; totals: SteamAchTotals; recent: SteamAchEntry[]; games: SteamAchGame[] }
export interface SteamClientStatus { found: boolean; path: string | null; protocol: boolean; libraries: number; installed: number }

/* ---------- Estatísticas gamer (consolidadas: NEXSTAGE + Steam + RetroAchievements) ---------- */
/** Origem das estatísticas. Novas plataformas (ex.: "epic", "xbox") chegam como novos ids. */
export type StatSource = "local" | "steam" | "retroachievements" | (string & {});
export interface StatSourceInfo { id: StatSource; connected: boolean; syncedAt: string | null; hasPlaytime: boolean; hasAchievements: boolean }
export interface StatAch { source: StatSource; unlocked: number; total: number; pct: number }
export interface StatGame {
  key: string; title: string; cover: string | null; icon: string | null; platform: string; libraryGameId: number | null; totalSeconds: number;
  localSeconds: number | null; steamSeconds: number | null; recentSeconds: number; lastActivity: string | null; sources: StatSource[]; achievements: StatAch[];
}
export interface StatUnlock { source: StatSource; name: string; description: string; game: string; icon: string | null; at: string | null; rarityPct: number | null; hardcore: boolean; points: number | null }
export interface StatGameAch { source: StatSource; title: string; icon: string | null; unlocked: number; total: number; pct: number; libraryGameId: number | null }
export interface StatPlatformAch { source: StatSource; unlocked: number; total: number; games: number; gamesComplete: number; pct: number }
export interface GamerStats {
  generatedAt: string; sources: StatSourceInfo[]; libraryCount: number; totalSeconds: number; localSeconds: number; steamSeconds: number | null; recentSeconds: number;
  favorite: StatGame | null; mostPlayedRecent: StatGame | null; lastGame: StatGame | null; recentGames: StatGame[]; topGames: StatGame[];
  favoritePlatform: { platform: string; seconds: number } | null; platforms: { platform: string; seconds: number }[];
  achievements: {
    /** Uma linha por plataforma (conjuntos separados). */
    platforms: StatPlatformAch[];
    ra: { unlocked: number; unlockedHc: number; total: number; games: number; mastered: number; points: number | null; pointsSoftcore: number | null } | null;
    unlocked: number; total: number; pct: number; completed: StatGameAch[]; closest: StatGameAch[]; recent: StatUnlock[]; rare: StatUnlock[];
  };
  history: { month: string; localSeconds: number; unlocks: Record<string, number> }[];
  steam: { totalSeconds: number; recentSeconds: number; owned: number; played: number; ranking: StatGame[]; recent: StatGame[]; achGames: StatGameAch[]; achUnlocked: number; achTotal: number } | null;
}
export interface StatsVisibility { mode: "private" | "public" | "custom"; summary: boolean; topGames: boolean; achievements: boolean; history: boolean; steam: boolean }
export type StatsTab = "summary" | "top" | "achievements" | "history" | "steam";

/* ---------- Fontes de dados (IGDB, HowLongToBeat, Nexus Mods) ---------- */
export type SourceId = "igdb" | "nexus";
export interface SourceView {
  id: SourceId; enabled: boolean; configured: boolean; authorized: boolean; status: "ok" | "error" | null;
  lastSync: string | null; lastError: string | null; info: Record<string, unknown>; cacheEntries: number;
}
export interface IgdbData {
  id: number; name: string; url: string | null; summary: string; storyline: string; cover: string | null; artworks: string[]; screenshots: string[];
  developers: string[]; publishers: string[]; genres: string[]; platforms: string[]; releaseDate: string | null; franchises: string[]; series: string[];
  ageRatings: string[]; videos: { id: string; name: string }[];
  timeToBeat: { hastily: number | null; normally: number | null; completely: number | null; count: number | null } | null; fetchedAt: string | null;
}
export interface IgdbCandidate { id: number; name: string; year: number | null; platforms: string[]; image: string | null; onPlatform: boolean; steamMatch: boolean }
export interface IgdbResult { status: "found" | "choose" | "not_found" | "disabled"; info: GameInfo; candidates: IgdbCandidate[] }
export interface NexusGame { id: number; name: string; domain: string; modCount: number }
export interface NexusLinkStatus { enabled: boolean; link: NexusGame | null; candidates: NexusGame[]; error: string | null }
export interface NexusMod {
  modId: number; domain: string; name: string; summary: string; picture: string | null; thumbnail: string | null; author: string; uploader: string;
  createdAt: string | null; updatedAt: string | null; downloads: number | null; endorsements: number | null; category: string | null; version: string | null;
  supportsVortex: boolean; url: string;
}
export interface NexusModsPage { mods: NexusMod[]; total: number; cached: boolean }
export interface NexusModDetail {
  files: { name: string; version: string; category: string; date: number | null; sizeKb: number | null; primary: boolean }[];
  requirements: { name: string; url: string | null; notes: string }[];
  /** Imagens da descrição do mod (colocadas pelo autor). */
  images: string[];
}
export type ModSort = "popular" | "downloads" | "endorsements" | "updated" | "new";

/** Nota de atualização oficial publicada na Steam (seção "Últimas atualizações" da Home). */
export interface SteamNewsItem {
  appid: number;
  gameId: number;
  gameTitle: string;
  cover: string | null;
  gid: string;
  title: string;
  author: string;
  /** Epoch (s) da publicação da nota. */
  date: number;
  /** BBCode da Steam. */
  contents: string;
  /** Marcada pelo desenvolvedor como Notas de atualização (senão: anúncio com título de atualização). */
  official: boolean;
  url: string;
  /** Epoch (s) em que a cópia instalada neste PC foi atualizada pela Steam. */
  localUpdated: number | null;
}
export interface SteamNewsOverview {
  connected: boolean;
  candidates: number;
  checked: number;
  running: boolean;
  done: number;
  total: number;
  error: string | null;
  checkedAt: number | null;
  items: SteamNewsItem[];
  seenGid: string | null;
}

/* ---------- Backup e restauração ---------- */
export type BackupAutoMode = "never" | "exit" | "weekly";
export interface BackupSummary {
  profiles: number; profileNames: string[]; games: number; libraryEntries: number;
  favorites: number; sessions: number; playtimeSeconds: number; collections: number; emulators: number;
  gallery: number; images: number; captures: number; achievements: number; accounts: number; modFavorites: number;
  settings: number; totalBytes: number;
}
export interface BackupInfo { secrets: number; capturesDir: string; busy: boolean }
export interface BackupTarget { status: "new" | "replace" | "conflict"; path: string }
export interface BackupReport { path: string; bytes: number; files: number; encrypted: boolean; secrets: "included" | "excluded" | "local"; secretsSkipped: number; missing: number }
export interface BackupInspect {
  path: string; appVersion: string; createdAt: string; kind: "manual" | "auto" | "safety"; formatVersion: number;
  encrypted: boolean; needsPassword: boolean; secrets: "included" | "excluded" | "local"; summary: BackupSummary;
  files: number; totalBytes: number; problem: string | null; warnings: string[];
}
export interface BackupVerify { ok: number; invalid: string[] }
export interface BackupRestoreReport {
  safetyPath: string; restoredFiles: number; skipped: string[]; secrets: string; secretsApplied: number; secretsFailed: number; warnings: string[];
}
export interface BackupAuto {
  mode: BackupAutoMode; dir: string | null; last: string | null; lastPath: string | null; lastError: string | null;
  keep: number; hasPassword: boolean; includeCaptures: boolean;
}
export interface BackupProgress { op: "create" | "verify" | "restore"; kind: string; phase: string; done: number; total: number; cancelable: boolean }
