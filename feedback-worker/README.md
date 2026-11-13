# Serviço de feedback do NEXSTAGE

O app envia os relatos de **Configurações → Atualizações → Feedback e relatórios** para este
Cloudflare Worker, que cria uma Issue no GitHub. O token do GitHub fica **só aqui**, como segredo do
Worker: o app distribuído não contém credencial nenhuma, e quem envia não precisa de conta no GitHub.

O plano gratuito da Cloudflare basta (100 mil requisições por dia).

## Proteções
- Aceita só `POST` com o cabeçalho do app, até 200 KB, com categoria e campos válidos.
- Até 5 envios a cada 10 minutos por IP.
- Só os anexos conhecidos (`diagnostico.txt`, `nexstage.log`), com o texto em blocos de código e menções `@` neutralizadas.
- O app já filtra dados pessoais dos anexos antes de enviar; o usuário revisa tudo antes.

## Publicar (uma vez só)

1. **Crie um repositório PRIVADO para receber os relatos**, por exemplo `nexstage-feedback`. Issues
   de repositório público ficam visíveis para qualquer pessoa, inclusive os anexos de diagnóstico.
2. **Crie um token fine-grained** em GitHub → Settings → Developer settings → Personal access tokens →
   Fine-grained tokens:
   - Repository access: **Only select repositories** → o repositório de feedback;
   - Permissions → Repository → **Issues: Read and write** (nada mais);
   - Defina uma validade e anote a data para renovar.
3. **Publique o Worker** (precisa de uma conta gratuita na Cloudflare), na pasta `feedback-worker`:
   ```
   npx wrangler login
   npx wrangler secret put GITHUB_TOKEN
   npx wrangler deploy
   ```
   Se o repositório não for `bbbaby999/nexstage-feedback`, ajuste `GITHUB_REPO` em `wrangler.toml` antes do deploy.
   O deploy mostra o endereço, algo como `https://nexstage-feedback.SEU-SUBDOMINIO.workers.dev`.
4. **Ligue o app ao Worker** (na pasta do projeto) e recompile:
   ```
   node scripts/setup-feedback.mjs https://nexstage-feedback.SEU-SUBDOMINIO.workers.dev
   ```

Sem o passo 4, a tela de feedback aparece no app com o aviso "envio ainda não configurado".

## Trocar ou revogar o token
Gere um novo token e rode `npx wrangler secret put GITHUB_TOKEN` de novo. O app não muda.
