fn main() {
    // Versão do app: única fonte é o package.json (o tauri.conf.json aponta para ele). Exposta ao Rust
    // como NX_VERSION; a versão do Cargo.toml é só do crate e não aparece para o usuário.
    println!("cargo:rerun-if-changed=../package.json");
    let pkg = std::fs::read_to_string("../package.json").expect("package.json");
    let v: serde_json::Value = serde_json::from_str(&pkg).expect("package.json inválido");
    let version = v["version"].as_str().expect("package.json sem \"version\"");
    println!("cargo:rustc-env=NX_VERSION={version}");
    tauri_build::build()
}
