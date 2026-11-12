# NEXSTAGE

**ALL GAMES. ONE STAGE.** — launcher de jogos para Windows 10/11, pensado para ser usado tanto com mouse e teclado quanto com controle, do sofá.

Feito com Tauri 2, React, TypeScript, Rust e SQLite.

## Recursos

- **Biblioteca única**: jogos de PC (executável), jogos da Steam e jogos de emulador (ROM/ISO), com capas, fundos, favoritos, coleções, notas e tempo de jogo.
- **Perfis**: cada pessoa da casa tem a própria biblioteca, estatísticas, tema e contas.
- **Steam**: importa a biblioteca, conquistas e notas de atualização (chave da Web API do próprio usuário).
- **RetroAchievements**: conquistas dos jogos de emulador, com aviso de desbloqueio por cima do jogo.
- **Emuladores**: cadastro de emuladores e importação de ROMs/ISOs por pasta.
- **Central de Controles**: Xbox, DualSense, DualShock 4, Pro Controller, Joy-Con (inclusive juntos como um só) e controles genéricos, com identificação única por aparelho e ajustes individuais (LED, vibração, zona morta, mapeamento e padrão de confirmação).
- **Overlay e capturas**: abre o NEXSTAGE por cima do jogo e tira capturas de tela por atalho de teclado ou de controle, com galeria por perfil.
- **Fontes de dados opcionais**: IGDB (informações e imagens), Nexus Mods (mods), SteamGridDB e RAWG (pesquisa de imagens).
- **Backup e restauração**, com proteção por senha opcional.
- **Modo ocioso**, notificações, 10 idiomas e atualização automática.

## Instalação (usuário)

Baixe o instalador `NEXSTAGE_x.y.z_x64-setup.exe` na página de [Releases](../../releases) e execute. O WebView2 já vem no Windows 11 e no Windows 10 atualizado.

Os dados ficam em `%APPDATA%\app.nexstage.launcher\` (banco `nexstage.db`, imagens e galeria). Desinstalar o app não apaga essa pasta.

## Desenvolvimento

### Requisitos
1. [Node.js 18+](https://nodejs.org)
2. [Rust (rustup)](https://rustup.rs) com o toolchain MSVC
3. Visual Studio Build Tools com "Desenvolvimento para desktop com C++"

### Comandos
```bash
npm install
npm run tauri dev      # app em modo de desenvolvimento
npm run check          # verificação de tipos (TypeScript) e das traduções
npm test               # testes do frontend (scripts/test-*.mjs)
npm run test:rust      # testes do backend (Rust)
npm run tauri build    # instalador NSIS em src-tauri/target/release/bundle/nsis
```

### Traduções
O português é o idioma de origem: os textos da interface ficam no código dentro de `t("...")` / `tk("...")`.
Depois de adicionar ou mudar textos:
```bash
node scripts/i18n/build-keys.cjs    # atualiza src/locales/keys.ts
node scripts/i18n/check-locales.cjs # confere se todos os idiomas têm o mesmo número de entradas
```
Os arquivos de idioma são listas na mesma ordem de `keys.ts`; entradas novas vão no fim, e `""` mostra o texto em português.

## Publicar uma versão

1. Gere a chave de assinatura das atualizações **fora do projeto** (uma vez só):
   ```bash
   npx tauri signer generate -w "%USERPROFILE%\.tauri\nexstage.key"
   ```
2. Configure o repositório de releases (uma vez só): `node scripts/setup-updater.mjs USUARIO/REPOSITORIO`
3. Atualize a versão **só** no `package.json` (o `tauri.conf.json`, o Rust e a interface leem dela) e adicione a entrada em `src/data/patchnotes.ts`.
4. Compile com a chave definida nas variáveis de ambiente `TAURI_SIGNING_PRIVATE_KEY` (conteúdo ou caminho do arquivo) e `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, e rode `npm run tauri build`.
5. `node scripts/release.mjs "O que mudou"` gera a pasta `release\` com o instalador, a assinatura e o `latest.json` para anexar ao Release `vX.Y.Z` no GitHub.

> **Nunca** coloque a chave privada (`nexstage.key`), senhas ou chaves de API no repositório. O `.gitignore` já bloqueia os formatos mais comuns.

## Segurança e privacidade

- As credenciais (Steam, RetroAchievements, IGDB, Nexus Mods, SteamGridDB, RAWG) ficam cifradas pela DPAPI do Windows e só abrem no mesmo usuário do mesmo computador. As de Steam, RetroAchievements, IGDB e Nexus Mods nunca voltam para a interface.
- O NEXSTAGE não tem servidor próprio: ele conversa só com as APIs que o usuário configurou.
- O app não altera os controles, drivers nem o Windows. Os ajustes da Central de Controles valem apenas dentro do NEXSTAGE.

## Estrutura

```
src/                  Frontend (React + TypeScript)
  features/           telas (Home, Biblioteca, Galeria, Configurações, Central de Controles...)
  components/         componentes reutilizáveis
  lib/                api.ts (único ponto de contato com o Rust), navegação por foco, controles, i18n
  locales/            traduções
src-tauri/src/        Backend (Rust)
  commands/           comandos chamados pela interface
  steam/ retro/       integrações Steam e RetroAchievements
  sources/            fontes de dados (IGDB, Nexus Mods)
  backup/             backup e restauração
  input.rs hidpads.rs devident.rs   leitura e identificação dos controles
scripts/              publicação, traduções e testes
```

## Licença

A definir.
