// NEXSTAGE — serviço de feedback (Cloudflare Worker).
//
// Recebe o relato enviado pelo app e cria uma Issue no repositório de feedback do GitHub.
// O token do GitHub fica SÓ aqui, como segredo do Worker (nunca no app distribuído):
//   GITHUB_TOKEN  (segredo)  token fine-grained com acesso só ao repositório de feedback, permissão Issues: leitura e escrita
//   GITHUB_REPO   (variável) "usuario/repositorio" de destino (recomendado: repositório PRIVADO)
// Veja feedback-worker/README.md para publicar.

const CATEGORIES = {
  improvement: { label: "melhoria", title: "Melhoria" },
  feature: { label: "nova funcionalidade", title: "Funcionalidade" },
  bug: { label: "bug", title: "Bug" },
  general: { label: "comentário", title: "Comentário" },
};
const ATTACHMENTS = ["diagnostico.txt", "nexstage.log"];
const MAX_REQUEST = 200 * 1024; // bytes
const MAX_BODY = 60000; // limite da Issue no GitHub: 65 536 caracteres
const RATE = { max: 5, windowSec: 600 }; // por IP

const json = (status, data) => new Response(JSON.stringify(data), { status, headers: { "content-type": "application/json" } });
const str = (v, max) => (typeof v === "string" ? v.trim().slice(0, max) : "");
// evita que o conteúdo feche o bloco de código ou mencione/chame pessoas no GitHub
const fence = (s) => s.replace(/```/g, "ʼʼʼ");
const quiet = (s) => s.replace(/@(?=[A-Za-z0-9-])/g, "@​");

async function limited(request) {
  const ip = request.headers.get("cf-connecting-ip") || "unknown";
  const key = new Request(`https://rate-limit.nexstage/${encodeURIComponent(ip)}`);
  const cache = caches.default;
  const hit = await cache.match(key);
  const count = hit ? Number(await hit.text()) || 0 : 0;
  if (count >= RATE.max) return true;
  await cache.put(key, new Response(String(count + 1), { headers: { "cache-control": `max-age=${RATE.windowSec}` } }));
  return false;
}

function buildBody(d, attachments) {
  const cat = CATEGORIES[d.category];
  let head = `| | |\n|---|---|\n| **Categoria** | ${cat.title} |\n| **Versão** | NEXSTAGE ${d.version} |\n| **Enviado em** | ${d.sentAt || "—"} |\n| **Idioma** | ${d.language || "—"} |\n| **Anexos** | ${attachments.length ? attachments.map((a) => a.name).join(", ") : "nenhum"} |\n`;
  if (d.comments) head += `\n### Comentários e sugestões\n\n${quiet(d.comments)}\n`;
  if (d.problem) head += `\n### Descrição do problema\n\n${quiet(d.problem)}\n`;
  head += "\n---\n_Enviado pelo NEXSTAGE (Configurações → Feedback e relatórios)._\n";
  // anexos dividem o espaço que sobra (o fim do log, onde estão os erros recentes, é preservado)
  // os menores levam o que precisam; o que sobra vai para os maiores
  let room = Math.max(0, MAX_BODY - head.length - 200 * attachments.length);
  const budget = new Map();
  [...attachments].sort((x, y) => x.content.length - y.content.length).forEach((a, i, all) => {
    const take = Math.min(a.content.length, Math.floor(room / (all.length - i)));
    budget.set(a.name, take);
    room -= take;
  });
  const parts = attachments.map((a) => {
    let c = a.content;
    const each = budget.get(a.name);
    if (c.length > each) c = "[…início omitido…]\n" + c.slice(c.length - each + 40);
    return `\n<details><summary>${a.name}</summary>\n\n\`\`\`text\n${fence(c)}\n\`\`\`\n</details>\n`;
  });
  return head + parts.join("");
}

async function createIssue(env, title, body, labels) {
  const res = await fetch(`https://api.github.com/repos/${env.GITHUB_REPO}/issues`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${env.GITHUB_TOKEN}`,
      accept: "application/vnd.github+json",
      "x-github-api-version": "2022-11-28",
      "user-agent": "nexstage-feedback-worker",
      "content-type": "application/json",
    },
    body: JSON.stringify(labels ? { title, body, labels } : { title, body }),
  });
  return res;
}

export default {
  async fetch(request, env) {
    if (request.method !== "POST") return json(405, { error: "method" });
    if (request.headers.get("x-nexstage-client") !== "feedback-1") return json(400, { error: "client" });
    if (!env.GITHUB_TOKEN || !env.GITHUB_REPO) return json(503, { error: "not configured" });
    const len = Number(request.headers.get("content-length") || 0);
    if (len > MAX_REQUEST) return json(413, { error: "too large" });

    let d;
    try {
      const raw = await request.text();
      if (raw.length > MAX_REQUEST) return json(413, { error: "too large" });
      d = JSON.parse(raw);
    } catch {
      return json(400, { error: "json" });
    }
    const data = {
      category: str(d.category, 20),
      title: str(d.title, 120),
      comments: str(d.comments, 8000),
      problem: str(d.problem, 8000),
      version: str(d.version, 20).replace(/[^0-9A-Za-z.\-+]/g, ""),
      sentAt: str(d.sentAt, 64),
      language: str(d.language, 12),
    };
    if (!CATEGORIES[data.category] || data.title.length < 3 || (!data.comments && !data.problem)) return json(400, { error: "invalid" });
    const attachments = (Array.isArray(d.attachments) ? d.attachments : [])
      .filter((a) => a && ATTACHMENTS.includes(a.name) && typeof a.content === "string")
      .slice(0, ATTACHMENTS.length)
      .map((a) => ({ name: a.name, content: a.content.slice(0, 60000) }));

    if (await limited(request)) return json(429, { error: "rate" });

    const cat = CATEGORIES[data.category];
    const title = `[${cat.title}] ${quiet(data.title)}`;
    const body = buildBody(data, attachments);
    let res = await createIssue(env, title, body, ["feedback", cat.label]);
    if (res.status === 422) res = await createIssue(env, title, body, null); // rótulos indisponíveis: cria sem eles
    if (!res.ok) return json(502, { error: "github", status: res.status });
    const issue = await res.json();
    return json(200, { ok: true, id: issue.number });
  },
};
