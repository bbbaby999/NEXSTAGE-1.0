/**
 * Conversor SEGURO do BBCode dos anúncios da Steam ([p], [h2], [list], [*], [b], [url=...]...) para
 * elementos React — nunca injeta HTML. Imagens/vídeos incorporados são omitidos (as notas completas
 * com mídia ficam no link oficial). Links só https e abertos no navegador do sistema.
 */
import type { ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

interface Node { tag: string; arg?: string; kids: (Node | string)[] }

const BLOCK = new Set(["p", "h1", "h2", "h3", "h4", "list", "olist", "*", "quote", "code", "table", "tr", "td", "th", "hr"]);
const DROP = new Set(["img", "previewyoutube", "video", "dynamiclink", "carousel", "spoiler"]);
const KNOWN = new Set([...BLOCK, ...DROP, "b", "i", "u", "s", "strike", "url", "noparse", "emoticon", "expand"]);

/** "\[" é colchete literal no BBCode da Steam (ex.: "\[ GAMEPLAY ]"). */
const ESC = "\u0001";

export function parseBBCode(src: string): Node {
  const text = src.replace(/\\\[/g, ESC).replace(/\{STEAM_CLAN_IMAGE\}[^\s[\]]*/g, "").replace(/\r/g, "");
  const root: Node = { tag: "root", kids: [] };
  const stack: Node[] = [root];
  // [tag], [tag=valor] ou [tag atributo="..."] (formato novo da Steam: [p align="start"], [url href="..."])
  const re = /\[(\/?)([a-z*0-9]+)(?:=([^\]]*)|(\s[^\]]*))?\]/gi;
  let last = 0;
  let m: RegExpExecArray | null;
  const push = (s: string) => { if (s) stack[stack.length - 1].kids.push(s.replace(new RegExp(ESC, "g"), "[")); };
  while ((m = re.exec(text))) {
    const tag = m[2].toLowerCase();
    if (!KNOWN.has(tag)) continue; // colchetes comuns no texto
    push(text.slice(last, m.index));
    last = re.lastIndex;
    if (m[1]) {
      // fecha até a tag correspondente (tolerante a BBCode malformado)
      const i = stack.map((n) => n.tag).lastIndexOf(tag);
      if (i > 0) stack.length = i;
      continue;
    }
    if (tag === "hr") { stack[stack.length - 1].kids.push({ tag, kids: [] }); continue; }
    if (tag === "*") {
      // item de lista: fecha o item anterior aberto
      const i = stack.map((n) => n.tag).lastIndexOf("*");
      if (i > 0 && stack.slice(i).every((n) => n.tag !== "list" && n.tag !== "olist")) stack.length = i;
    }
    // com atributos, só o endereço do link interessa (alinhamento e afins são ignorados)
    const arg = m[3] ?? (m[4] ? /\bhref\s*=\s*["']?([^"'\s]+)/i.exec(m[4])?.[1] : undefined);
    const n: Node = { tag, arg, kids: [] };
    stack[stack.length - 1].kids.push(n);
    stack.push(n);
  }
  push(text.slice(last));
  return root;
}

const plain = (n: Node | string): string =>
  typeof n === "string" ? n : DROP.has(n.tag) ? "" : n.kids.map(plain).join(BLOCK.has(n.tag) ? " " : "");

/** Resumo: os primeiros itens de lista (as mudanças) ou o começo do texto. */
export function summarize(src: string, maxItems = 4, maxChars = 280): { items: string[]; text: string } {
  const root = parseBBCode(src);
  const items: string[] = [];
  const walk = (n: Node) => {
    for (const k of n.kids) {
      if (typeof k === "string" || items.length >= maxItems) continue;
      if (k.tag === "*") {
        const s = plain(k).replace(/\s+/g, " ").trim();
        if (s) items.push(s.length > 140 ? `${s.slice(0, 137).trimEnd()}…` : s);
      } else if (!DROP.has(k.tag)) walk(k);
    }
  };
  walk(root);
  const all = plain(root).replace(/\s+/g, " ").trim();
  return { items, text: all.length > maxChars ? `${all.slice(0, maxChars - 1).trimEnd()}…` : all };
}

function render(n: Node | string, key: number): ReactNode {
  if (typeof n === "string") {
    const parts = n.split("\n");
    return parts.map((p, i) => (i ? [<br key={`${key}-${i}`} />, p] : p));
  }
  const kids = n.kids.map(render);
  switch (n.tag) {
    case "p": return <p key={key}>{kids}</p>;
    case "h1": case "h2": return <h4 key={key}>{kids}</h4>;
    case "h3": case "h4": return <h5 key={key}>{kids}</h5>;
    case "b": return <b key={key}>{kids}</b>;
    case "i": return <i key={key}>{kids}</i>;
    case "u": return <u key={key}>{kids}</u>;
    case "s": case "strike": return <s key={key}>{kids}</s>;
    case "list": return <ul key={key}>{kids}</ul>;
    case "olist": return <ol key={key}>{kids}</ol>;
    case "*": return <li key={key}>{kids}</li>;
    case "quote": return <blockquote key={key}>{kids}</blockquote>;
    case "code": case "noparse": return <code key={key}>{kids}</code>;
    case "hr": return <hr key={key} />;
    case "table": return <div key={key} className="bb-table"><table><tbody>{kids}</tbody></table></div>;
    case "tr": return <tr key={key}>{kids}</tr>;
    case "td": return <td key={key}>{kids}</td>;
    case "th": return <th key={key}>{kids}</th>;
    case "url": {
      const href = (n.arg ?? plain(n)).trim().replace(/^["']|["']$/g, "");
      if (!/^https:\/\//i.test(href)) return <span key={key}>{kids}</span>;
      return <button key={key} type="button" className="bb-link" onClick={() => openUrl(href).catch(() => {})}>{kids.length ? kids : href}</button>;
    }
    default: return DROP.has(n.tag) ? null : <span key={key}>{kids}</span>;
  }
}

export function BBCode({ src }: { src: string }) {
  return <div className="bbcode">{parseBBCode(src).kids.map(render)}</div>;
}
