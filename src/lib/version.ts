import pkg from "../../package.json" with { type: "json" };

/** Versão exibida ao usuário. O número técnico (semver) vem do package.json, única fonte da versão
 *  (o tauri.conf.json aponta para ele e o Rust o lê em build.rs). */
export const CHANNEL = "Beta";
export const APP_VERSION: string = pkg.version;
export const VERSION_LABEL = `${CHANNEL} ${APP_VERSION}`;
