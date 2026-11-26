/**
 * Histórico de versões do NEXSTAGE. A cada nova versão, acrescente uma entrada NO TOPO e mantenha
 * a versão do package.json (única fonte) igual à primeira entrada.
 * Seções: new = novas funcionalidades · improve = melhorias e otimizações · fix = correções · ui = interface.
 * Todo texto vai dentro de tk("…"): vira chave de tradução (scripts/i18n/build-keys.cjs) e precisa ser traduzido
 * nos 9 idiomas (src/locales), como o resto da interface.
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
    version: "1.3.4",
    date: "2026-11-26",
    summary: tk("Pronto para o Tomb Survivors beta: nova capa, nova arte e nova descrição na página do jogo."),
    sections: [
      { type: "ui", items: [tk("Tomb Survivors: nova capa, arte de fundo e descrição atualizada (novo mapa, personagens e chefes).")] },
      { type: "improve", items: [tk("Recebe a atualização 0.5.0-beta do Tomb Survivors.")] },
    ],
  },
  {
    version: "1.3.3",
    date: "2026-11-22",
    summary: tk("Melhorias visuais e correções de bugs."),
    sections: [
      { type: "ui", items: [tk("Melhorias visuais.")] },
      { type: "fix", items: [tk("Correções de bugs.")] },
    ],
  },
  {
    version: "1.3.2",
    date: "2026-11-21",
    summary: tk("Emuladores prontos para baixar e atualizar pelo app, 11 novas plataformas (GameCube, Wii, Nintendo 64, Mega Drive e mais) e o primeiro jogo integrado ao NexStage."),
    sections: [
      { type: "new", items: [
        tk("Jogos integrados ao NexStage: o Tomb Survivors aparece na biblioteca, instala e atualiza pelo app e tem página própria com conquistas e estatísticas."),
        tk("Emuladores: cartões prontos de 19 emuladores para baixar e instalar pelo app (você escolhe a pasta) ou localizar uma instalação que já existe. “Procurar no PC” encontra os que você já tem."),
        tk("Atualização dos emuladores: aviso de versão nova e botão Atualizar em cada emulador, com a opção de atualizar automaticamente (com o emulador fechado). Configurações, BIOS e saves são mantidos."),
        tk("Novas plataformas: GameCube, Wii, Wii U, Nintendo DS, Nintendo 3DS, Game Boy, Game Boy Color, Game Boy Advance, Nintendo 64, Dreamcast e Mega Drive, com os formatos de arquivo de cada uma."),
        tk("Novos emuladores sugeridos: Dolphin, Cemu, melonDS, mGBA, Azahar, Flycast, Snes9x, Mesen, RMG e ares. Lichii e Eden também podem ser baixados pelo app."),
        tk("Editar emulador: abra a pasta de instalação no navegador de arquivos do app (ou no Explorador do Windows) e mova a pasta para outro lugar."),
      ] },
      { type: "improve", items: [
        tk("Importação: reconhece discos de GameCube e Wii (inclusive .rvz, .wia e .gcz), as faixas citadas em arquivos .gdi e cartuchos de Mega Drive salvos como .bin."),
        tk("RetroAchievements: vínculo com jogos de Game Boy, Game Boy Color, Game Boy Advance, Nintendo 64 e Mega Drive."),
        tk("“Iniciar jogos em tela cheia” funciona também nos novos emuladores."),
        tk("Tela de emuladores: filtro Instalados / Não instalados."),
        tk("Ao instalar ou mover um emulador para um lugar onde já existe uma pasta com o mesmo nome, o app pergunta se quer substituí-la (a pasta antiga vai para a Lixeira)."),
      ] },
      { type: "fix", items: [
        tk("Navegador de arquivos: com o controle, △ leva aos botões Cancelar e Selecionar esta pasta, que antes eram difíceis de alcançar; segurar ○ cancela de uma vez."),
        tk("Navegador de arquivos: as pastas favoritas apareciam empilhadas no canto superior direito na página “Este computador”."),
      ] },
    ],
  },
  {
    version: "1.3.1",
    date: "2026-11-18",
    summary: tk("PC Game Pass na Home com página própria para cada jogo e trailers, DLCs e notas de atualização dos jogos da Steam, jogos do Xbox não instalados e controles de vídeo do próprio app."),
    sections: [
      { type: "new", items: [
        tk("PC Game Pass na Home (experimental): carrossel com os jogos que estão chegando, os adicionados recentemente e os que vão sair, com a data de saída prevista no catálogo da Microsoft. Pode ser desligado em Configurações → Contas e plataformas → Xbox."),
        tk("Página de cada jogo do Game Pass: descrição, capturas de tela, informações, recursos no PC e classificação indicativa. Adicione o jogo à biblioteca (instalado ou não), jogue quando estiver instalado ou abra-o no app Xbox."),
        tk("Trailers dos jogos do Game Pass dentro do app, com qualidade automática. Sem trailer do Xbox, os do YouTube aparecem pelo IGDB (se ele estiver ativado em Fontes de dados)."),
        tk("Jogos da Steam: aba DLCs na página do jogo, com preço, data, DLCs novas, em breve e instaladas neste PC."),
        tk("Home → Novas DLCs: as DLCs lançadas recentemente para os jogos da Steam da sua biblioteca."),
        tk("Jogos da Steam: bloco “Última atualização” na página do jogo, com a nota de atualização oficial mais recente."),
        tk("Importar → Xbox: os jogos de PC que você já jogou e não estão instalados, com atalho para instalar pela Microsoft Store."),
      ] },
      { type: "improve", items: [
        tk("Vídeos (trailers do Game Pass e vídeos do IGDB): barra de controles do app, com reproduzir/pausar, avançar e voltar, linha do tempo, som e tela cheia."),
        tk("Tela cheia de vídeo: ✕ pausa, ← volta 5 s, → avança 5 s, □ liga/desliga o som e ○ fecha. A dica △ Tela cheia aparece com um vídeo em foco."),
        tk("Visualizador de capturas: ações no rodapé e imagem maior; ↑/↓ trocam de captura e ←/→ escolhem a ação."),
      ] },
      { type: "fix", items: [
        tk("Notas de atualização da Steam: códigos como [p align=\"start\"] não aparecem mais no resumo, e os links no formato novo voltaram a abrir."),
      ] },
      { type: "ui", items: [
        tk("Carrossel do Game Pass com o mesmo efeito 3D das capas da Biblioteca."),
      ] },
    ],
  },
  {
    version: "1.3.0",
    date: "2026-11-17",
    summary: tk("Navegador de arquivos próprio com controle, monitor preferido, ações em lote e imagens automáticas no Zerei!, Modo Ocioso com os jogos zerados e uma navegação pelo controle mais previsível."),
    sections: [
      { type: "new", items: [
        tk("Navegador de arquivos do NEXSTAGE: escolher jogos, imagens, pastas e arquivos agora funciona com o controle, com acesso rápido, unidades, pastas recentes, miniaturas das imagens e favoritos (▢/X favorita a pasta). Cada tela lembra a última pasta aberta, e atalhos de pasta da Área de trabalho funcionam. O seletor do Windows continua disponível em Configurações → Controles e atalhos."),
        tk("Monitor preferido (Configurações → Janela e inicialização): com mais de um monitor, o NEXSTAGE abre sempre no escolhido; se ele estiver desligado, abre no monitor ligado e volta para o preferido quando ele for religado."),
        tk("Zerei! → Atualizar imagens: procura capa, logo e plano de fundo automaticamente (primeiro resultado), com aviso das fontes configuradas e barra de progresso. Por padrão não altera as imagens que você já escolheu; marque “Substituir imagens existentes” para trocá-las."),
        tk("Zerei! → seleção de vários jogos: excluir, mover para outra lista (zerado, jogando, pausado, lista de desejos, não iniciado) e atualizar imagens de uma vez. Segure Ⓐ ou o botão do mouse sobre um jogo para começar a seleção com ele marcado; com itens selecionados, ☰ abre as ações e ▢ abre “Mover para…”."),
        tk("Galeria: com capturas selecionadas, ☰ abre as ações da seleção."),
        tk("Modo Ocioso → Zerados: apresentação com os planos de fundo dos jogos zerados no Zerei!, com selos de conclusão, data, tempo de jogo e plataforma."),
        tk("“Abrir pasta de arquivos” em Mais opções do jogo: abre a pasta onde o jogo está (atalhos levam ao arquivo original; jogos da Steam, à pasta de instalação)."),
        tk("Start salva: nos formulários de jogo, no registro do Zerei!, na escolha de capa, fundo e logo e no editor de enquadramento. Sair com alterações pergunta “Sair sem salvar” ou “Salvar”."),
      ] },
      { type: "improve", items: [
        tk("Navegação pelo controle: o foco sempre vai para o item mais próximo na direção apertada."),
        tk("Overlay em jogo: toma o foco do jogo de forma mais confiável, para o controle passar a navegar no overlay."),
        tk("Biblioteca e Zerei!: rolagem mais fluida ao segurar o direcional (as capas carregam em segundo plano)."),
        tk("Modo Ocioso: as imagens sempre preenchem a tela com um leve zoom, e as informações ficam mais legíveis sobre qualquer imagem."),
      ] },
      { type: "fix", items: [
        tk("Zerei!: capas, logos e fundos que não apareciam (arquivos apagados por uma versão antiga) agora usam uma alternativa em vez de ficar em branco."),
        tk("Central de Controles: a coleção nunca é gravada por cima antes de ter sido carregada."),
      ] },
      { type: "ui", items: [
        tk("Botões com vidro neutro; a cor do tema aparece no botão em foco ou com o mouse por cima, e Remover/Excluir ficam vermelhos. O foco perdeu a borda: agora é um brilho por trás com um leve aumento."),
        tk("Página do jogo no Zerei!: arte de fundo sem película escura, botões legíveis e créditos das fontes só como texto."),
        tk("Pesquisa de imagens: borda clara na miniatura e no jogo em foco."),
      ] },
    ],
  },
  {
    version: "1.2.0",
    date: "2026-11-14",
    summary: tk("Integração com o Xbox e a Microsoft Store, a nova página Zerei! para registrar os jogos que você zerou, logos dos jogos e uma página do jogo redesenhada."),
    sections: [
      { type: "new", items: [
        tk("Jogos do Xbox / Microsoft Store (Game Pass): importe de uma vez os jogos do app Xbox instalados neste PC (Biblioteca → Importar → Xbox) ou adicione o atalho criado pelo app Xbox. Eles abrem pelo Windows e o NEXSTAGE registra o tempo de jogo. Não precisa de conta."),
        tk("Conta Xbox (Configurações → Contas e plataformas): gamerscore, histórico de jogos, tempo de jogo e conquistas do Xbox na página de cada jogo, com sincronização automática opcional. O login acontece no site oficial da Microsoft: o NEXSTAGE nunca vê sua senha, a sessão fica cifrada pelo Windows e o acesso é somente leitura."),
        tk("Aviso de conquista do Xbox durante o jogo: com um jogo do Xbox aberto, as conquistas novas aparecem no cartão por cima do jogo, com som."),
        tk("Zerei!: uma página para acompanhar os jogos que você zerou, está jogando, pausou ou quer jogar, mesmo que não estejam na biblioteca. Pesquise pelo nome e registre datas de início e de conclusão, horas, tipo de conclusão (zerado ou platinado), nota, dificuldade e comentários. Zerou de novo? Cada nova conclusão ganha o próprio registro."),
        tk("Estatísticas do Zerei!: jogos zerados, média das avaliações, zerados por plataforma, conclusões por ano e histórico, com filtros por nome, ano e nota."),
        tk("Importação e exportação do Zerei! em CSV, sem duplicar registros já existentes."),
        tk("“Marcar como zerado” na página do jogo inclui o jogo no Zerei!."),
        tk("Logos dos jogos: escolha a logo de cada jogo no SteamGridDB (em “Editar jogo”). Ela substitui o título escrito na Home, na página do jogo e na tela de descanso."),
        tk("Logos automáticas (opcional): jogos sem logo recebem a primeira logo encontrada no SteamGridDB, inclusive os adicionados depois. A sua escolha nunca é substituída."),
        tk("Trazer o NEXSTAGE para frente pelo controle: segure Select + LB (configurável em Configurações → Controles e atalhos) por 0,7 s em qualquer janela, inclusive dentro do jogo, e o NEXSTAGE volta em tela cheia. No teclado, Alt+Enter alterna entre tela cheia e janela."),
      ] },
      { type: "improve", items: [
        tk("Analógico direito: na vertical rola a página; na horizontal rola os carrosséis."),
        tk("Editor de enquadramento de imagens funciona com o controle: o direcional move a imagem, L1/L2 e R1/R2 ajustam o zoom."),
        tk("Pesquisa de imagens em duas etapas: escolha o jogo e depois as capas, logos e artes. Foto de perfil, banner e fundo da Home também podem ser pesquisados na internet."),
        tk("Metacritic: resultados mais precisos e opção de trocar o nome e pesquisar de novo."),
        tk("Fundos muito largos (como as artes da Steam) não são mais ampliados e cortados: aparecem na largura total, completados por uma cópia desfocada."),
      ] },
      { type: "ui", items: [
        tk("Nova página do jogo: cabeçalho com a arte de fundo e a logo, selos de plataforma e gêneros no canto, botão Jogar em destaque e as opções (Zerado, Capturas, Coleções, Editar, Remover) reunidas em “Mais opções”, também pelo botão Menu/Options do controle."),
        tk("Home: o jogo mais recente fica em destaque, seguido dos carrosséis Favoritos e “Volte a jogar”."),
        tk("Remover um jogo da biblioteca pede confirmação."),
      ] },
    ],
  },
  {
    version: "1.1.0",
    date: "2026-11-13",
    summary: tk("Troféus de PS3 e PS Vita, pontuação e níveis no perfil, um novo visual para a página de perfil e suporte a arquivos XCI. O NEXSTAGE deixa a fase beta."),
    sections: [
      { type: "new", items: [
        tk("Troféus de PlayStation 3 (RPCS3) e PS Vita (Vita3K): o NEXSTAGE lê os troféus gravados pelos emuladores e os exibe na página de cada jogo e na nova aba Troféus, em Conquistas, com tipo (platina, ouro, prata e bronze), descrição, data e progresso. Troféus ocultos permanecem ocultos até serem obtidos."),
        tk("Aviso de troféu desbloqueado durante o jogo, com o mesmo cartão das conquistas da Steam e do RetroAchievements. A leitura é local: nenhuma conta é necessária e nada é enviado à internet."),
        tk("Pontuação e níveis no perfil: cada conquista vale pontos, na escala dos troféus do PlayStation (bronze 15, prata 30, ouro 90, platina 180), somados de todas as fontes. Na Steam, o valor considera a raridade; no RetroAchievements, os pontos da conquista e o modo Hardcore; jogos 100% concluídos rendem um bônus. Os pontos sobem o nível do perfil e aparecem em cada conquista e nos avisos de desbloqueio."),
        tk("Suporte a arquivos XCI (cartuchos do Nintendo Switch): reconhecidos na importação por pasta e em “Adicionar jogo”, com verificação do cabeçalho do arquivo."),
      ] },
      { type: "improve", items: [
        tk("Tela cheia nos emuladores: a nova opção “Iniciar jogos em tela cheia”, em Emuladores → Editar, vale para todos os jogos associados. O NEXSTAGE aplica o método correto de cada emulador (PCSX2, DuckStation, RPCS3, PPSSPP, Vita3K, Ryujinx, yuzu e derivados, Lichii, xemu e Xenia), inclusive nos que não aceitam argumento de tela cheia, como o RPCS3. Argumentos de tela cheia já cadastrados foram convertidos automaticamente na nova opção."),
        tk("Qualquer jogo pode ser associado a um emulador em “Editar jogo”, não apenas ISOs e ROMs importados."),
        tk("Backup e restauração incluem as novas configurações: tela cheia dos emuladores, vínculos de troféus e a personalização do perfil."),
      ] },
      { type: "ui", items: [
        tk("Nova página de perfil: cabeçalho maior com a arte do perfil, moldura do avatar e anel de nível na cor da sua faixa, vitrine de favoritos com destaque para o primeiro, atividade recente com a arte de cada jogo e uma coluna lateral com pontuação, números do perfil, conquistas recentes e as mais raras."),
        tk("A aba Conquistas ganhou a opção Troféus, ao lado de RetroAchievements e Steam (no controle, L2 e R2 alternam entre as três)."),
        tk("O perfil ficou mais objetivo: saíram a lista de jogos recentes das estatísticas (já coberta pela atividade recente), a configuração de visibilidade das estatísticas e a nota sobre tempo de jogo."),
        tk("O selo “Beta” foi removido: esta é a primeira versão estável do NEXSTAGE."),
      ] },
    ],
  },
  {
    version: "1.0.2",
    date: "2026-11-12",
    summary: tk("Nova central de Feedback e relatórios: envie sugestões e relate problemas direto do app."),
    sections: [
      { type: "new", items: [
        tk("Feedback e relatórios (Configurações → Atualizações): envie sugestões, ideias de novas funcionalidades, comentários ou relate problemas ao desenvolvedor, sem precisar de conta."),
        tk("Anexos opcionais de diagnóstico: você vê o conteúdo exato antes de enviar, e nome de usuário, e-mails, chaves e endereços de rede são removidos automaticamente."),
        tk("Registro local de erros, guardado só no seu computador e anexado a um relato apenas com a sua autorização."),
      ] },
    ],
  },
  {
    version: "1.0.1",
    date: "2026-11-12",
    summary: tk("Traduções completas em todos os idiomas e configurações de notificações mais organizadas."),
    sections: [
      { type: "improve", items: [
        tk("Traduções completas em inglês, espanhol, francês, alemão, italiano, russo, japonês, coreano e chinês: Central de Controles, Backup e restauração, notas de atualização da Steam e os demais textos que ainda apareciam em português."),
        tk("Datas, horário do relógio e números seguem o idioma escolhido."),
      ] },
      { type: "ui", items: [
        tk("A notificação de captura (e a duração dela) foi para Configurações → Sons e notificações, junto das demais notificações."),
        tk("Removidos os botões de teste das notificações de conquistas e de controles."),
      ] },
    ],
  },
  {
    version: "1.0.0",
    date: "2026-11-04",
    summary: tk("Versão oficial: novos temas, capas para os emuladores, mais atalhos de controle e um app mais leve e estável."),
    sections: [
      { type: "new", items: [
        tk("Fontes de dados (Configurações): IGDB com descrição, empresas, franquia, classificação, imagens, trailers e tempo para zerar; Nexus Mods com a nova aba Mods (pesquisa, filtros, requisitos, versões e favoritos — sem instalar nada)."),
        tk("HowLongToBeat: a consulta automática foi removida porque contornava a proteção do site; os tempos já salvos continuam, e o IGDB passa a informar o tempo para zerar."),
        tk("Seu universo gamer: cards na Home com jogo favorito, horas, conquistas, último jogo e plataforma favorita — cada card abre as Estatísticas do perfil."),
        tk("Estatísticas consolidadas (NEXSTAGE + Steam + RetroAchievements) no perfil e na página Estatísticas: resumo, ranking, conquistas recentes e raras, jogos 100% e gráficos mensais."),
        tk("Visibilidade das estatísticas: privado, público ou personalizado, com prévia de como outros perfis veem."),
        tk("Steam: conecte sua conta pelo login oficial “Entrar com a Steam” em Configurações → Contas e plataformas (o NEXSTAGE nunca vê sua senha)."),
        tk("Importação da Steam: escolha exatamente quais jogos entram, com filtros, pesquisa e prévia antes de confirmar."),
        tk("Jogos da Steam são instalados e iniciados pelo cliente oficial (botões Instalar e Jogar), preservando overlay, atualizações e anti-cheat."),
        tk("Tempo de jogo, conquistas e estatísticas públicas da Steam na página de cada jogo, como dados externos."),
        tk("RetroAchievements: conecte sua conta por perfil em Configurações (a chave da API fica cifrada pelo Windows e nunca é exibida)."),
        tk("Nova página Conquistas: pontos, conquistas recentes, jogos com progresso, filtros (obtidas, pendentes, recentes, Hardcore), pesquisa e ordenação."),
        tk("Jogos de PS1, PS2, PSP, SNES e NES são identificados pelo hash oficial do arquivo; sugestões por nome só valem após sua confirmação."),
        tk("Progresso e conquistas na página de cada jogo compatível, com selo discreto “RA” na capa."),
        tk("Sincronização automática com cache offline e aviso de novos desbloqueios (quase em tempo real com PCSX2 e DuckStation, pelo log do emulador confirmado pela API)."),
        tk("Emuladores com capa: cada emulador ganha uma arte com o símbolo e a cor do console, e você pode trocar a imagem em “Editar”."),
        tk("Novos temas em Configurações → Aparência: Meio-Dia e Black & Pink."),
        tk("Teclado virtual: o botão Start/Options do controle conclui a edição."),
      ] },
      { type: "ui", items: [
        tk("Selo da plataforma nas capas do carrossel “Volte a jogar” da Home e na página de cada jogo, ao lado dos gêneros."),
        tk("Na Home, passar o mouse ou o foco do controle sobre um jogo do carrossel mostra a arte dele no plano de fundo."),
        tk("Ao entrar na Biblioteca, o foco do controle vai direto para o primeiro jogo; ao voltar da página de um jogo, volta para ele."),
        tk("Temas renomeados e recoloridos: Azul Meia-Noite, Neon, Carmesim, Esmeralda, Verde Grama e Branco Gelo."),
      ] },
      { type: "improve", items: [
        tk("Capas e fundos agora ficam em arquivos, não mais dentro do banco de dados: a biblioteca carrega mais rápido e o banco fica bem menor. A conversão é automática na primeira abertura."),
        tk("Importar capturas, abrir a galeria, carregar a biblioteca e varrer pastas não travam mais a interface."),
        tk("Executável cerca de 10% menor e leitura de controles mais econômica quando nenhum controle está conectado."),
        tk("Camada extra de segurança (política de conteúdo) na interface."),
      ] },
      { type: "fix", items: [
        tk("Atualizações do banco de dados agora são aplicadas de forma atômica: uma queda de energia no meio não deixa o banco pela metade."),
        tk("Um erro inesperado em uma operação não deixa mais o banco indisponível até reiniciar o app."),
        tk("O download de imagens da internet não aceita mais redirecionamentos para sites fora da lista permitida."),
      ] },
    ],
  },
  {
    version: "0.1.7",
    date: "2026-11-02",
    sections: [
      { type: "new", items: [
        tk("Adicionar pasta de jogos: o NEXSTAGE varre a pasta e as subpastas (.iso, .bin/.cue, .exe, .nsp, .pkg e .vpk), mostra uma pré-visualização e só importa depois da sua confirmação."),
        tk("A plataforma é identificada pelo conteúdo dos arquivos (não só pela extensão): discos de PS1, PS2, PS3, PSP, Xbox e Xbox 360, pacotes NSP, PKG e VPK e executáveis do Windows."),
        tk("Página Emuladores: cadastre emuladores com plataformas, formatos e argumentos (com sugestões prontas). Os jogos importados são associados automaticamente ao emulador compatível, e você pode escolher quando há mais de um."),
        tk("Pastas de jogos: veja as pastas adicionadas, atualize a varredura para achar jogos novos ou remova uma pasta (só o vínculo; nenhum arquivo é apagado)."),
        tk("Detecção de duplicidade por caminho normalizado: o que já está na biblioteca não é importado de novo."),
      ] },
      { type: "improve", items: [
        tk("Em “Editar jogo” você escolhe o emulador de jogos importados por ISO ou pasta. A associação antiga (executável + argumentos com {iso}) continua funcionando."),
        tk("Os argumentos de inicialização aceitam {rom} além de {iso}."),
      ] },
      { type: "ui", items: ["A página Emuladores deixou de ser um aviso “em breve”."] },
    ],
  },
  {
    version: "0.1.6",
    date: "2026-11-01",
    sections: [
      { type: "new", items: [
        tk("Botão do overlay no controle: escolha uma combinação de 2 a 3 botões (segurada por 0,7 s) para abrir o overlay, inclusive dentro do jogo."),
        tk("Captura de tela por atalho de teclado e por combinação do controle, ambos configuráveis. Funciona durante o jogo e salva em Imagens\\NEXSTAGE\\Capturas, com cópia na Galeria do perfil ativo."),
        tk("Editor de imagem para foto de perfil e banner: zoom, reposicionamento e prévia em tempo real."),
        tk("Biblioteca: ordenar por plataforma (agrupado), com filtro de plataforma."),
        tk("Patch Notes nas Configurações, com abertura automática após atualizar (opcional)."),
      ] },
      { type: "improve", items: [
        tk("Clicar no nome ou no avatar do perfil ativo agora abre “Meu perfil”."),
        tk("A preferência de plataforma da Biblioteca é lembrada por perfil."),
      ] },
      { type: "ui", items: [
        tk("Novo botão “Trocar usuário” ao lado do perfil na barra superior."),
        tk("Novas opções em Configurações: captura de tela, combinações do controle e Patch Notes."),
      ] },
    ],
  },
  {
    version: "0.1.5",
    date: "2026-10-31",
    sections: [
      { type: "new", items: [
        tk("Importação de arquivos ISO na Biblioteca (o arquivo original não é copiado nem alterado)."),
        tk("Inicialização com o Windows e modos de abertura: minimizado, janela ou tela cheia."),
        tk("Botão “Abrir pasta” na visualização das capturas."),
        tk("Link do Fandom na página do jogo, no lugar do OpenCritic."),
        tk("Identificação de versão “Beta”."),
      ] },
      { type: "fix", items: [
        tk("Um jogo que falhava ao iniciar já era marcado como “iniciado” e subia na Tela inicial."),
      ] },
    ],
  },
  {
    version: "0.1.0 – 0.1.4",
    date: "Versões anteriores à 0.1.5",
    summary: tk("Resumo geral: não há registro individual de cada versão."),
    sections: [
      { type: "new", items: [
        tk("Perfis, Biblioteca, coleções, galeria de capturas, estatísticas e página “Meu perfil”."),
        tk("Página do jogo com dados da Steam, tempo para zerar e gêneros; ordenação e filtro por gênero."),
        tk("Overlay sobre jogos, navegação por controle (Xbox/PlayStation), teclado virtual e sons."),
        tk("Temas, fundo animado com ondas e partículas cósmicas, animação de inicialização e atualização pelo app."),
      ] },
    ],
  },
];
