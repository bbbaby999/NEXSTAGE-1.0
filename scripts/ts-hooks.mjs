// Testes em Node (scripts/test-*.mjs): completa a extensão .ts/.tsx de importações relativas sem
// extensão ("./i18n"), como o Vite faz no app. Só afeta os testes; o app não usa este arquivo.
export async function resolve(specifier, context, next) {
  try {
    return await next(specifier, context);
  } catch (err) {
    const relative = specifier.startsWith("./") || specifier.startsWith("../");
    if (err?.code !== "ERR_MODULE_NOT_FOUND" || !relative || /\.[cm]?[jt]sx?$/.test(specifier)) throw err;
    for (const ext of [".ts", ".tsx"]) {
      try {
        return await next(specifier + ext, context);
      } catch {
        // tenta a próxima extensão
      }
    }
    throw err;
  }
}
