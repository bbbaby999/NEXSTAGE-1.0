// Registra scripts/ts-hooks.mjs: node --import ./scripts/ts-register.mjs scripts/test-xxx.mjs
import { register } from "node:module";
register("./ts-hooks.mjs", import.meta.url);
