import { tk } from "../lib/i18n";
/** Sugestões de cadastro. Os argumentos são pontos de partida: confira a documentação do seu emulador/versão. */
export interface EmulatorPreset { name: string; platforms: string[]; formats: string[]; args: string; note?: string }

export const EMULATOR_PRESETS: EmulatorPreset[] = [
  { name: "PCSX2", platforms: ["Playstation 2"], formats: ["iso", "bin", "cue"], args: '-batch -fullscreen -- "{rom}"' },
  { name: "DuckStation", platforms: ["Playstation 1"], formats: ["iso", "bin", "cue"], args: '-batch -fullscreen -- "{rom}"' },
  { name: "RPCS3", platforms: ["Playstation 3"], formats: ["iso", "pkg"], args: '--no-gui "{rom}"',
    note: tk("Arquivos .pkg precisam ser instalados no RPCS3 (menu de instalação); não são iniciados por caminho.") },
  { name: "PPSSPP", platforms: ["PSP"], formats: ["iso"], args: '"{rom}"' },
  { name: "Vita3K", platforms: ["Ps Vita"], formats: ["vpk"], args: "",
    note: tk("Arquivos .vpk precisam ser instalados no Vita3K; depois inicie o jogo pelo próprio emulador.") },
  { name: "Emulador de Switch (Ryujinx ou compatível)", platforms: ["Nintendo Switch"], formats: ["nsp"], args: '"{rom}"' },
  { name: "Lichii", platforms: ["Nintendo Switch 2"], formats: ["nsp", "nup"], args: '"{rom}"',
    note: tk("Argumentos de ponto de partida: confira na documentação do Lichii como ele recebe o caminho do jogo.") },
  { name: "xemu", platforms: ["XBOX"], formats: ["iso"], args: '-dvd_path "{rom}"' },
  { name: "Xenia", platforms: ["Xbox 360"], formats: ["iso"], args: '"{rom}"' },
];
export const ALL_FORMATS = ["iso", "bin", "cue", "nsp", "nup", "pkg", "vpk"] as const;
