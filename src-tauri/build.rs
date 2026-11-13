fn main() {
    // Versão do app: única fonte é o package.json (o tauri.conf.json aponta para ele). Exposta ao Rust
    // como NX_VERSION; a versão do Cargo.toml é só do crate e não aparece para o usuário.
    println!("cargo:rerun-if-changed=../package.json");
    let pkg = std::fs::read_to_string("../package.json").expect("package.json");
    let v: serde_json::Value = serde_json::from_str(&pkg).expect("package.json inválido");
    let version = v["version"].as_str().expect("package.json sem \"version\"");
    println!("cargo:rustc-env=NX_VERSION={version}");
    // Feedback: endereço do serviço intermediário (não é segredo; o token do GitHub fica só nele).
    // Configure com scripts/setup-feedback.mjs. Sem o arquivo, o envio aparece como "não configurado".
    println!("cargo:rerun-if-changed=feedback.json");
    let endpoint = std::fs::read_to_string("feedback.json")
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["endpoint"].as_str().map(str::to_string))
        .filter(|u| u.starts_with("https://"))
        .unwrap_or_default();
    println!("cargo:rustc-env=NX_FEEDBACK_URL={endpoint}");
    tauri_build::build()
}
