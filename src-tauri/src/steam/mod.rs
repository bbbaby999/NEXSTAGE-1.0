//! Integração com a Steam (APIs oficiais + cliente Steam local).
//!
//! Princípios (ver também `launcher.rs`):
//! - login só pelo "Entrar com a Steam" oficial (OpenID): o NEXSTAGE nunca pede a senha e não guarda
//!   cookies, tokens ou credenciais do cliente;
//! - sem chave compartilhada embutida: a chave PESSOAL da Web API (opcional) fica no cofre do Windows;
//! - instalar e jogar SEMPRE pelo cliente oficial (`steam://install/<appid>`, `steam://run/<appid>`):
//!   a Steam continua responsável por download, atualização, verificação, autenticação e execução —
//!   e o overlay da Steam e os anti-cheats funcionam como se o jogo fosse aberto pela própria Steam;
//! - nenhum acesso a processos, memória, arquivos ou DLLs dos jogos.
//!
//! - `openid`: login oficial · `client`: Web API e perfil público · `local`: cliente instalado (leitura)
//! - `vdf`: leitor de arquivos da Steam · `store`: cache e importação · `sync`: sincronização
//! - `watch`: aviso de conquista quase em tempo real
//! - `news`: notas de atualização oficiais dos jogos (seção "Últimas atualizações" da Home)

pub mod client;
pub mod local;
pub mod news;
pub mod openid;
pub mod store;
pub mod sync;
pub mod vdf;
pub mod watch;
