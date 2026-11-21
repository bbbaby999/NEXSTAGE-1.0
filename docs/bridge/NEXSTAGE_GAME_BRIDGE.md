# NexStage Game Bridge — Especificação técnica (v1)

Status: **Fases 1 a 10 concluídas** — integração completa (comunicação local; cadastro na biblioteca; instalação e atualização; conquistas; avisos; estatísticas e perfis; página especial; testes e segurança). Ver §19 — Estado da implementação.
Primeiro jogo integrado: **Tomb Survivors** (`game_id = "tomb-survivors"`).

---

## 1. Objetivo e princípios

O Game Bridge é uma camada **opcional** de comunicação entre o NexStage e jogos feitos para o ecossistema.

1. **Independência total.** O jogo abre, salva, registra conquistas e funciona offline sem o NexStage.
   O NexStage funciona sem nenhum jogo integrado instalado.
2. **Genérico.** O Bridge não contém regras de nenhum jogo. Tudo que é específico (conquistas,
   estatísticas aceitas, versões, arquivos) vem do **manifesto assinado** de cada jogo.
3. **Seguro por padrão.** Nenhum evento é aceito sem sessão autenticada, formato válido e catálogo
   correspondente. Nenhum manifesto executa comandos.
4. **Sem perda de dados.** Todo evento fica na caixa de saída do jogo até ser confirmado pelo NexStage.
5. **Reaproveitamento.** Tempo de jogo, notificações, downloads, hashes e backup usam o que já existe
   no NexStage (ver §12).

---

## 2. Transporte

| Item | Definição |
|---|---|
| Tecnologia | TCP **somente em loopback** (`127.0.0.1`), porta efêmera escolhida pelo sistema a cada sessão |
| Por que não Named Pipes | o Godot não tem cliente de pipe sem extensão nativa (GDExtension compilada); TCP local é nativo no Godot (`StreamPeerTCP`) e no Rust (`tokio::net`). O protocolo é independente do transporte: um transporte por pipe pode ser adicionado depois sem mudar as mensagens |
| Enquadramento | **NDJSON**: uma mensagem JSON por linha (`\n`), UTF-8 |
| Tamanho máximo | 64 KiB por linha; acima disso a conexão é encerrada (`E_SIZE`) |
| Conexões | 1 conexão por sessão; uma 2ª conexão com a mesma sessão é recusada (`E_SESSION`) |
| Servidor | criado pelo NexStage **só quando um jogo integrado é iniciado** e encerrado ao fim da sessão |
| Internet | não é usada para a comunicação |

### 2.1 Descoberta e credenciais (variáveis de ambiente do processo do jogo)

O NexStage inicia o jogo com estas variáveis (nada é passado pela linha de comando, que aparece em listas de processos com mais facilidade):

| Variável | Conteúdo |
|---|---|
| `NEXSTAGE_BRIDGE_PROTO` | versões de protocolo aceitas pelo launcher, ex. `1` |
| `NEXSTAGE_BRIDGE_PORT` | porta TCP local |
| `NEXSTAGE_BRIDGE_GAME` | `game_id` esperado (ex. `tomb-survivors`) |
| `NEXSTAGE_BRIDGE_SESSION` | id da sessão (UUID v4 aleatório) |
| `NEXSTAGE_BRIDGE_KEY` | segredo da sessão: 32 bytes aleatórios em base64 |
| `NEXSTAGE_BRIDGE_PROFILE` | **chave opaca do perfil** (UUID aleatório por perfil, guardado em `profile_settings`); o jogo nunca recebe o id numérico do perfil |
| `NEXSTAGE_BRIDGE_PROFILE_NAME` | nome de exibição (só para a interface do jogo) |

Sem essas variáveis o jogo roda em **modo independente** (sem tentar conectar).

---

## 3. Formato das mensagens

Cada linha é um **envelope** com a carga útil como **texto** e a assinatura calculada sobre esses bytes exatos
(evita depender de canonização de JSON entre Godot e Rust):

```json
{"p":"{\"v\":1,\"id\":\"…\",\"seq\":12,\"ts\":\"2026-11-20T15:00:00Z\",\"game\":\"tomb-survivors\",\"session\":\"…\",\"type\":\"achievement.unlocked\",\"data\":{…}}","mac":"<hex>"}
```

- `mac = HMAC-SHA256(KEY, utf8(p))`, em hexadecimal minúsculo.
- Carga útil (`p`, depois de verificada a assinatura):

| Campo | Tipo | Regra |
|---|---|---|
| `v` | int | versão do protocolo (1) |
| `id` | string | UUID v4 único do evento (gerado no jogo) |
| `seq` | int | sequência da sessão, estritamente crescente (começa em 1) |
| `ts` | string | horário UTC ISO-8601 do envio |
| `game` | string | igual a `NEXSTAGE_BRIDGE_GAME` |
| `session` | string | igual a `NEXSTAGE_BRIDGE_SESSION` |
| `type` | string | tipo do evento (§5) |
| `data` | objeto | conteúdo do tipo (validado por esquema) |
| `origin` | string, opcional | `live` (padrão) ou `offline` (evento gerado sem launcher, ver §7) |
| `at` | string, opcional | horário real em que o fato aconteceu (para eventos offline) |

Respostas do NexStage usam o mesmo envelope, assinadas com a mesma chave, `type` iniciado por `launcher.`.

---

## 4. Ciclo da sessão

```
NexStage                                   Jogo
   | inicia processo com variáveis (§2.1)   |
   |<------- conexão TCP --------------------|
   |<------- hello --------------------------|  (versão do jogo, slot, capacidades)
   |-------- launcher.welcome -------------->|  (protocolo aceito, catálogo em uso)
   |<------- game.started -------------------|
   |<------- outbox.flush (pendentes) -------|
   |-------- launcher.ack (por evento) ----->|
   |<------- game.heartbeat (15 s) ----------|
   |<------- achievement.* / statistics.* ---|
   |-------- launcher.ack ------------------>|
   |<------- game.closed --------------------|
   | fim do processo (tempo de jogo oficial) |
```

- **Tempo de jogo oficial** continua vindo do NexStage (fim do processo → `play_sessions`), como hoje.
  `game.heartbeat` só indica que a ponte está viva.
- **Desconexão:** sem mensagem por 45 s → sessão marcada como `lost`; o jogo continua e tenta reconectar
  (1 s, 2 s, 4 s… até 30 s) enquanto o processo vive. Eventos ficam na caixa de saída.
- **Recusa** (`launcher.reject`, com código): o jogo continua em modo independente.

---

## 5. Eventos

Tipos desconhecidos recebem `launcher.ack` com `status: "unsupported"` (não derrubam a sessão).
`unsupported` **não** tira o evento da caixa de saída do jogo: ele é reenviado em sessões futuras, quando o
launcher passar a aplicar aquele tipo.
Todo campo extra é ignorado; campos obrigatórios ausentes → `E_SCHEMA`.

| Tipo | Direção | `data` |
|---|---|---|
| `hello` | jogo→NS | `{ game_version, build, protocol: [1], slot: { uid, index, owner_profile, created_at }, caps: ["achievements","statistics","progress"] }` |
| `launcher.welcome` | NS→jogo | `{ protocol: 1, catalog_version, profile_bound: bool, server_ts }` |
| `launcher.reject` | NS→jogo | `{ code, message }` |
| `launcher.ack` | NS→jogo | `{ ack_id, status: "accepted"|"duplicate"|"rejected"|"unsupported", code?, message? }` |
| `game.started` | jogo→NS | `{ mode, slot_uid }` |
| `game.heartbeat` | jogo→NS | `{ state: "menu"|"playing"|"paused", mode? }` |
| `game.closed` | jogo→NS | `{ reason: "quit"|"crash_recovered"|"other" }` |
| `game.progress.updated` | jogo→NS | `{ slot_uid, percent: 0..100, details: { … } }` (progresso geral do slot) |
| `game.version.changed` | jogo→NS | `{ from, to }` (enviado após o `hello` quando o jogo detecta que foi atualizado) |
| `achievement.unlocked` | jogo→NS | `{ achievement_id, unlocked_at, slot_uid }` |
| `achievement.progress.updated` | jogo→NS | `{ achievement_id, current, target, slot_uid }` |
| `statistics.updated` | jogo→NS | `{ slot_uid, values: { <chave>: <valor>, … } }` (**valores absolutos**, não diferenças) |
| `outbox.flush` | jogo→NS | `{ events: [ <carga útil de evento>, … ] }` (até 200 por mensagem) |

---

## 6. Validação no NexStage (ordem)

1. Tamanho ≤ 64 KiB e JSON válido.
2. `mac` confere com a chave da sessão (comparação em tempo constante) → senão `E_AUTH` e encerra.
3. `v` suportado → senão `E_PROTO_VERSION`.
4. `game` e `session` iguais aos da sessão aberta → senão `E_SESSION`.
5. `seq` maior que o último recebido → senão `E_REPLAY`.
6. `id` ainda não visto para este jogo (tabela `bridge_events_seen`, guardada por 90 dias) → senão `duplicate`.
7. `ts` dentro de ±10 min do relógio do PC → senão ack `rejected` com `E_REPLAY` (mensagem antiga/adiantada).
8. Esquema do `type` válido → senão `E_SCHEMA`.
9. Regras do catálogo (manifesto): conquista existe, estatística permitida, limites (§9) → senão `E_CATALOG`.
10. Limite de ritmo: 50 mensagens/s → `E_RATE`.

O **perfil** é sempre o que o NexStage associou à sessão no momento em que abriu o jogo. Um id de perfil
enviado pelo jogo nunca é usado. Trocar de perfil no NexStage com o jogo aberto não muda a sessão.

---

## 7. Caixa de saída e modo independente

- O jogo grava todo evento relevante na **caixa de saída do slot** (`outbox`) antes de enviar.
- Ao receber `launcher.ack` com `accepted`, `duplicate` ou `rejected`, o evento sai da caixa.
- Jogo aberto **sem o NexStage**: os eventos ficam com `origin: "offline"` e `at` = horário real.
  Na próxima sessão pelo NexStage, vão em `outbox.flush` (assinados pela sessão nova).
- Eventos offline passam pelas mesmas regras, exceto a janela de `ts`, e com regras extras:
  só desbloqueio de conquista existente, progresso só sobe, estatísticas só sobem.
- A caixa de saída tem limite de 5.000 eventos por slot (os mais antigos de `statistics.updated` são
  compactados: basta o último valor).

---

## 8. Perfis e slots de save

O jogo tem **5 slots de save**, iguais abrindo pelo NexStage ou direto.

- Cada slot tem um `uid` aleatório e um campo `owner_profile` (chave opaca de perfil do NexStage, ou vazio).
- **Vínculo:** na primeira vez que um slot sem dono é jogado por uma sessão do NexStage, ele passa a
  pertencer àquele perfil (`owner_profile` = chave da sessão).
- Slot de outro perfil, aberto por uma sessão do NexStage: o jogo avisa ("Este slot pertence a outro perfil
  do NexStage") e permite jogar, mas **não sincroniza** (o NexStage recusa com `E_PROFILE_MISMATCH`).
  Isso impede que dois perfis dividam conquistas por acidente.
- Slot com dono, aberto **sem** NexStage: joga normalmente; os eventos ficam na caixa de saída e serão
  enviados quando o dono abrir o jogo pelo NexStage.
- Slot sem dono, aberto sem NexStage: joga normalmente; os eventos ficam guardados e são enviados para o
  perfil que vincular o slot depois.
- Conquistas no NexStage pertencem ao **perfil** e nunca são revertidas (mesmo se o slot for apagado).
- Estatísticas no NexStage são guardadas **por perfil e por slot** (`slot_uid`); a página do jogo mostra a
  soma/máximo dos slots do perfil.

---

## 9. Catálogo do jogo (vem no manifesto assinado)

### 9.1 Conquistas

```json
{
  "id": "guardian_slayer",
  "name": { "pt-BR": "Guardião Derrotado", "en": "Guardian Slain" },
  "description": { "pt-BR": "Vença o Sacerdote-Guardião.", "en": "Defeat the Guardian Priest." },
  "icon": "achievements/guardian_slayer.png",
  "category": "progressao",
  "rarity": "epica",
  "points": 90,
  "target": null,
  "hidden": false,
  "mode": "classic",
  "since": "0.2.0"
}
```

- `rarity` → `points` (mesma escala de `score.rs`): `comum` 15 · `rara` 30 · `epica` 90 · `lendaria` 180.
- `target`: número para conquistas com progresso (ex. 10000 abates); `null` = sem progresso.
- `mode`: modo de jogo (`classic` hoje; `infinite` no futuro). Conquistas de um modo que ainda não existe
  **não entram no catálogo publicado** até o modo existir.

### 9.2 Estatísticas permitidas

```json
{ "key": "total_kills", "kind": "counter", "label": { "pt-BR": "Inimigos derrotados" }, "max_step": 100000 }
```

- `kind`: `counter` (só sobe), `max` (só sobe), `duration_s` (só sobe), `ref` (texto curto, ex. id da arma).
- `max_step`: maior aumento aceito entre dois envios (descarta valores absurdos).
- Chaves fora do catálogo são ignoradas (`E_CATALOG` no ack, sem derrubar a sessão).

---

## 10. Manifesto de distribuição

Publicado em um **repositório público de distribuição** (ex. `bbbaby999/tomb-survivors-releases`),
em cada release e em `releases/latest/download/manifest.json`.

```json
{
  "schema": 1,
  "game_id": "tomb-survivors",
  "name": "Tomb Survivors",
  "version": "0.2.0-alpha",
  "build": 7,
  "released_at": "2026-12-01T12:00:00Z",
  "min_nexstage": "1.4.0",
  "protocol": [1],
  "requirements": { "os": "windows-x86_64", "gpu": "Vulkan 1.2" },
  "executable": "TombSurvivors.exe",
  "files": [
    { "path": "TombSurvivors.exe", "size": 93000000, "sha256": "…", "url": "https://github.com/bbbaby999/tomb-survivors-releases/releases/download/v0.2.0-alpha/TombSurvivors.exe" },
    { "path": "TombSurvivors.pck", "size": 98000000, "sha256": "…", "url": "https://github.com/…/TombSurvivors.pck" }
  ],
  "assets": { "cover": "https://…/cover.jpg", "logo": "https://…/logo.png", "icons_base": "https://…/achievements/" },
  "notes": { "pt-BR": "…", "en": "…" },
  "catalog": { "version": 3, "achievements": [ … ], "statistics": [ … ] }
}
```

- Assinatura **destacada**: `manifest.json.sig` = Ed25519 (base64) sobre os bytes de `manifest.json`.
  A chave pública fica embutida no NexStage (por jogo, em uma lista de jogos oficiais). Verificação com
  `ring` (já é dependência).
- Regras: URLs só `https://` e só em hosts permitidos (`github.com`, `objects.githubusercontent.com`);
  `path` sem `..`, sem caminho absoluto, sem `:`; executável deve estar na lista `files`.
- O manifesto **nunca** contém comandos, scripts ou argumentos de execução.

---

## 11. Instalação, atualização e desinstalação

Pasta: `%LOCALAPPDATA%\NEXSTAGE\games\<game_id>\`

```
tomb-survivors\
  current            (texto: versão ativa)
  versions\0.1.0-alpha\…
  versions\0.2.0-alpha\…
  staging\           (downloads em andamento; apagada ao concluir/cancelar)
```

1. Baixar e verificar o manifesto (assinatura → esquema → regras).
2. Para cada arquivo: se uma versão instalada já tem o mesmo `sha256`, copiar localmente (atualização
   incremental por arquivo); senão baixar para `staging\` com retomada (`Range`), pausável.
3. Conferir tamanho e SHA-256 de todos os arquivos. Falha → apagar `staging\`, nada muda.
4. Mover `staging\` para `versions\<versão>\` (renomeação atômica no mesmo disco).
5. Gravar `current` (arquivo temporário + renomear).
6. Manter só a versão anterior para **voltar** (rollback) e apagar as mais antigas.
- **Jogo aberto** durante a atualização: a troca espera o processo terminar.
- **Verificar integridade:** recalcula o SHA-256 da versão ativa contra o manifesto guardado.
- **Desinstalar:** apaga a pasta do jogo. Os **saves não são tocados** (ficam em
  `%APPDATA%\Godot\app_userdata\…`); a interface avisa isso.

---

## 12. Reaproveitamento no NexStage

| Necessidade | Reaproveita |
|---|---|
| Tempo de jogo, histórico, "jogado por último" | `launcher.rs` (`launch_game` / `end_session`), `play_sessions`, `profile_games` |
| Jogo na biblioteca | tabela `games` com `source = 'nexstage'`, `external_id = <game_id>` |
| Notificação de conquista | `shot_toast::notify_achievements` com `source: "nexstage"` (respeita `ach_toast`/`ach_sound`) |
| Download HTTPS, SHA-256, aleatório, HMAC, Ed25519 | `reqwest`, `sha2`, `getrandom`, `ring` |
| Servidor local | `tokio` (já usado pelo Tauri; ativar recursos `net`, `io-util`) |
| Pontuação / futuro Rewards | `score.rs` ganha a fonte `nexstage` |
| Backup | tabelas novas entram no backup automaticamente |

**Novas dependências Rust:** nenhuma (apenas recursos do `tokio`).

---

## 13. Tabelas novas no NexStage (migração aditiva)

```sql
CREATE TABLE IF NOT EXISTS integrated_games (
  game_id TEXT PRIMARY KEY, name TEXT NOT NULL,
  installed_version TEXT, installed_build INTEGER, install_dir TEXT,
  manifest TEXT, manifest_checked_at TEXT, library_game_id INTEGER REFERENCES games(id) ON DELETE SET NULL
);
CREATE TABLE IF NOT EXISTS bridge_sessions (
  session_id TEXT PRIMARY KEY, game_id TEXT NOT NULL, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  game_version TEXT, slot_uid TEXT, started_at TEXT NOT NULL, ended_at TEXT, status TEXT NOT NULL, last_seq INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS bridge_achievements (
  game_id TEXT NOT NULL, achievement_id TEXT NOT NULL, data TEXT NOT NULL, catalog_version INTEGER NOT NULL,
  PRIMARY KEY (game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_unlocks (
  profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  game_id TEXT NOT NULL, achievement_id TEXT NOT NULL,
  unlocked_at TEXT NOT NULL, origin TEXT NOT NULL, event_id TEXT NOT NULL,
  PRIMARY KEY (profile_id, game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_progress (
  profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  game_id TEXT NOT NULL, achievement_id TEXT NOT NULL, current INTEGER NOT NULL, target INTEGER NOT NULL, updated_at TEXT NOT NULL,
  PRIMARY KEY (profile_id, game_id, achievement_id)
);
CREATE TABLE IF NOT EXISTS bridge_stats (
  profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  game_id TEXT NOT NULL, slot_uid TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, updated_at TEXT NOT NULL,
  PRIMARY KEY (profile_id, game_id, slot_uid, key)
);
CREATE TABLE IF NOT EXISTS bridge_events_seen (
  game_id TEXT NOT NULL, event_id TEXT NOT NULL, seen_at TEXT NOT NULL,
  PRIMARY KEY (game_id, event_id)
);
```

- `INSERT OR IGNORE` em `bridge_unlocks`: desbloqueio repetido nunca altera nem remove o existente.
- As tabelas antigas `achievements` / `profile_achievements` (nunca usadas) **não são reaproveitadas** e
  continuam onde estão; remoção só com autorização.

---

## 14. Códigos de erro

| Código | Quando | Efeito |
|---|---|---|
| `E_SIZE` | linha > 64 KiB | encerra conexão |
| `E_AUTH` | assinatura inválida | encerra conexão |
| `E_PROTO_VERSION` | versão não suportada | `launcher.reject` |
| `E_SESSION` | jogo/sessão diferentes, 2ª conexão | encerra conexão |
| `E_REPLAY` | `seq` repetida/menor | ack `rejected` |
| `E_SCHEMA` | campos inválidos | ack `rejected` |
| `E_CATALOG` | conquista/estatística fora do catálogo ou fora dos limites | ack `rejected` |
| `E_PROFILE_MISMATCH` | slot de outro perfil | ack `rejected` (jogo para de enviar daquele slot) |
| `E_RATE` | > 50 msgs/s | ack `rejected` |

Erros são gravados no registro local do NexStage (`diaglog`), sem dados pessoais.

---

## 15. Versionamento e compatibilidade

- Protocolo: inteiro `v`. Mudanças compatíveis (campos novos opcionais, tipos novos) **não** mudam `v`.
  Mudança incompatível → `v` + 1; o launcher aceita a lista `NEXSTAGE_BRIDGE_PROTO` e o jogo escolhe a maior comum.
- Manifesto: `schema` inteiro, mesma regra. `min_nexstage` impede instalar em launcher antigo.
- Catálogo: `catalog.version` cresce a cada mudança; conquistas nunca são removidas, só marcadas `retired`.

---

## 16. Segurança (modelo de ameaça)

| Ameaça | Proteção | Limite conhecido |
|---|---|---|
| Outro programa injeta eventos | HMAC com chave por sessão, só loopback, 1 conexão | programas do **mesmo usuário** do Windows podem ler o ambiente do processo do jogo |
| Repetição de eventos | `seq` crescente, `id` único guardado (90 dias), janela de `ts` ±10 min | — |
| Repetição de respostas do NexStage | o jogo exige `seq` crescente do launcher (também entre reconexões), mesma sessão e jogo | — |
| Adivinhar a assinatura pelo tempo | comparação em tempo constante nos dois lados (`ring::hmac::verify`; `_same` no jogo) | — |
| Inundação / mensagens enormes | linha ≤ 64 KB, 50 mensagens/s, ≤ 200 pendentes por `outbox.flush`, 45 s sem mensagem = conexão perdida, ≤ 300 estatísticas | — |
| Conquista inventada | catálogo vem do manifesto assinado | — |
| Estatística absurda | só chaves do catálogo assinado, tipo conferido, números 0–10¹², nunca descem num slot | um save editado à mão pode mandar números altos (ver última linha) |
| Dados de outro perfil | sessão presa ao perfil no NexStage, vínculo do slot | — |
| Manifesto/arquivo malicioso | Ed25519, só HTTPS em hosts do GitHub (redirecionamentos também), SHA-256 e tamanho de cada arquivo, caminhos validados, nenhum comando executado | — |
| Instalação quebrada | staging + tudo conferido antes da troca + versão anterior guardada + retomada automática de download | — |
| **Trapaça do próprio usuário** | — | jogo local: quem controla o PC pode editar o save. Conquistas globais (Rewards) exigirão validação em servidor no futuro |

---

## 17. Preparado para o futuro (fora desta versão)

- **NexStage Rewards:** a pontuação das conquistas integradas já usa a escala de `score.rs`; a fonte
  `nexstage` entra no cálculo; o Rewards somará fontes sem mudar o protocolo.
- **Nuvem:** slots têm `uid` estável e eventos têm `id` único — base para sincronização futura.
- **Outros jogos:** basta um manifesto, uma chave pública na lista de jogos oficiais e o cliente do Bridge.

---

## 19. Estado da implementação

### Fase 3 — comunicação local (concluída)

NexStage (`src-tauri/src/bridge/`):
- `protocol.rs` — envelope assinado (HMAC-SHA256 via `ring`, verificação em tempo constante), campos, formatos dos
  eventos, horário UTC sem dependências, UUID v4.
- `session.rs` — validação na ordem do §6, respostas assinadas, `outbox.flush` com confirmação por evento,
  vínculo de slot (`E_PROFILE_MISMATCH`), reconexão (novo `hello`, sequência continua).
- `server.rs` — TCP em 127.0.0.1, porta efêmera, uma conexão por vez, 45 s sem mensagem = conexão perdida.
- `store.rs` — tabelas do §13 (todas criadas; conquistas/estatísticas preenchidas nas Fases 6/8).
- `launcher.rs` — jogos com `source = 'nexstage'` recebem as variáveis da sessão; a sessão termina com o processo.
- `Cargo.toml` — só recursos novos do `tokio` (`net`, `io-util`; testes: `macros`, `rt`, `process`).

**Provisório (documentado), resolvido nas próximas fases:**
- `achievement.*`, `statistics.updated` e `game.progress.updated` são reconhecidos e validados, mas respondidos
  `unsupported` (ficam guardados no jogo) até as Fases 6 e 8.
- A conferência pelo catálogo (`E_CATALOG`) depende do manifesto (Fase 5).
- A lista de jogos oficiais é a constante `OFFICIAL_GAMES` até o cadastro com manifesto (Fases 4 e 5).

Testes: `cargo test --lib bridge` (10 testes) e o teste de ponta a ponta com o jogo real:
`NEXSTAGE_E2E_GODOT` + `NEXSTAGE_E2E_PROJECT` e `cargo test --lib bridge::server::tests::e2e_game -- --ignored`.

### Fase 4 — cadastro na biblioteca (concluída)

NexStage:
- `src-tauri/src/integrated.rs` — catálogo dos jogos oficiais (Tomb Survivors: executável, arquivo de dados, capa e
  arte de fundo embutidas em `src-tauri/assets/integrated/`); presença automática na biblioteca; local da instalação.
  - O jogo entra **uma vez** na biblioteca de cada perfil; se o usuário remover, não volta sozinho
    (`profile_settings: integrated_added:<id>`). Todos os perfis usam a mesma linha em `games` (`source='nexstage'`).
  - Comandos: `integrated_status`, `integrated_locate` (aponta uma cópia existente do jogo — exige o `.exe` e o
    `.pck` juntos; lê `version.txt` se houver), `integrated_unlink` (não apaga arquivos), `integrated_add_to_library`.
- `commands/games.rs` — `list_library` garante a presença do jogo; `update_game` não deixa trocar executável/argumentos
  de jogo integrado; `Game` ganhou `externalId`.
- `commands/launcher.rs` — jogo integrado não instalado (ou pasta apagada) mostra uma mensagem clara em vez de tentar abrir.
- `bridge/mod.rs` — a lista de jogos oficiais agora vem de `integrated` (`is_official`).
- Interface: `components/NexStageGamePanel.tsx` (bloco "NexStage": estado, versão, pasta, Localizar/Trocar/Desvincular),
  selo "NexStage" e botão principal "Localizar instalação" em `features/GameDetail.tsx`, campo de executável oculto em
  `components/GameForm.tsx`; textos nos 10 idiomas (`locales/*`).

**Provisório:** o botão **Instalar** (download pelo NexStage) entra na Fase 5; até lá, a instalação é por
"Localizar instalação" (zip baixado do release).

Testes: `integrated::tests` (presença única por perfil, remoção respeitada, instalação exige `.exe` + `.pck`, versão).

### Fase 5 — instalação e atualização pelo NexStage (concluída)

Distribuição:
- Repositório **público** `bbbaby999/tomb-survivors-releases` (só downloads). Primeira versão: **v0.2.0-alpha** (build 26).
- Cada release: `TombSurvivors.exe` (exportação final, sem "(DEBUG)"), `TombSurvivors.pck`, `version.txt`,
  `manifest.json`, `manifest.json.sig` (Ed25519), capa/arte e o zip para download manual.
- O NexStage procura em `releases/latest/download/manifest.json` (releases publicados como normais, não pré-lançamento).
- Ferramentas no repositório do jogo: `release_tools/keygen.mjs`, `build_release.mjs`, `publish.mjs`.
  A chave privada fica em `%USERPROFILE%\.nexstage-keys\` (fora dos repositórios); a pública está em `integrated/mod.rs`.

NexStage (`src-tauri/src/integrated/`):
- `manifest.rs` — assinatura Ed25519 (`ring`), formato, versão mínima do NexStage, protocolo, URLs só HTTPS em hosts do
  GitHub, caminhos seguros, executável na lista.
- `install.rs` — download com retomada (`Range`), pausa e cancelamento, atualização incremental (arquivo igual é copiado
  da versão instalada e conferido), SHA-256 de tudo antes da troca, `staging` → `versions\<v>` → `current`, versão
  anterior guardada, verificação, reparo, volta de versão, desinstalação. Redirecionamentos só para hosts permitidos.
- `mod.rs` — comandos `integrated_check_update`, `integrated_install` (também retoma), `integrated_pause`,
  `integrated_cancel`, `integrated_verify`, `integrated_rollback`, `integrated_uninstall`; eventos
  `integrated-progress`, `integrated-status`, `integrated-error`. Nada é trocado com o jogo aberto.
- Interface: bloco NexStage com Instalar (tamanho), barra de progresso, Pausar/Continuar/Cancelar, Atualizar, Procurar
  atualização, Novidades, Verificar arquivos/Reparar, Voltar para a versão anterior, Desinstalar (com confirmação) e
  "Já tenho o jogo: localizar instalação"; botão principal da página vira Instalar/progresso. 32 textos em 10 idiomas.

Testes: `integrated::manifest` (assinatura, adulteração, outra chave, outro jogo, NexStage antigo, hosts, caminhos) e
`integrated::install` com servidor HTTP local (instalação limpa, atualização incremental sem baixar o que não mudou,
adulteração detectada, reparo, volta de versão, desinstalação, download corrompido sem alterar nada, pausa/retomada,
cancelamento). Ponta a ponta com o release real: `cargo test --lib integrated::tests::e2e_real_release -- --ignored`.

### Fase 6 — conquistas (concluída)

- `bridge/achievements.rs`: catálogo guardado SÓ a partir do manifesto com assinatura conferida (`latest_manifest`),
  nunca rebaixado para uma versão menor; definições inválidas ignoradas. `achievement.unlocked` e
  `achievement.progress.updated` conferidos contra o catálogo (id existente, não reservado, mesmo alvo) → `E_CATALOG`.
  Desbloqueio permanente e idempotente por perfil; data original do jogo para eventos offline (data futura vira agora);
  progresso só sobe e não desbloqueia sozinho.
- Sessão: conquistas só do slot liberado na conexão; sem catálogo ainda → `unsupported` (o jogo guarda para depois);
  `welcome.catalog_version` real; `Core.fresh_unlocks` lista os desbloqueios novos (avisos na Fase 7).
- Comando `integrated_achievements(profileId, gameId)`; bloco Conquistas na página do jogo (progresso, pontos,
  lista com filtros). 8 textos novos em 10 idiomas.
- Instalador: queda de conexão no meio de um arquivo é retomada sozinha (até 3 tentativas, continuando do `.part`).
- Testes: `bridge::achievements` (3), sessão com catálogo, queda de conexão retomada, e o ponta a ponta com o jogo real
  (`e2e_game`, agora também confere conquista aceita e conquista inventada recusada).

### Fase 7 — avisos (concluída)

- Conquista nova (primeira vez no perfil): cartão por cima do jogo pela janela de avisos já existente
  (`shot_toast::notify_achievements`, fonte `nexstage`, respeita Configurações → Notificações). Nome no idioma do app;
  épicas e lendárias usam o estilo raro; pendentes sincronizadas de uma vez mostram até 5 cartões + "e mais N".
  Reenvios, outros slots do mesmo perfil e progresso não avisam. O jogo só mostra o próprio aviso quando roda sem o NexStage.
- `ConnSource::unlocked` é chamado fora do lock do banco, depois de cada mensagem.
- Ao abrir o app (25 s depois): procura versão nova dos jogos integrados instalados (e traz o catálogo); avisa uma vez
  por versão (`app_settings.integrated_update_notified:<id>`), evento `integrated-update-available`.
- `integrated-installed` (instalação concluída) e `integrated-error` viram avisos globais do app, em qualquer tela.
- Testes: `toasts_use_app_language_and_summarize`, `new_unlock_triggers_notification_once` (TCP real).

### Fase 8 — estatísticas e perfis (concluída)

- `bridge/statistics.rs` + tabela `bridge_stat_defs` (aditiva): definições só do manifesto assinado
  (`catalog.statistics`, tipos `counter`, `max`, `duration_s`, `max_duration_s`, `ref`). `statistics.updated` guarda
  valores por perfil + slot (`bridge_stats`): números nunca descem num slot, textos são substituídos, chaves fora do
  catálogo ignoradas, valor de tipo errado → `E_SCHEMA`, nenhuma chave conhecida → `E_CATALOG`.
  `game.progress.updated` vira `progress_percent` do slot (sem precisar de catálogo).
- Perfil = soma dos slots dele: contadores/durações somados, recordes pelo maior, texto do slot mais recente.
  Comando `integrated_stats(profileId, gameId)`; bloco Estatísticas na página do jogo (destaques + janela completa).
- Pontuação do perfil (`score.rs`): nova fonte `nexstage` — pontos do catálogo assinado (15/30/90/180) + bônus de 180
  com todas as conquistas disponíveis. Aparece no painel de pontuação e no perfil.
- Perfis: tudo por `profile_id`; apagar um perfil apaga os dados dele (chaves estrangeiras em cascata); slot de outro
  perfil continua jogável sem sincronizar.
- Testes: `bridge::statistics` (2), sessão com estatísticas/progresso, `nexstage_points_come_from_signed_catalog`,
  e o ponta a ponta com o jogo real agora confere estatísticas e progresso registrados.

### Fase 9 — página especial (concluída)

- `features/NexStagePage.tsx`: na página de um jogo oficial (`source = nexstage`), abas próprias no lugar de DLCs/Mods
  — Visão geral (anel de progresso, pontos, destaques, conquistas recentes e "quase lá", descrição oficial),
  Conquistas, Estatísticas, Slots (slots do jogo usados pelo perfil, com número, progresso e recordes) e Novidades
  (notas da versão publicada + página de todas as versões). L2/R2 passam pelas abas. Tema "tumba" (`.nx-theme`).
  Busca de IGDB/Steam e "Sobre o jogo" genérico não aparecem para jogos oficiais.
- Backend: tabela `bridge_slots` (aditiva; `game.started` agora traz `slot_number`), comandos `integrated_info`
  (descrição oficial pt-BR/en, link das versões) e `integrated_slots`. Teste `slots_list_number_and_values`.
- 21 textos novos em 10 idiomas.

### Fase 10 — testes completos, segurança e documentação (concluída)

Revisão de segurança (Bridge, instalador, catálogos, jogo). Corrigido no jogo: assinatura comparada em tempo
constante e respostas do NexStage com sequência obrigatoriamente crescente (antes, uma resposta repetida na mesma sessão
seria aceita). Tabela do §16 atualizada com o que está implementado de fato.

**Como rodar todos os testes**

| O quê | Comando |
|---|---|
| NexStage (Rust) | `cd src-tauri && cargo test --lib` |
| Ponta a ponta com o jogo real | `NEXSTAGE_E2E_GODOT=<Godot console .exe> NEXSTAGE_E2E_PROJECT=<pasta do jogo> cargo test --lib e2e_game -- --ignored` |
| Instalação do release publicado (internet, ~200 MB) | `cargo test --lib e2e_real_release -- --ignored` |
| Interface (tipos, idiomas, testes) | `npm run check` e `npm test` |
| Jogo: slots / conquistas e estatísticas | `<Godot> --path . --headless -- --slots-selftest` e `-- --ach-selftest` (usam o slot 5 só se vazio) |
| Jogo: partida completa automática | `<Godot> --path . res://scenes/Main.tscn -- --bot --autopick --nometa --speed=6 --stats` |

Observação: neste PC a conexão local perde pacotes de vez em quando (o Windows derruba a conexão após ~19 s); o
instalador retoma sozinho, então os testes passam, mas a rodada completa pode levar ~20 s em vez de ~1 s.

## 18. Plano de testes (Fases 3–10)

Comunicação: conexão, assinatura inválida, sequência repetida, evento duplicado, esquema inválido,
tipo desconhecido, queda e reconexão, 2ª conexão, limite de ritmo.
Conquistas: desbloqueio ao vivo, NexStage fechado durante o desbloqueio (caixa de saída), repetição,
troca de perfil, dois perfis, slot de outro perfil.
Instalação: limpa, atualização, atualização interrompida, rollback, integridade, desinstalação, manifesto
com assinatura inválida, URL fora da lista, caminho com `..`.
Jogo: abertura independente, abertura pelo NexStage, dados corrompidos (save e caixa de saída),
recuperação após reinício.
