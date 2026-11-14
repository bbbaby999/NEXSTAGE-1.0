import pkg from "../../package.json" with { type: "json" };

/** Versão exibida ao usuário. O número (semver) vem do package.json, única fonte da versão
 *  (o tauri.conf.json aponta para ele e o Rust o lê em build.rs). Desde a 1.1.0 não há mais selo "Beta". */
export const APP_VERSION: string = pkg.version;
export const VERSION_LABEL = APP_VERSION;
