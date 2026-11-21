import { tk } from "../lib/i18n";
/** Sugestões de cadastro. Os argumentos são pontos de partida: confira a documentação do seu emulador/versão. */
/** `fullscreen`: marca "Iniciar jogos em tela cheia" (o NEXSTAGE aplica no método de cada emulador). */
/** `catalog`: item do catálogo do backend (emu_catalog.rs): o NEXSTAGE sabe baixar, instalar e localizar este emulador. */
export interface EmulatorPreset { name: string; platforms: string[]; formats: string[]; args: string; fullscreen: boolean; note?: string; catalog?: string }

export const EMULATOR_PRESETS: EmulatorPreset[] = [
  { name: "PCSX2", platforms: ["Playstation 2"], formats: ["iso", "bin", "cue", "chd"], args: '-batch -- "{rom}"', fullscreen: true, catalog: "pcsx2" },
  { name: "DuckStation", platforms: ["Playstation 1"], formats: ["iso", "bin", "cue", "chd"], args: '-batch -- "{rom}"', fullscreen: true, catalog: "duckstation" },
  { name: "RPCS3", platforms: ["Playstation 3"], formats: ["iso", "pkg"], args: '--no-gui "{rom}"', fullscreen: true, catalog: "rpcs3",
    note: tk("Arquivos .pkg precisam ser instalados no RPCS3 (menu de instalação); não são iniciados por caminho.") },
  { name: "PPSSPP", platforms: ["PSP"], formats: ["iso"], args: '"{rom}"', fullscreen: true, catalog: "ppsspp" },
  { name: "Vita3K", platforms: ["Ps Vita"], formats: ["vpk"], args: "", fullscreen: true, catalog: "vita3k",
    note: tk("Arquivos .vpk precisam ser instalados no Vita3K; depois inicie o jogo pelo próprio emulador.") },
  { name: "Eden", platforms: ["Nintendo Switch"], formats: ["nsp", "xci"], args: '"{rom}"', fullscreen: true, catalog: "eden",
    note: tk("O Eden precisa das chaves (prod.keys) e do firmware do seu próprio console para abrir jogos.") },
  { name: tk("Emulador de Switch (Ryujinx ou compatível)"), platforms: ["Nintendo Switch"], formats: ["nsp", "xci"], args: '"{rom}"', fullscreen: true },
  // Opções do próprio Lichii (`-h`): -f = tela cheia (posto pela caixa, antes de tudo), -g = arquivo do jogo. O parser
  // para no primeiro argumento solto: com o caminho antes das opções ("{rom}" -g) ele não carrega o jogo.
  { name: "Lichii", platforms: ["Nintendo Switch 2"], formats: ["nsp", "xci", "nup"], args: '-g "{rom}"', fullscreen: true, catalog: "lichii",
    note: tk("O Lichii lê as opções antes do jogo: -g indica o arquivo. Não coloque o caminho antes das opções.") },
  { name: "xemu", platforms: ["XBOX"], formats: ["iso"], args: '-dvd_path "{rom}"', fullscreen: true, catalog: "xemu",
    note: tk("O xemu precisa da BIOS, do MCPX e do HD do seu próprio Xbox (configure no emulador).") },
  { name: "Xenia", platforms: ["Xbox 360"], formats: ["iso"], args: '"{rom}"', fullscreen: true, catalog: "xenia" },
  // -b: fecha o Dolphin junto com o jogo; -e: inicia o arquivo
  { name: "Dolphin", platforms: ["GameCube", "Wii"], formats: ["iso", "gcm", "rvz", "wbfs", "wia", "gcz", "wad"], args: '-b -e "{rom}"', fullscreen: true, catalog: "dolphin" },
  { name: "Cemu", platforms: ["Wii U"], formats: ["wud", "wux", "wua", "rpx"], args: '-g "{rom}"', fullscreen: true, catalog: "cemu",
    note: tk("O Cemu precisa das chaves (keys.txt) do seu próprio Wii U para jogos criptografados.") },
  { name: "melonDS", platforms: ["Nintendo DS"], formats: ["nds"], args: '"{rom}"', fullscreen: true, catalog: "melonds",
    note: tk("O melonDS funciona sem BIOS; para mais compatibilidade, use a BIOS e o firmware do seu próprio DS.") },
  { name: "mGBA", platforms: ["Game Boy Advance", "Game Boy Color", "Game Boy"], formats: ["gba", "gbc", "gb"], args: '"{rom}"', fullscreen: true, catalog: "mgba" },
  { name: "Azahar", platforms: ["Nintendo 3DS"], formats: ["3ds", "cci", "cxi", "3dsx"], args: '"{rom}"', fullscreen: true, catalog: "azahar",
    note: tk("O Azahar abre jogos de 3DS descriptografados (feitos a partir do seu próprio console).") },
  { name: "Flycast", platforms: ["Dreamcast"], formats: ["gdi", "cdi", "chd"], args: '"{rom}"', fullscreen: true, catalog: "flycast" },
  { name: "RMG", platforms: ["Nintendo 64"], formats: ["z64", "n64", "v64"], args: '"{rom}"', fullscreen: true, catalog: "rmg" },
  { name: "ares", platforms: ["Nintendo 64", "Mega Drive"], formats: ["z64", "n64", "v64", "md", "gen", "smd", "bin"], args: '"{rom}"', fullscreen: true, catalog: "ares",
    note: tk("O ares roda vários consoles; aqui ele fica para Nintendo 64 e Mega Drive (adicione outras plataformas se quiser).") },
  { name: "Snes9x", platforms: ["Super Nintendo"], formats: ["sfc", "smc"], args: '"{rom}"', fullscreen: true, catalog: "snes9x" },
  { name: "Mesen", platforms: ["Nintendinho", "Super Nintendo"], formats: ["nes", "sfc", "smc"], args: '"{rom}"', fullscreen: true, catalog: "mesen",
    note: tk("O Mesen precisa do .NET 8 (Desktop Runtime) instalado no Windows.") },
];
/** Formatos de jogo aceitos (mesma lista do backend: emulators.rs FORMATS). */
export const ALL_FORMATS = [
  "iso", "bin", "cue", "chd", "nsp", "xci", "nup", "pkg", "vpk",
  "gcm", "rvz", "wbfs", "wia", "gcz", "wad", "wud", "wux", "wua", "rpx",
  "nds", "3ds", "cci", "cxi", "3dsx", "gba", "gb", "gbc", "z64", "n64", "v64", "md", "gen", "smd",
  "gdi", "cdi", "sfc", "smc", "nes",
] as const;
