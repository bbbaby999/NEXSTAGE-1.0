//! Integração com o RetroAchievements (API Web oficial).
//!
//! - `client`: serviço único de requisições (limites, validação, erros sem expor a chave)
//! - `secret`: chave da API cifrada com a DPAPI do Windows
//! - `store`: cache SQLite (offline) e detecção de novos desbloqueios
//! - `hash`/`md5`: identificação dos jogos pelo hash oficial (algoritmo do rcheevos)
//! - `link`: associação biblioteca ↔ RetroAchievements (automática só por hash; resto com confirmação)
//! - `sync`: sincronização + notificações
//! - `watch`: sincronização automática e gatilho pelos logs dos emuladores compatíveis
//! Os comandos expostos à interface ficam em `commands::retro`.

pub mod client;
pub mod hash;
pub mod link;
pub mod md5;
pub mod secret;
pub mod store;
pub mod sync;
pub mod util;
pub mod watch;
