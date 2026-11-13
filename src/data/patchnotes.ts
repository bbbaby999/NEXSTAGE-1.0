/**
 * Histórico de versões do NEXSTAGE. A cada nova versão, acrescente uma entrada NO TOPO e mantenha
 * a versão do package.json (única fonte) igual à primeira entrada.
 * Seções: new = novas funcionalidades · improve = melhorias e otimizações · fix = correções · ui = interface.
 */
import { tk } from "../lib/i18n";

export type NoteType = "new" | "improve" | "fix" | "ui";
export interface PatchNote {
  version: string;
  date: string; // AAAA-MM-DD (ou texto, quando não houver data exata)
  summary?: string;
  sections: { type: NoteType; items: string[] }[];
}

export const NOTE_LABEL: Record<NoteType, string> = {
  new: tk("Novas funcionalidades"), improve: tk("Melhorias e otimizações"), fix: tk("Correções de bugs"), ui: tk("Alterações na interface"),
};

export const PATCH_NOTES: PatchNote[] = [
  {
    version: "1.0.2",
    date: "2026-11-12",
    summary: "Nova central de Feedback e relatórios: envie sugestões e relate problemas direto do app.",
    sections: [
      { type: "new", items: [
        "Feedback e relatórios (Configurações → Atualizações): envie sugestões, ideias de novas funcionalidades, comentários ou relate problemas ao desenvolvedor, sem precisar de conta.",
        "Anexos opcionais de diagnóstico: você vê o conteúdo exato antes de enviar, e nome de usuário, e-mails, chaves e endereços de rede são removidos automaticamente.",
        "Registro local de erros, guardado só no seu computador e anexado a um relato apenas com a sua autorização.",
      ] },
    ],
  },
  {
    version: "1.0.1",
    date: "2026-11-12",
    summary: "Traduções completas em todos os idiomas e configurações de notificações mais organizadas.",
    sections: [
      { type: "improve", items: [
        "Traduções completas em inglês, espanhol, francês, alemão, italiano, russo, japonês, coreano e chinês: Central de Controles, Backup e restauração, notas de atualização da Steam e os demais textos que ainda apareciam em português.",
        "Datas, horário do relógio e números seguem o idioma escolhido.",
      ] },
      { type: "ui", items: [
        "A notificação de captura (e a duração dela) foi para Configurações → Sons e notificações, junto das demais notificações.",
        "Removidos os botões de teste das notificações de conquistas e de controles.",
      ] },
    ],
  },
  {
    version: "1.0.0",
    date: "2026-11-04",
    summary: "Versão oficial: novos temas, capas para os emuladores, mais atalhos de controle e um app mais leve e estável.",
    sections: [
      { type: "new", items: [
        "Fontes de dados (Configurações): IGDB com descrição, empresas, franquia, classificação, imagens, trailers e tempo para zerar; Nexus Mods com a nova aba Mods (pesquisa, filtros, requisitos, versões e favoritos — sem instalar nada).",
        "HowLongToBeat: a consulta automática foi removida porque contornava a proteção do site; os tempos já salvos continuam, e o IGDB passa a informar o tempo para zerar.",
        "Seu universo gamer: cards na Home com jogo favorito, horas, conquistas, último jogo e plataforma favorita — cada card abre as Estatísticas do perfil.",
        "Estatísticas consolidadas (NEXSTAGE + Steam + RetroAchievements) no perfil e na página Estatísticas: resumo, ranking, conquistas recentes e raras, jogos 100% e gráficos mensais.",
        "Visibilidade das estatísticas: privado, público ou personalizado, com prévia de como outros perfis veem.",
        "Steam: conecte sua conta pelo login oficial “Entrar com a Steam” em Configurações → Contas e plataformas (o NEXSTAGE nunca vê sua senha).",
        "Importação da Steam: escolha exatamente quais jogos entram, com filtros, pesquisa e prévia antes de confirmar.",
        "Jogos da Steam são instalados e iniciados pelo cliente oficial (botões Instalar e Jogar), preservando overlay, atualizações e anti-cheat.",
        "Tempo de jogo, conquistas e estatísticas públicas da Steam na página de cada jogo, como dados externos.",
        "RetroAchievements: conecte sua conta por perfil em Configurações (a chave da API fica cifrada pelo Windows e nunca é exibida).",
        "Nova página Conquistas: pontos, conquistas recentes, jogos com progresso, filtros (obtidas, pendentes, recentes, Hardcore), pesquisa e ordenação.",
        "Jogos de PS1, PS2, PSP, SNES e NES são identificados pelo hash oficial do arquivo; sugestões por nome só valem após sua confirmação.",
        "Progresso e conquistas na página de cada jogo compatível, com selo discreto “RA” na capa.",
        "Sincronização automática com cache offline e aviso de novos desbloqueios (quase em tempo real com PCSX2 e DuckStation, pelo log do emulador confirmado pela API).",
        "Emuladores com capa: cada emulador ganha uma arte com o símbolo e a cor do console, e você pode trocar a imagem em “Editar”.",
        "Novos temas em Configurações → Aparência: Meio-Dia e Black & Pink.",
        "Teclado virtual: o botão Start/Options do controle conclui a edição.",
      ] },
      { type: "ui", items: [
        "Selo da plataforma nas capas do carrossel “Volte a jogar” da Home e na página de cada jogo, ao lado dos gêneros.",
        "Na Home, passar o mouse ou o foco do controle sobre um jogo do carrossel mostra a arte dele no plano de fundo.",
        "Ao entrar na Biblioteca, o foco do controle vai direto para o primeiro jogo; ao voltar da página de um jogo, volta para ele.",
        "Temas renomeados e recoloridos: Azul Meia-Noite, Neon, Carmesim, Esmeralda, Verde Grama e Branco Gelo.",
      ] },
      { type: "improve", items: [
        "Capas e fundos agora ficam em arquivos, não mais dentro do banco de dados: a biblioteca carrega mais rápido e o banco fica bem menor. A conversão é automática na primeira abertura.",
        "Importar capturas, abrir a galeria, carregar a biblioteca e varrer pastas não travam mais a interface.",
        "Executável cerca de 10% menor e leitura de controles mais econômica quando nenhum controle está conectado.",
        "Camada extra de segurança (política de conteúdo) na interface.",
      ] },
      { type: "fix", items: [
        "Atualizações do banco de dados agora são aplicadas de forma atômica: uma queda de energia no meio não deixa o banco pela metade.",
        "Um erro inesperado em uma operação não deixa mais o banco indisponível até reiniciar o app.",
        "O download de imagens da internet não aceita mais redirecionamentos para sites fora da lista permitida.",
      ] },
    ],
  },
  {
    version: "0.1.7",
    date: "2026-11-02",
    sections: [
      { type: "new", items: [
        "Adicionar pasta de jogos: o NEXSTAGE varre a pasta e as subpastas (.iso, .bin/.cue, .exe, .nsp, .pkg e .vpk), mostra uma pré-visualização e só importa depois da sua confirmação.",
        "A plataforma é identificada pelo conteúdo dos arquivos (não só pela extensão): discos de PS1, PS2, PS3, PSP, Xbox e Xbox 360, pacotes NSP, PKG e VPK e executáveis do Windows.",
        "Página Emuladores: cadastre emuladores com plataformas, formatos e argumentos (com sugestões prontas). Os jogos importados são associados automaticamente ao emulador compatível, e você pode escolher quando há mais de um.",
        "Pastas de jogos: veja as pastas adicionadas, atualize a varredura para achar jogos novos ou remova uma pasta (só o vínculo; nenhum arquivo é apagado).",
        "Detecção de duplicidade por caminho normalizado: o que já está na biblioteca não é importado de novo.",
      ] },
      { type: "improve", items: [
        "Em “Editar jogo” você escolhe o emulador de jogos importados por ISO ou pasta. A associação antiga (executável + argumentos com {iso}) continua funcionando.",
        "Os argumentos de inicialização aceitam {rom} além de {iso}.",
      ] },
      { type: "ui", items: ["A página Emuladores deixou de ser um aviso “em breve”."] },
    ],
  },
  {
    version: "0.1.6",
    date: "2026-11-01",
    sections: [
      { type: "new", items: [
        "Botão do overlay no controle: escolha uma combinação de 2 a 3 botões (segurada por 0,7 s) para abrir o overlay, inclusive dentro do jogo.",
        "Captura de tela por atalho de teclado e por combinação do controle, ambos configuráveis. Funciona durante o jogo e salva em Imagens\\NEXSTAGE\\Capturas, com cópia na Galeria do perfil ativo.",
        "Editor de imagem para foto de perfil e banner: zoom, reposicionamento e prévia em tempo real.",
        "Biblioteca: ordenar por plataforma (agrupado), com filtro de plataforma.",
        "Patch Notes nas Configurações, com abertura automática após atualizar (opcional).",
      ] },
      { type: "improve", items: [
        "Clicar no nome ou no avatar do perfil ativo agora abre “Meu perfil”.",
        "A preferência de plataforma da Biblioteca é lembrada por perfil.",
      ] },
      { type: "ui", items: [
        "Novo botão “Trocar usuário” ao lado do perfil na barra superior.",
        "Novas opções em Configurações: captura de tela, combinações do controle e Patch Notes.",
      ] },
    ],
  },
  {
    version: "0.1.5",
    date: "2026-10-31",
    sections: [
      { type: "new", items: [
        "Importação de arquivos ISO na Biblioteca (o arquivo original não é copiado nem alterado).",
        "Inicialização com o Windows e modos de abertura: minimizado, janela ou tela cheia.",
        "Botão “Abrir pasta” na visualização das capturas.",
        "Link do Fandom na página do jogo, no lugar do OpenCritic.",
        "Identificação de versão “Beta”.",
      ] },
      { type: "fix", items: [
        "Um jogo que falhava ao iniciar já era marcado como “iniciado” e subia na Tela inicial.",
      ] },
    ],
  },
  {
    version: "0.1.0 – 0.1.4",
    date: "Versões anteriores à 0.1.5",
    summary: "Resumo geral: não há registro individual de cada versão.",
    sections: [
      { type: "new", items: [
        "Perfis, Biblioteca, coleções, galeria de capturas, estatísticas e página “Meu perfil”.",
        "Página do jogo com dados da Steam, tempo para zerar e gêneros; ordenação e filtro por gênero.",
        "Overlay sobre jogos, navegação por controle (Xbox/PlayStation), teclado virtual e sons.",
        "Temas, fundo animado com ondas e partículas cósmicas, animação de inicialização e atualização pelo app.",
      ] },
    ],
  },
];
