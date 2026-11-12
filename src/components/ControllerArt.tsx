import { createContext, useContext, useId, type ReactNode } from "react";
import type { Visual } from "../lib/controllers";

/**
 * Ilustrações vetoriais dos controles (Central de Controles). Desenhos próprios e estilizados — sem
 * fotos nem logotipos —, todos na mesma tela 200×130 para que as proporções nunca distorçam.
 *
 * Com `live` (testador), o MESMO desenho do modelo mostra o estado real: botões acendem, analógicos
 * se movem e o touchpad/botão PS reagem — o testador corresponde ao controle cadastrado.
 */

/** Estado ao vivo: máscara de botões (bits = índices da Gamepad API; 16 = PS/Home, 17 = touchpad). */
export interface LiveState { mask: number; lx: number; ly: number; rx: number; ry: number; lt: number; rt: number }

export const BITS = {
  A: 1, B: 2, X: 4, Y: 8, LB: 16, RB: 32, LT: 64, RT: 128, SELECT: 256, START: 512, L3: 1024, R3: 2048,
  UP: 4096, DOWN: 8192, LEFT: 16384, RIGHT: 32768, HOME: 65536, TOUCH: 131072,
} as const;
type B = keyof typeof BITS;

const Ctx = createContext<{ g: string; live: LiveState | null }>({ g: "ca", live: null });
const useArt = () => useContext(Ctx);
function usePressed(b?: B) {
  const { live } = useArt();
  return !!(live && b && live.mask & BITS[b]);
}
const HOT = "var(--b, #22e4ff)";

/** Corpo: gradiente vertical, brilho superior (sheen), borda de luz e sombra inferior. */
function Body({ d, from, to, stroke = "rgba(255,255,255,.16)" }: { d: string; from: string; to: string; stroke?: string }) {
  const { g } = useArt();
  const id = `${g}-b${from.replace(/\W/g, "")}${to.replace(/\W/g, "")}`;
  return (
    <>
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor={from} />
          <stop offset="1" stopColor={to} />
        </linearGradient>
      </defs>
      <path d={d} fill={`url(#${id})`} />
      <path d={d} fill={`url(#${g}-sheen)`} />
      <path d={d} fill={`url(#${g}-under)`} />
      <path d={d} fill="none" stroke={stroke} strokeWidth="1" />
    </>
  );
}

/** Botão redondo com sombra, brilho e estado pressionado. */
function Btn({ x, y, r, fill, b, children }: { x: number; y: number; r: number; fill: string; b?: B; children?: ReactNode }) {
  const { g } = useArt();
  const on = usePressed(b);
  return (
    <g className={on ? "art-on" : undefined}>
      <circle cx={x} cy={y + 0.9} r={r + 0.4} fill="rgba(0,0,0,.45)" />
      <circle cx={x} cy={y} r={r} fill={on ? HOT : fill} />
      <circle cx={x} cy={y} r={r} fill={`url(#${g}-gloss)`} />
      <circle cx={x} cy={y} r={r - 0.4} fill="none" stroke="rgba(255,255,255,.18)" strokeWidth=".6" />
      {children}
    </g>
  );
}

/** Analógico: base escura, anel com textura de borracha e topo côncavo; acompanha o eixo ao vivo. */
function Stick({ x, y, r = 11, cap = "#2b2e35", ring = "#0b0c0f", side, b }: { x: number; y: number; r?: number; cap?: string; ring?: string; side?: "l" | "r"; b?: B }) {
  const { g, live } = useArt();
  const on = usePressed(b);
  const ax = live && side ? (side === "l" ? live.lx : live.rx) : 0;
  const ay = live && side ? (side === "l" ? live.ly : live.ry) : 0;
  const cx = x + ax * r * 0.42, cy = y + ay * r * 0.42;
  return (
    <g className={on ? "art-on" : undefined}>
      <circle cx={x} cy={y} r={r + 3} fill={ring} />
      <circle cx={x} cy={y} r={r + 3} fill="none" stroke="rgba(255,255,255,.06)" />
      <circle cx={cx} cy={cy + 1.2} r={r} fill="rgba(0,0,0,.55)" />
      <circle cx={cx} cy={cy} r={r} fill={on ? HOT : cap} />
      <circle cx={cx} cy={cy} r={r - 0.8} fill="none" stroke="rgba(255,255,255,.14)" strokeWidth="1.6" strokeDasharray="1 1.6" />
      <circle cx={cx} cy={cy} r={r * 0.66} fill={`url(#${g}-concave)`} />
      <circle cx={cx} cy={cy} r={r} fill={`url(#${g}-gloss)`} opacity=".7" />
    </g>
  );
}

/** Direcional em cruz com relevo, covinha central e braços que acendem. */
function Dpad({ x, y, s = 7, fill = "#1b1d22", well }: { x: number; y: number; s?: number; fill?: string; well?: string }) {
  const { g } = useArt();
  const up = usePressed("UP"), dn = usePressed("DOWN"), lf = usePressed("LEFT"), rt = usePressed("RIGHT");
  const a = s * 0.5, b = s * 1.5;
  const d = `M${x - a} ${y - b}h${s}v${s}h${s}v${s}h${-s}v${s}h${-s}v${-s}h${-s}v${-s}h${s}z`;
  const arm = (on: boolean, rx: number, ry: number, w: number, h: number) => on && <rect x={rx} y={ry} width={w} height={h} rx={1.2} fill={HOT} />;
  return (
    <g>
      {well && <circle cx={x} cy={y} r={s * 2.15} fill={well} />}
      <path d={d} fill="rgba(0,0,0,.5)" transform="translate(0 1)" />
      <path d={d} fill={fill} />
      {arm(up, x - a, y - b, s, s)}{arm(dn, x - a, y + a, s, s)}{arm(lf, x - b, y - a, s, s)}{arm(rt, x + a, y - a, s, s)}
      <path d={d} fill={`url(#${g}-gloss)`} opacity=".8" />
      <path d={d} fill="none" stroke="rgba(255,255,255,.14)" strokeWidth=".7" />
      <circle cx={x} cy={y} r={s * 0.28} fill="rgba(0,0,0,.35)" />
    </g>
  );
}

/** Quatro botões em losango (cima, direita, baixo, esquerda = Y, B, A, X na posição física). */
const FACE_BITS: B[] = ["Y", "B", "A", "X"];
function Face({ x, y, d = 12.5, r = 6.8, fills, marks, markColors, ps }: {
  x: number; y: number; d?: number; r?: number; fills: string[]; marks?: string[]; markColors?: string[]; ps?: boolean;
}) {
  const pos = [[x, y - d], [x + d, y], [x, y + d], [x - d, y]];
  return (
    <g>
      {pos.map(([cx, cy], i) => (
        <Btn key={i} x={cx} y={cy} r={r} fill={fills[i % fills.length]} b={FACE_BITS[i]}>
          {ps && <PsMark i={i} x={cx} y={cy} r={r * 0.5} c={markColors?.[i] ?? "#ddd"} />}
          {!ps && marks && (
            <text x={cx} y={cy + r * 0.4} textAnchor="middle" fontSize={r * 1.12} fontWeight="700" fill={markColors?.[i] ?? "rgba(255,255,255,.92)"}
              fontFamily="Segoe UI Variable Display, Segoe UI, sans-serif">{marks[i]}</text>
          )}
        </Btn>
      ))}
    </g>
  );
}

/** △ ○ ✕ □ desenhados em vetor (cima, direita, baixo, esquerda). */
function PsMark({ i, x, y, r, c }: { i: number; x: number; y: number; r: number; c: string }) {
  const sw = Math.max(1, r * 0.34);
  const common = { fill: "none", stroke: c, strokeWidth: sw, strokeLinecap: "round" as const, strokeLinejoin: "round" as const };
  if (i === 0) return <path d={`M${x} ${y - r * 0.95}L${x + r} ${y + r * 0.72}H${x - r}Z`} {...common} />;
  if (i === 1) return <circle cx={x} cy={y} r={r * 0.88} {...common} />;
  if (i === 2) return <path d={`M${x - r * 0.78} ${y - r * 0.78}L${x + r * 0.78} ${y + r * 0.78}M${x + r * 0.78} ${y - r * 0.78}L${x - r * 0.78} ${y + r * 0.78}`} {...common} />;
  return <rect x={x - r * 0.76} y={y - r * 0.76} width={r * 1.52} height={r * 1.52} rx={r * 0.12} {...common} />;
}

/** Botão pequeno em pílula (Select/Start/Share...). */
function Pill({ x, y, w = 9, h = 3.8, fill = "#111", rot = 0, b }: { x: number; y: number; w?: number; h?: number; fill?: string; rot?: number; b?: B }) {
  const on = usePressed(b);
  return (
    <g transform={rot ? `rotate(${rot} ${x} ${y})` : undefined}>
      <rect x={x - w / 2} y={y - h / 2 + 0.6} width={w} height={h} rx={h / 2} fill="rgba(0,0,0,.4)" />
      <rect x={x - w / 2} y={y - h / 2} width={w} height={h} rx={h / 2} fill={on ? HOT : fill} stroke="rgba(255,255,255,.12)" strokeWidth=".5" />
    </g>
  );
}

/** Ombro (LB/RB, L1/R1) em forma livre. */
function Bumper({ d, fill, b }: { d: string; fill: string; b: B }) {
  const on = usePressed(b);
  return <path d={d} fill={on ? HOT : fill} stroke="rgba(255,255,255,.12)" strokeWidth=".6" />;
}

/** Touchpad (DualShock 4 / DualSense): acende com o clique. */
function Touchpad({ x, y, w, h, fill, stroke = "rgba(255,255,255,.14)" }: { x: number; y: number; w: number; h: number; fill: string; stroke?: string }) {
  const { g } = useArt();
  const on = usePressed("TOUCH");
  return (
    <g>
      <rect x={x} y={y + 1} width={w} height={h} rx={6} fill="rgba(0,0,0,.35)" />
      <rect x={x} y={y} width={w} height={h} rx={6} fill={on ? HOT : fill} stroke={stroke} />
      <rect x={x} y={y} width={w} height={h} rx={6} fill={`url(#${g}-gloss)`} opacity=".5" />
    </g>
  );
}

// Silhueta "moderna" (Xbox / Pro Controller / genérico): asas com pegas longas.
const MODERN = "M58 24C42 24 31 31 25 45L11 92C7 108 15 121 29 121C39 121 45 115 51 106L61 92H139L149 106C155 115 161 121 171 121C185 121 193 108 189 92L175 45C169 31 158 24 142 24C129 24 122 30 100 30C78 30 71 24 58 24Z";
const MODERN_BUMPERS = ["M44 28C52 19 65 18 75 22L72 29C63 27 53 28 46 33Z", "M156 28C148 19 135 18 125 22L128 29C137 27 147 28 154 33Z"];
// Silhueta PlayStation (pegas mais finas e paralelas).
const PS = "M52 30C34 30 25 42 22 58L13 102C10 117 22 125 34 118L58 98H142L166 118C178 125 190 117 187 102L178 58C175 42 166 30 148 30H52Z";
const PS_SHOULDERS = ["M36 26a4 4 0 0 1 4-4h22a4 4 0 0 1 4 4v6H36Z", "M134 26a4 4 0 0 1 4-4h22a4 4 0 0 1 4 4v6h-30Z"];
const MODERN_PS = "M50 30C33 30 23 42 20 58L12 100C9 116 22 124 34 116L60 96H140L166 116C178 124 191 116 188 100L180 58C177 42 167 30 150 30Z";
const PS_COLORS = ["#4fd0a0", "#ef6a70", "#7ea6f2", "#e58bc9"];
const XB_COLORS = ["#f0c63c", "#e8494e", "#52c35f", "#3f92f2"];

function XboxSeries() {
  return (
    <g>
      <Bumper d={MODERN_BUMPERS[0]} fill="#22252b" b="LB" /><Bumper d={MODERN_BUMPERS[1]} fill="#22252b" b="RB" />
      <Body d={MODERN} from="#3d414a" to="#15171b" />
      <Stick x={62} y={52} side="l" b="L3" />
      <Dpad x={82} y={81} s={7.2} fill="#24272d" well="#101114" />
      <Stick x={122} y={81} side="r" b="R3" />
      <Face x={143} y={53} d={13.6} r={7.4} fills={["#1f2227"]} marks={["Y", "B", "A", "X"]} markColors={XB_COLORS} />
      <Btn x={100} y={41} r={7} fill="#262930" b="HOME">
        <circle cx={100} cy={41} r={3.4} fill="none" stroke="rgba(255,255,255,.85)" strokeWidth="1.3" />
      </Btn>
      <Btn x={88} y={57} r={2.6} fill="#121316" b="SELECT" /><Btn x={112} y={57} r={2.6} fill="#121316" b="START" />
    </g>
  );
}

function Xbox360() {
  return (
    <g>
      <Bumper d={MODERN_BUMPERS[0]} fill="#cfd2d8" b="LB" /><Bumper d={MODERN_BUMPERS[1]} fill="#cfd2d8" b="RB" />
      <Body d={MODERN} from="#fdfdfe" to="#c3c7ce" stroke="rgba(0,0,0,.16)" />
      <Stick x={62} y={52} cap="#3a3d44" ring="#a3a8b1" side="l" b="L3" />
      <Dpad x={82} y={81} s={7.2} fill="#a2a7b0" well="#dde0e5" />
      <Stick x={122} y={81} cap="#3a3d44" ring="#a3a8b1" side="r" b="R3" />
      <Face x={143} y={53} d={13.6} r={7.4} fills={["#e8b923", "#d6343a", "#3aa648", "#2d6fd1"]} marks={["Y", "B", "A", "X"]} />
      <circle cx={100} cy={45} r={9.5} fill="#b9bec6" stroke="#8d939c" />
      <Btn x={100} y={45} r={6.4} fill="#4fbf48" b="HOME">
        <path d="M100 40V50M95 45H105" stroke="#e1fbdc" strokeWidth="1.2" />
      </Btn>
      <Btn x={86} y={58} r={2.6} fill="#5a5f68" b="SELECT" /><Btn x={114} y={58} r={2.6} fill="#5a5f68" b="START" />
    </g>
  );
}

function Ps1() {
  return (
    <g>
      <Bumper d={PS_SHOULDERS[0]} fill="#a9abb1" b="LB" /><Bumper d={PS_SHOULDERS[1]} fill="#a9abb1" b="RB" />
      <Body d={PS} from="#dcdde1" to="#a6a8ae" stroke="rgba(0,0,0,.2)" />
      <circle cx={52} cy={60} r={18} fill="#c1c3c8" />
      <Dpad x={52} y={60} s={7.6} fill="#8f9298" />
      <circle cx={148} cy={60} r={23} fill="#c1c3c8" />
      <Face x={148} y={60} d={12.5} r={7} fills={["#8d9096"]} ps markColors={["#3fbf8a", "#e8535a", "#5a8fe6", "#e07ac2"]} />
      <Pill x={90} y={64} w={11} h={4.2} fill="#55585e" rot={-25} b="SELECT" /><Pill x={110} y={64} w={11} h={4.2} fill="#55585e" rot={-25} b="START" />
      <rect x="84" y="40" width="32" height="8" rx="2" fill="#c8cacf" />
    </g>
  );
}

function DualShockClassic({ ps3 }: { ps3?: boolean }) {
  return (
    <g>
      <Bumper d={PS_SHOULDERS[0]} fill="#26272c" b="LB" /><Bumper d={PS_SHOULDERS[1]} fill="#26272c" b="RB" />
      <Body d={PS} from={ps3 ? "#3d3f47" : "#303137"} to={ps3 ? "#0c0d10" : "#141518"} />
      <circle cx={52} cy={58} r={17} fill="#1b1c20" />
      <Dpad x={52} y={58} s={7.2} fill="#2f3137" />
      <Face x={148} y={58} d={12.5} r={6.8} fills={["#26282d"]} ps markColors={PS_COLORS} />
      <Stick x={78} y={87} r={10} cap="#25272c" ring="#08090b" side="l" b="L3" />
      <Stick x={122} y={87} r={10} cap="#25272c" ring="#08090b" side="r" b="R3" />
      <Pill x={88} y={58} w={9} h={3.8} fill="#08090b" b="SELECT" /><Pill x={112} y={58} w={9} h={3.8} fill="#08090b" b="START" />
      {ps3 ? (
        <Btn x={100} y={74} r={5.6} fill="#1d1e22" b="HOME"><circle cx={100} cy={74} r={2} fill="rgba(255,255,255,.55)" /></Btn>
      ) : (
        <g><Btn x={100} y={72} r={3.6} fill="#0b0b0d" b="HOME" /><circle cx={100} cy={64} r={1.6} fill="#ff3b3b" /></g>
      )}
    </g>
  );
}

function Ds4() {
  return (
    <g>
      <Bumper d="M56 32V27C62 21 78 21 86 26V32Z" fill="#1f2126" b="LB" /><Bumper d="M144 32V27C138 21 122 21 114 26V32Z" fill="#1f2126" b="RB" />
      <Body d="M50 30C33 30 23 42 20 58L12 100C9 116 22 124 34 116L60 96H140L166 116C178 124 191 116 188 100L180 58C177 42 167 30 150 30H124L120 36H80L76 30Z" from="#383c45" to="#13151a" />
      <Touchpad x={77} y={33} w={46} h={27} fill="#24272e" />
      <path d="M82 31.5H118" stroke="#4f8dff" strokeWidth="2.4" strokeLinecap="round" opacity=".95" />
      <Dpad x={51} y={58} s={7.2} fill="#2c2f36" well="#191b20" />
      <Face x={149} y={58} d={12.5} r={6.8} fills={["#22252b"]} ps markColors={PS_COLORS} />
      <Stick x={76} y={85} r={10} side="l" b="L3" /><Stick x={124} y={85} r={10} side="r" b="R3" />
      <Pill x={71} y={40} w={4} h={8.5} fill="#0d0e11" b="SELECT" /><Pill x={129} y={40} w={4} h={8.5} fill="#0d0e11" b="START" />
      <Btn x={100} y={78} r={4.8} fill="#1f2126" b="HOME"><circle cx={100} cy={78} r={1.6} fill="rgba(255,255,255,.5)" /></Btn>
    </g>
  );
}

function DualSense() {
  return (
    <g>
      <Bumper d="M56 32V27C62 21 78 21 86 26V32Z" fill="#eceef1" b="LB" /><Bumper d="M144 32V27C138 21 122 21 114 26V32Z" fill="#eceef1" b="RB" />
      <Body d={MODERN_PS} from="#ffffff" to="#d3d6dc" stroke="rgba(0,0,0,.16)" />
      <path d="M62 38C70 34 130 34 138 38L144 70C140 86 132 94 120 96H80C68 94 60 86 56 70Z" fill="#16171b" />
      <Touchpad x={77} y={33} w={46} h={26} fill="#f3f4f7" stroke="rgba(0,0,0,.18)" />
      <path d="M75 36C73 46 73 52 76 58M125 36C127 46 127 52 124 58" stroke="#4f8dff" strokeWidth="1.8" fill="none" strokeLinecap="round" />
      <Dpad x={47} y={58} s={6.8} fill="#d3d6dc" />
      <Face x={153} y={58} d={11.5} r={6.2} fills={["#dcdfe4"]} ps markColors={["#6f747d", "#6f747d", "#6f747d", "#6f747d"]} />
      <Stick x={78} y={81} r={10} cap="#24262b" ring="#0b0c0e" side="l" b="L3" />
      <Stick x={122} y={81} r={10} cap="#24262b" ring="#0b0c0e" side="r" b="R3" />
      <Pill x={70} y={38} w={3.4} h={7} fill="#c9ccd2" b="SELECT" /><Pill x={130} y={38} w={3.4} h={7} fill="#c9ccd2" b="START" />
      <Btn x={100} y={78} r={4.2} fill="#2a2c31" b="HOME"><circle cx={100} cy={78} r={1.4} fill="rgba(255,255,255,.55)" /></Btn>
      <Pill x={100} y={88} w={8} h={2.6} fill="#2a2c31" />
    </g>
  );
}

function JoyCon() {
  return (
    <g>
      <Bumper d="M88 14H76C66 14 59 18 55 24L88 24Z" fill="#0a8fbb" b="LB" />
      <Bumper d="M112 14H124C134 14 141 18 145 24L112 24Z" fill="#c22a14" b="RB" />
      <Body d="M88 16H76C60 16 52 28 52 44V96C52 112 60 122 76 122H88Z" from="#33d3f6" to="#0694c2" stroke="rgba(0,0,0,.2)" />
      <Body d="M112 16H124C140 16 148 28 148 44V96C148 112 140 122 124 122H112Z" from="#ff6d55" to="#d52c16" stroke="rgba(0,0,0,.2)" />
      <rect x="86" y="16" width="4" height="106" fill="#1b1d22" /><rect x="110" y="16" width="4" height="106" fill="#1b1d22" />
      <Stick x={70} y={45} r={9} side="l" b="L3" />
      <g>
        <Btn x={70} y={69} r={5} fill="#1f2126" b="UP" /><Btn x={80} y={79} r={5} fill="#1f2126" b="RIGHT" />
        <Btn x={70} y={89} r={5} fill="#1f2126" b="DOWN" /><Btn x={60} y={79} r={5} fill="#1f2126" b="LEFT" />
      </g>
      <Pill x={80} y={26} w={6} h={2.6} fill="#1f2126" b="SELECT" />
      <rect x="66.5" y="101" width="7" height="7" rx="1.5" fill="#1f2126" />
      <Face x={130} y={47} d={10} r={5} fills={["#1f2126"]} marks={["X", "A", "B", "Y"]} markColors={["#e2e2e2", "#e2e2e2", "#e2e2e2", "#e2e2e2"]} />
      <Stick x={130} y={84} r={9} side="r" b="R3" />
      <g className={undefined}><path d="M117 26h7M120.5 22.5v7" stroke="#1f2126" strokeWidth="2.2" strokeLinecap="round" /></g>
      <Btn x={124} y={104} r={3.8} fill="#2a2c31" b="HOME" />
    </g>
  );
}

function SwitchPro() {
  return (
    <g>
      <Bumper d={MODERN_BUMPERS[0]} fill="#202227" b="LB" /><Bumper d={MODERN_BUMPERS[1]} fill="#202227" b="RB" />
      <Body d={MODERN} from="#30333a" to="#101114" />
      <path d="M25 45L11 92C7 108 15 121 29 121C39 121 45 115 51 106L61 92M175 45L189 92C193 108 185 121 171 121C161 121 155 115 149 106L139 92" fill="none" stroke="rgba(170,180,200,.3)" strokeWidth="5" />
      <Stick x={62} y={52} cap="#1f2125" side="l" b="L3" />
      <Dpad x={82} y={81} s={6.4} fill="#282a30" />
      <Stick x={122} y={81} cap="#1f2125" side="r" b="R3" />
      <Face x={143} y={53} d={13.6} r={7.4} fills={["#25272c"]} marks={["X", "A", "B", "Y"]} markColors={["#d6dae0", "#d6dae0", "#d6dae0", "#d6dae0"]} />
      <Btn x={88} y={41} r={2.8} fill="#2b2d33" b="SELECT"><path d="M86.4 41h3.2" stroke="#aab0bb" strokeWidth="1" /></Btn>
      <Btn x={112} y={41} r={2.8} fill="#2b2d33" b="START"><path d="M110.4 41h3.2M112 39.4v3.2" stroke="#aab0bb" strokeWidth="1" /></Btn>
      <Btn x={110} y={56} r={3.6} fill="#2b2d33" b="HOME" />
      <rect x="86.5" y="52.5" width="6.5" height="6.5" rx="1.3" fill="#2b2d33" />
    </g>
  );
}

function Nes() {
  return (
    <g>
      <Body d="M22 40H178A6 6 0 0 1 184 46V96A6 6 0 0 1 178 102H22A6 6 0 0 1 16 96V46A6 6 0 0 1 22 40Z" from="#d8d8dc" to="#a7a7ae" stroke="rgba(0,0,0,.25)" />
      <rect x="22" y="47" width="156" height="49" rx="3" fill="#1c1c20" />
      <rect x="77" y="55" width="46" height="9" rx="1" fill="#c2c2c7" /><rect x="77" y="68" width="46" height="9" rx="1" fill="#c2c2c7" />
      <rect x="77" y="81" width="46" height="10" rx="2" fill="#c2c2c7" />
      <Pill x={92} y={86} w={10} h={4} fill="#2a2a2e" b="SELECT" /><Pill x={108} y={86} w={10} h={4} fill="#2a2a2e" b="START" />
      <Dpad x={50} y={72} s={8.2} fill="#2a2a2e" />
      <rect x="128" y="63" width="45" height="26" rx="3" fill="#c2c2c7" />
      <Btn x={140} y={76} r={8} fill="#d02a2a" b="A" /><Btn x={161} y={76} r={8} fill="#d02a2a" b="B" />
    </g>
  );
}

function Snes() {
  return (
    <g>
      <Bumper d="M30 42C38 30 56 26 68 29L64 34C54 32 42 35 35 44Z" fill="#9c9ca4" b="LB" />
      <Bumper d="M170 42C162 30 144 26 132 29L136 34C146 32 158 35 165 44Z" fill="#9c9ca4" b="RB" />
      <Body d="M58 30H142A40 40 0 0 1 142 110H58A40 40 0 0 1 58 30Z" from="#e4e4e8" to="#b0b0b7" stroke="rgba(0,0,0,.22)" />
      <circle cx={56} cy={70} r={23} fill="#cacacf" />
      <Dpad x={56} y={70} s={8.2} fill="#2a2a2e" />
      <circle cx={144} cy={70} r={28} fill="#cacacf" />
      <Face x={144} y={70} d={13.5} r={7.6} fills={["#3c6fd1", "#d3343a", "#e6b52b", "#3aa14d"]} marks={["X", "A", "B", "Y"]}
        markColors={["rgba(255,255,255,.9)", "rgba(255,255,255,.9)", "rgba(0,0,0,.55)", "rgba(255,255,255,.9)"]} />
      <Pill x={91} y={74} w={12} h={4.6} fill="#55555c" rot={-35} b="SELECT" /><Pill x={109} y={74} w={12} h={4.6} fill="#55555c" rot={-35} b="START" />
    </g>
  );
}

function N64() {
  return (
    <g>
      <Body d="M36 34C22 34 14 46 14 60V86C14 104 24 120 38 120C48 120 52 110 54 98L60 76H80L86 104C88 118 94 124 100 124C106 124 112 118 114 104L120 76H140L146 98C148 110 152 120 162 120C176 120 186 104 186 86V60C186 46 178 34 164 34Z" from="#a6a9b0" to="#696d75" stroke="rgba(0,0,0,.28)" />
      <Dpad x={46} y={60} s={7.6} fill="#2b2d33" />
      <Stick x={100} y={82} r={8} cap="#8f9299" ring="#3a3c42" side="l" b="L3" />
      <Btn x={100} y={52} r={6} fill="#d23a3a" b="START" />
      <Btn x={146} y={67} r={7.4} fill="#3f6fe0" b="A" />
      <Btn x={133} y={54} r={7.4} fill="#3aa64f" b="X" />
      <Face x={163} y={52} d={8.2} r={4.4} fills={["#f2c230"]} />
    </g>
  );
}

function Wii() {
  return (
    <g>
      <Body d="M86 8H114A14 14 0 0 1 128 22V110A14 14 0 0 1 114 124H86A14 14 0 0 1 72 110V22A14 14 0 0 1 86 8Z" from="#ffffff" to="#d4d7dd" stroke="rgba(0,0,0,.2)" />
      <circle cx={88} cy={18} r={3} fill="#c6c9cf" stroke="#9aa0a8" />
      <Dpad x={100} y={34} s={6.2} fill="#c9ccd2" />
      <Btn x={100} y={58} r={7.6} fill="#eceef1" b="A">
        <text x={100} y={61.2} textAnchor="middle" fontSize="8.5" fontWeight="700" fill="#8a9099" fontFamily="Segoe UI, sans-serif">A</text>
      </Btn>
      <Btn x={86} y={80} r={3.4} fill="#dcdfe4" b="SELECT" /><Btn x={100} y={80} r={3.8} fill="#dcdfe4" b="HOME" /><Btn x={114} y={80} r={3.4} fill="#dcdfe4" b="START" />
      <Btn x={100} y={96} r={4.8} fill="#eceef1" b="X" /><Btn x={100} y={108} r={4.8} fill="#eceef1" b="Y" />
      {[88, 96, 104, 112].map((x, i) => <rect key={x} x={x - 2} y={117} width="4" height="2" rx="1" fill={i === 0 ? "#4f9bff" : "#c6cad1"} />)}
    </g>
  );
}

function MegaDrive() {
  return (
    <g>
      <Body d="M42 36C22 36 12 56 14 76C16 98 34 112 58 108L100 98L142 108C166 112 184 98 186 76C188 56 178 36 158 36C140 36 128 44 100 44C72 44 60 36 42 36Z" from="#383940" to="#0e0f11" />
      <circle cx={56} cy={72} r={21} fill="#1b1c20" stroke="rgba(255,255,255,.08)" />
      <circle cx={56} cy={72} r={16.5} fill="#27282d" />
      <Dpad x={56} y={72} s={6.8} fill="#36383e" />
      <Pill x={100} y={62} w={14} h={4.8} fill="#1a1b1f" b="START" />
      <text x={100} y={72} textAnchor="middle" fontSize="4.6" fill="#c34" fontWeight="700" fontFamily="Segoe UI, sans-serif" letterSpacing=".1em">START</text>
      {([[128, 86, "A", "X"], [146, 78, "B", "A"], [164, 70, "C", "B"]] as [number, number, string, B][]).map(([x, y, l, b]) => (
        <g key={l}>
          <Btn x={x} y={y} r={8.2} fill="#24252a" b={b} />
          <text x={x} y={y + 15} textAnchor="middle" fontSize="5.5" fontWeight="700" fill="#9a9aa2" fontFamily="Segoe UI, sans-serif">{l}</text>
        </g>
      ))}
    </g>
  );
}

function SegaCd() {
  const top: B[] = ["LB", "Y", "RB"], bottom: B[] = ["X", "A", "B"];
  return (
    <g>
      <Body d="M48 36C26 36 16 54 18 74C20 96 36 108 58 104L100 96L142 104C164 108 180 96 182 74C184 54 174 36 152 36C136 36 126 42 100 42C74 42 64 36 48 36Z" from="#45464d" to="#18191d" />
      <circle cx={56} cy={70} r={18.5} fill="#25262b" />
      <Dpad x={56} y={70} s={6.8} fill="#3b3d44" />
      <Pill x={100} y={60} w={13} h={4.4} fill="#16171a" b="START" />
      <text x={100} y={52} textAnchor="middle" fontSize="4.6" fill="#8c8c95" fontFamily="Segoe UI, sans-serif" letterSpacing=".08em">MODE</text>
      <text x={100} y={70} textAnchor="middle" fontSize="4.6" fill="#8c8c95" fontFamily="Segoe UI, sans-serif" letterSpacing=".08em">START</text>
      {[124, 142, 160].map((x, i) => (
        <g key={x}>
          <Btn x={x} y={61 - i * 3} r={5.4} fill="#2e2f35" b={top[i]}>
            <text x={x} y={63 - i * 3} textAnchor="middle" fontSize="5.2" fontWeight="700" fill="#9c9ca5" fontFamily="Segoe UI, sans-serif">{"XYZ"[i]}</text>
          </Btn>
          <Btn x={x + 2} y={82 - i * 3} r={7.4} fill="#1a1b1f" b={bottom[i]}>
            <text x={x + 2} y={84.6 - i * 3} textAnchor="middle" fontSize="6.5" fontWeight="700" fill="#b0b0b9" fontFamily="Segoe UI, sans-serif">{"ABC"[i]}</text>
          </Btn>
        </g>
      ))}
    </g>
  );
}

function Generic() {
  return (
    <g>
      <Bumper d={MODERN_BUMPERS[0]} fill="#20253a" b="LB" /><Bumper d={MODERN_BUMPERS[1]} fill="#20253a" b="RB" />
      <Body d={MODERN} from="#333a50" to="#131724" stroke="rgba(120,160,255,.3)" />
      <Dpad x={64} y={58} s={7.2} fill="#252b3c" well="#191d2a" />
      <Face x={136} y={58} d={13.6} r={7.4} fills={["#2a3044"]} marks={["4", "2", "1", "3"]} markColors={["#b4bede", "#b4bede", "#b4bede", "#b4bede"]} />
      <Stick x={82} y={87} r={9.5} cap="#262b3b" side="l" b="L3" /><Stick x={118} y={87} r={9.5} cap="#262b3b" side="r" b="R3" />
      <Pill x={92} y={52} w={9} h={3.8} fill="#0d1018" b="SELECT" /><Pill x={108} y={52} w={9} h={3.8} fill="#0d1018" b="START" />
      <Btn x={100} y={66} r={3.6} fill="#1a1f2e" b="HOME" />
    </g>
  );
}

const ART: Record<Visual, () => JSX.Element> = {
  "xbox-series": XboxSeries,
  "xbox-360": Xbox360,
  ps1: Ps1,
  ps2: () => <DualShockClassic />,
  ps3: () => <DualShockClassic ps3 />,
  ps4: Ds4,
  ps5: DualSense,
  joycon: JoyCon,
  "switch-pro": SwitchPro,
  nes: Nes,
  snes: Snes,
  n64: N64,
  wii: Wii,
  megadrive: MegaDrive,
  segacd: SegaCd,
  generic: Generic,
};

/** Modelos com touchpad (o testador mostra o clique) e com analógicos (mostra os eixos). */
export const HAS_TOUCHPAD = (v: Visual) => v === "ps4" || v === "ps5";

/** Ilustração do controle; ocupa a largura disponível sem distorcer (proporção 20:13). */
export default function ControllerArt({ visual, className = "", title, live = null }: {
  visual: Visual; className?: string; title?: string; live?: LiveState | null;
}) {
  const g = `ca${useId().replace(/:/g, "")}`;
  const Art = ART[visual] ?? Generic;
  return (
    <svg className={`ctl-art ${className}`} viewBox="0 0 200 130" preserveAspectRatio="xMidYMid meet" role={title ? "img" : undefined} aria-label={title} aria-hidden={title ? undefined : true}>
      <defs>
        <radialGradient id={`${g}-gloss`} cx=".35" cy=".25" r=".75">
          <stop offset="0" stopColor="#fff" stopOpacity=".42" />
          <stop offset=".45" stopColor="#fff" stopOpacity=".08" />
          <stop offset="1" stopColor="#fff" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={`${g}-concave`} cx=".5" cy=".45" r=".6">
          <stop offset="0" stopColor="#000" stopOpacity=".35" />
          <stop offset=".75" stopColor="#000" stopOpacity=".05" />
          <stop offset="1" stopColor="#fff" stopOpacity=".12" />
        </radialGradient>
        <linearGradient id={`${g}-sheen`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#fff" stopOpacity=".16" />
          <stop offset=".35" stopColor="#fff" stopOpacity=".03" />
          <stop offset=".5" stopColor="#fff" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={`${g}-under`} x1="0" y1="0" x2="0" y2="1">
          <stop offset=".6" stopColor="#000" stopOpacity="0" />
          <stop offset="1" stopColor="#000" stopOpacity=".28" />
        </linearGradient>
      </defs>
      <ellipse cx="100" cy="125" rx="72" ry="4.5" fill="rgba(0,0,0,.5)" />
      <Ctx.Provider value={{ g, live }}>
        <Art />
      </Ctx.Provider>
    </svg>
  );
}
