//! Inicialização de jogos por executável.
//!
//! O processo é iniciado sem bloquear a UI. Uma thread aguarda o término,
//! registra a sessão em `play_sessions`, soma o tempo em `profile_games` e
//! emite o evento `game-session-ended` para o frontend atualizar a biblioteca.
//!
//! Limitação conhecida: jogos cujo executável é apenas um "stub" que abre outro
//! processo e encerra (alguns launchers) terão tempo de jogo subestimado.
//!
//! Jogos integrados ao NexStage (`source = 'nexstage'`): antes de iniciar, o Game Bridge abre uma sessão
//! local e passa as credenciais ao processo do jogo por variáveis de ambiente (ver `bridge`). Sem sessão
//! (falha ou jogo comum), o jogo abre normalmente.
//!
//! Jogos da Steam (importados ou vinculados a um AppID e instalados) são SEMPRE iniciados pelo cliente
//! oficial (`steam://run/<appid>`): a Steam autentica, atualiza e executa, e o overlay da Steam e os
//! anti-cheats funcionam normalmente. O NEXSTAGE não abre, monitora, injeta nem encerra esses processos
//! (por isso não há sessão local cronometrada para eles; o tempo vem da Steam como dado externo).

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::overlay::{self, Runtime};
use rusqlite::params;
use std::{path::Path, process::Command, time::Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// Divide argumentos no estilo Windows: separados por espaço, aspas duplas agrupam.
/// Barras invertidas são mantidas literalmente (caminhos como `C:\Jogos\rom.iso`).
fn split_args(s: &str) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => cur.push(c),
        }
    }
    if in_quotes {
        return Err(AppError::Validation(
            "Argumentos inválidos: aspas não fechadas.".into(),
        ));
    }
    if has_token || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// Substitui `{iso}` pelo caminho do ISO em cada argumento (depois de separar, então espaços no caminho são seguros).
fn expand_args(args: Vec<String>, file: Option<&str>) -> Vec<String> {
    match file {
        Some(p) => args.into_iter().map(|a| a.replace("{iso}", p).replace("{rom}", p)).collect(),
        None => args,
    }
}

/// Tudo o que é preciso para iniciar o jogo, já validado (executável, argumentos, pasta).
struct Prepared {
    exe: std::path::PathBuf,
    args: Vec<String>,
    /// Emulador reconhecido + opção de tela cheia: ajustes de configuração feitos só ao iniciar (RPCS3).
    fullscreen: Option<(crate::emu_fullscreen::Kind, bool)>,
}

/// Valida o jogo (executável/emulador/arquivo existentes, argumentos) SEM iniciar nada.
fn prepare(db: &Db, profile_id: i64, game_id: i64) -> AppResult<Prepared> {
    let (mut exe, mut args, mut iso, emu_exe, emu_args, emu_name, emu_full) = {
        let conn = db.conn()?;
        let found = conn
            .query_row(
                "SELECT g.executable_path, g.launch_args, COALESCE(g.iso_path, g.rom_path), e.exe_path, e.args, e.name, e.fullscreen
                 FROM profile_games pg JOIN games g ON g.id = pg.game_id
                 LEFT JOIN emulators e ON e.id = g.emulator_id
                 WHERE pg.profile_id=?1 AND pg.game_id=?2",
                params![profile_id, game_id],
                |r| Ok((
                    r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?, r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?, r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                )),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    AppError::NotFound("Jogo não encontrado na biblioteca.".into())
                }
                other => other.into(),
            })?;
        found
    };

    // Emulador associado (cadastrado em Emuladores): usa o executável dele; os argumentos do jogo, se
    // existirem, têm prioridade sobre o modelo do emulador. Sem emulador, vale o sistema anterior.
    let mut fullscreen = None;
    if let Some(ee) = emu_exe {
        fullscreen = crate::emu_fullscreen::detect(emu_name.as_deref().unwrap_or(""), &ee).map(|k| (k, emu_full));
        if !Path::new(&ee).is_file() {
            return Err(AppError::NotFound(format!("O emulador associado não foi encontrado: {ee}. Atualize o caminho na página Emuladores.")));
        }
        // Jogo sem ISO/ROM (ex.: adicionado por executável, EBOOT.BIN, .xex): o arquivo cadastrado no jogo
        // é o que o emulador recebe em {iso}/{rom}.
        if iso.is_none() && !exe.trim().is_empty() && !Path::new(&exe).eq(Path::new(&ee)) {
            iso = Some(std::mem::take(&mut exe));
        }
        exe = ee;
        if args.trim().is_empty() {
            args = emu_args.unwrap_or_default();
        }
    }
    // Pacotes PKG (PS3) e VPK (Vita) precisam ser INSTALADOS no emulador; não são iniciados por caminho.
    if let Some(f) = &iso {
        let ext = Path::new(f).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if ext == "pkg" || ext == "vpk" {
            return Err(AppError::Validation(format!(
                "Arquivos .{ext} precisam ser instalados no emulador (menu de instalação de pacotes); depois adicione o jogo instalado."
            )));
        }
    }
    // Arquivo importado (ISO/ROM): nunca é montado nem executado automaticamente; precisa de emulador/comando.
    if exe.trim().is_empty() {
        return Err(if iso.is_some() {
            AppError::Validation(
                "Este jogo foi importado, mas ainda não tem emulador associado. Associe um emulador em \"Editar jogo\" (ou cadastre-o na página Emuladores) ou informe o executável e os argumentos (use {iso} ou {rom} para o caminho do arquivo).".into(),
            )
        } else {
            AppError::NotFound("Executável não configurado para este jogo.".into())
        });
    }
    if let Some(i) = &iso {
        if !Path::new(i).is_file() {
            return Err(AppError::NotFound(format!("O arquivo do jogo não foi encontrado: {i}.")));
        }
    }
    let path = Path::new(&exe);
    if !path.is_file() {
        return Err(AppError::NotFound(format!(
            "Executável não encontrado: {exe}. Edite o jogo e selecione o arquivo novamente."
        )));
    }
    let mut list = split_args(&args)?;
    // Opção "Iniciar jogos em tela cheia" do emulador: vale para todos os jogos associados a ele.
    if let Some((kind, on)) = fullscreen {
        list = crate::emu_fullscreen::apply_args(kind, on, list);
    }
    Ok(Prepared { exe: path.to_path_buf(), args: expand_args(list, iso.as_deref()), fullscreen })
}

/// Como o jogo será iniciado.
enum Launch {
    Exe(Prepared),
    /// Pelo cliente Steam (AppID).
    Steam(i64),
    /// Jogo da Microsoft Store / Xbox (AUMID), iniciado pelo Explorer (ver `msstore`).
    App(String),
}

/// Decide entre o cliente Steam e o executável/emulador, validando SEM iniciar nada.
fn plan(db: &Db, profile_id: i64, game_id: i64) -> AppResult<Launch> {
    let (appid, steam_only, store_app) = {
        let conn = db.conn()?;
        super::games::ensure_owned(&conn, profile_id, game_id)?;
        // jogo integrado ainda não instalado (ou pasta movida/apagada) neste PC
        let exe_now: String = conn.query_row("SELECT executable_path FROM games WHERE id=?1", [game_id], |r| r.get(0)).unwrap_or_default();
        if crate::integrated::library_source_is_integrated(&conn, game_id) && (exe_now.trim().is_empty() || !Path::new(&exe_now).is_file()) {
            return Err(AppError::NotFound("Este jogo integrado não está instalado neste PC. Use Instalar ou Localizar instalação na página do jogo.".into()));
        }
        let only: bool = conn
            .query_row("SELECT source='steam' AND trim(executable_path)='' FROM games WHERE id=?1", [game_id], |r| r.get(0))
            .unwrap_or(false);
        // Jogo da Microsoft Store / Xbox sem emulador: `shell:AppsFolder\<AUMID>` no lugar do executável.
        let store_app = conn
            .query_row("SELECT executable_path FROM games WHERE id=?1 AND emulator_id IS NULL", [game_id], |r| r.get::<_, String>(0))
            .ok()
            .and_then(|p| crate::msstore::aumid(&p).map(String::from));
        (crate::steam::store::appid_of(&conn, game_id)?, only, store_app)
    };
    if let Some(a) = store_app {
        if crate::msstore::package_root(&a).is_none() {
            return Err(AppError::NotFound("Este jogo da Microsoft Store / Xbox não está instalado neste PC. Instale-o pelo app Xbox.".into()));
        }
        // adicionado pelo catálogo do Game Pass antes de instalar: completa o AUMID pelo pacote instalado
        let a = crate::msstore::resolve_aumid(&a)
            .ok_or_else(|| AppError::NotFound("O jogo está instalado, mas o Windows ainda não registrou como iniciá-lo. Abra-o uma vez pelo app Xbox.".into()))?;
        return Ok(Launch::App(a));
    }
    if let Some(a) = appid {
        match crate::steam::local::app(a) {
            Some(x) if x.installed => {
                if !crate::steam::local::protocol_available() {
                    return Err(AppError::Validation("O protocolo steam:// não está registrado neste Windows. Abra ou reinstale o cliente Steam.".into()));
                }
                return Ok(Launch::Steam(a));
            }
            Some(_) if steam_only => {
                return Err(AppError::Validation("Este jogo ainda está sendo instalado ou atualizado pela Steam. Acompanhe o download no cliente Steam.".into()))
            }
            None if steam_only => {
                return Err(AppError::Validation("Este jogo da Steam não está instalado neste PC. Use o botão Instalar (o download é feito pelo cliente Steam).".into()))
            }
            _ => {} // vinculado, mas não instalado pela Steam: usa o executável cadastrado
        }
    }
    prepare(db, profile_id, game_id).map(Launch::Exe)
}

/// Confere se o jogo pode ser iniciado (usado antes do Gameboot: um erro aparece na hora, sem
/// tocar a animação inteira para depois falhar).
#[tauri::command]
pub fn check_launch(db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<()> {
    plan(&db, profile_id, game_id).map(|_| ())
}

/// Gameboot: `on` traz o NEXSTAGE para a frente, em tela cheia e acima das outras janelas, durante a
/// animação; `off` desfaz (usado só se o jogo não chegar a iniciar — ao iniciar, `launch_game` já
/// devolve a janela ao modo normal e a minimiza).
#[tauri::command]
pub fn gameboot_window(app: AppHandle, on: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WAS_FULLSCREEN: AtomicBool = AtomicBool::new(false);
    let Some(w) = app.get_webview_window("main") else { return };
    if on {
        WAS_FULLSCREEN.store(w.is_fullscreen().unwrap_or(false), Ordering::SeqCst);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_always_on_top(true);
        let _ = w.set_fullscreen(true);
        let _ = w.set_focus();
    } else {
        let _ = w.set_always_on_top(false);
        let _ = w.set_fullscreen(WAS_FULLSCREEN.load(Ordering::SeqCst));
        let _ = w.set_focus();
    }
}

#[tauri::command]
pub fn launch_game(app: AppHandle, db: State<Db>, profile_id: i64, game_id: i64) -> AppResult<()> {
    let Prepared { exe, args, fullscreen } = match plan(&db, profile_id, game_id)? {
        Launch::Exe(p) => p,
        Launch::Steam(appid) => return launch_via_steam(&app, &db, profile_id, game_id, appid),
        Launch::App(aumid) => return launch_store_app(&app, &db, profile_id, game_id, aumid),
    };
    let path = exe.as_path();
    if let Some((kind, on)) = fullscreen {
        crate::emu_fullscreen::apply_config(kind, on, path);
    }
    let mut cmd = Command::new(path);
    cmd.args(args);
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    // jogo integrado: sessão do Game Bridge (variáveis de ambiente para o processo do jogo)
    let bridge = crate::bridge::for_game(&app, profile_id, game_id);
    if let Some(b) = &bridge {
        cmd.envs(b.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            if let Some(b) = &bridge {
                crate::bridge::end(&app, &b.session);
            }
            return Err(AppError::Other(format!("Falha ao iniciar o jogo: {e}")));
        }
    };
    let started = Instant::now();
    // Só marca como "iniciado" quando o processo realmente subiu (define a ordem da Home).
    if let Ok(conn) = db.conn() {
        let _ = conn.execute(
            "UPDATE profile_games SET last_played_at=strftime('%Y-%m-%d %H:%M:%f','now') WHERE profile_id=?1 AND game_id=?2",
            params![profile_id, game_id],
        );
    }

    // Registra o processo (para o overlay poder encerrá-lo) e minimiza o launcher.
    app.state::<Runtime>().add(game_id, child.id());
    app.state::<Runtime>().last_game.store(game_id, std::sync::atomic::Ordering::SeqCst);
    overlay::set_active(&app, false);

    std::thread::spawn(move || {
        let _ = child.wait();
        end_session(&app, profile_id, game_id, started.elapsed().as_secs() as i64);
        if let Some(b) = bridge {
            crate::bridge::end(&app, &b.session);
        }
    });

    Ok(())
}

/// Registra a sessão encerrada (tempo de jogo), traz o NEXSTAGE de volta se era o último jogo aberto e
/// avisa a interface.
fn end_session(app: &AppHandle, profile_id: i64, game_id: i64, secs: i64) {
    let db = app.state::<Db>();
    if let Ok(conn) = db.conn() {
        let offset = format!("-{secs} seconds");
        let _ = conn.execute(
            "INSERT INTO play_sessions(profile_id,game_id,started_at,ended_at,duration_seconds)
             VALUES(?1,?2,datetime('now',?3),datetime('now'),?4)",
            params![profile_id, game_id, offset, secs],
        );
        let _ = conn.execute(
            "UPDATE profile_games SET playtime_seconds=playtime_seconds+?1
             WHERE profile_id=?2 AND game_id=?3",
            params![secs, profile_id, game_id],
        );
    }
    overlay::game_finished(app, game_id);
    let _ = app.emit("game-session-ended", game_id);
}

/// Jogo da Microsoft Store / Xbox: o Explorer inicia o app (o .exe não pode ser aberto direto). O processo
/// do jogo é reconhecido pela pasta do pacote; enquanto houver algum aberto lá, a sessão continua (cobre
/// o `gamelaunchhelper.exe` que abre o jogo de verdade e encerra). Sem processo em 2 min, nada é registrado.
fn launch_store_app(app: &AppHandle, db: &Db, profile_id: i64, game_id: i64, aumid: String) -> AppResult<()> {
    use std::time::Duration;
    let roots = crate::msstore::process_roots(&aumid);
    crate::msstore::launch(&aumid).map_err(|e| AppError::Other(format!("Falha ao iniciar o jogo: {e}")))?;
    if let Ok(conn) = db.conn() {
        let _ = conn.execute(
            "UPDATE profile_games SET last_played_at=strftime('%Y-%m-%d %H:%M:%f','now') WHERE profile_id=?1 AND game_id=?2",
            params![profile_id, game_id],
        );
    }
    app.state::<Runtime>().last_game.store(game_id, std::sync::atomic::Ordering::SeqCst);
    overlay::set_active(app, false); // desfaz o modo do Gameboot
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let wait_start = Instant::now();
        let mut started: Option<Instant> = None;
        let mut gone_since: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let pids = crate::msstore::pids_under(&roots);
            match (pids.last(), started) {
                (Some(&pid), _) => {
                    started.get_or_insert_with(Instant::now);
                    gone_since = None;
                    app.state::<Runtime>().add(game_id, pid); // o mais recente: o jogo, não o lançador
                }
                (None, None) if wait_start.elapsed() > Duration::from_secs(120) => return,
                (None, None) => {}
                // alguns segundos sem processo (troca lançador → jogo) antes de dar a sessão por encerrada
                (None, Some(t0)) => {
                    let g = *gone_since.get_or_insert_with(Instant::now);
                    if g.elapsed() >= Duration::from_secs(6) {
                        end_session(&app, profile_id, game_id, g.duration_since(t0).as_secs() as i64);
                        return;
                    }
                }
            }
        }
    });
    Ok(())
}

/// Entrega o jogo ao cliente Steam e só registra "jogado por último" (ordem da Home). Nenhum processo
/// é acompanhado ou tocado pelo NEXSTAGE.
fn launch_via_steam(app: &AppHandle, db: &Db, profile_id: i64, game_id: i64, appid: i64) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(format!("steam://run/{appid}"), None::<&str>)
        .map_err(|_| AppError::Other("Não foi possível pedir à Steam para iniciar o jogo.".into()))?;
    if let Ok(conn) = db.conn() {
        let _ = conn.execute(
            "UPDATE profile_games SET last_played_at=strftime('%Y-%m-%d %H:%M:%f','now') WHERE profile_id=?1 AND game_id=?2",
            params![profile_id, game_id],
        );
    }
    app.state::<Runtime>().last_game.store(game_id, std::sync::atomic::Ordering::SeqCst);
    overlay::set_active(app, false); // desfaz o modo do Gameboot
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::split_args;

    #[test]
    fn splits_quoted_windows_paths() {
        let v = split_args(r#"-fullscreen "D:\Roms\Meu Jogo.iso" --x"#).unwrap();
        assert_eq!(v, vec!["-fullscreen", r"D:\Roms\Meu Jogo.iso", "--x"]);
    }

    #[test]
    fn expands_iso_placeholder_keeping_spaces() {
        let args = split_args(r#"-fullscreen "{iso}""#).unwrap();
        let out = super::expand_args(args, Some(r"D:\Jogos\Meu Jogo.iso"));
        assert_eq!(out, vec!["-fullscreen", r"D:\Jogos\Meu Jogo.iso"]);
    }

    #[test]
    fn lichii_template_keeps_options_before_game_path() {
        // Modelo do Lichii: opções primeiro; o caminho vai inteiro como valor de -g (espaços, colchetes, acentos).
        let rom = r"E:\Nova pasta\Jogos Ação\Zelda [0400486024DEC000] (v1.0).xci";
        let out = super::expand_args(split_args(r#"-f -g "{rom}""#).unwrap(), Some(rom));
        assert_eq!(out, vec!["-f", "-g", rom]);
        let out = super::expand_args(split_args(r#"-g "{rom}""#).unwrap(), Some(rom));
        assert_eq!(out, vec!["-g", rom]);
    }

    #[test]
    fn rejects_unclosed_quote() {
        assert!(split_args(r#"-a "b"#).is_err());
    }
}
