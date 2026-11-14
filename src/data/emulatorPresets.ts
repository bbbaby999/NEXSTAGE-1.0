import { tk } from "../lib/i18n";
/** Sugestões de cadastro. Os argumentos são pontos de partida: confira a documentação do seu emulador/versão. */
/** `fullscreen`: marca "Iniciar jogos em tela cheia" (o NEXSTAGE aplica no método de cada emulador). */
export interface EmulatorPreset { name: string; platforms: string[]; formats: string[]; args: string; fullscreen: boolean; note?: string }

export const EMULATOR_PRESETS: EmulatorPreset[] = [
  { name: "PCSX2", platforms: ["Playstation 2"], formats: ["iso", "bin", "cue"], args: '-batch -- "{rom}"', fullscreen: true },
  { name: "DuckStation", platforms: ["Playstation 1"], formats: ["iso", "bin", "cue"], args: '-batch -- "{rom}"', fullscreen: true },
  { name: "RPCS3", platforms: ["Playstation 3"], formats: ["iso", "pkg"], args: '--no-gui "{rom}"', fullscreen: true,
    note: tk("Arquivos .pkg precisam ser instalados no RPCS3 (menu de instalação); não são iniciados por caminho.") },
  { name: "PPSSPP", platforms: ["PSP"], formats: ["iso"], args: '"{rom}"', fullscreen: true },
  { name: "Vita3K", platforms: ["Ps Vita"], formats: ["vpk"], args: "", fullscreen: true,
    note: tk("Arquivos .vpk precisam ser instalados no Vita3K; depois inicie o jogo pelo próprio emulador.") },
  { name: tk("Emulador de Switch (Ryujinx ou compatível)"), platforms: ["Nintendo Switch"], formats: ["nsp", "xci"], args: '"{rom}"', fullscreen: true },
  // Opções do próprio Lichii (`-h`): -f = tela cheia (posto pela caixa, antes de tudo), -g = arquivo do jogo. O parser
  // para no primeiro argumento solto: com o caminho antes das opções ("{rom}" -g) ele não carrega o jogo.
  { name: "Lichii", platforms: ["Nintendo Switch 2"], formats: ["nsp", "xci", "nup"], args: '-g "{rom}"', fullscreen: true,
    note: tk("O Lichii lê as opções antes do jogo: -g indica o arquivo. Não coloque o caminho antes das opções.") },
  { name: "xemu", platforms: ["XBOX"], formats: ["iso"], args: '-dvd_path "{rom}"', fullscreen: true },
  { name: "Xenia", platforms: ["Xbox 360"], formats: ["iso"], args: '"{rom}"', fullscreen: true },
];
export const ALL_FORMATS = ["iso", "bin", "cue", "nsp", "xci", "nup", "pkg", "vpk"] as const;
