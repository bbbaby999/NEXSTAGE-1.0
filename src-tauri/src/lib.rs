//! NEXSTAGE - backend Tauri.
//!
//! Módulos:
//! - `db`: conexão, schema e migrações SQLite
//! - `error`: erro unificado
//! - `input`: leitura nativa de controles (XInput / HID) -> eventos para a interface
//! - `overlay`: modo overlay, atalho global configurável e jogos em execução
//! - `commands::*`: comandos expostos ao frontend

mod commands;
mod detect;
mod db;
mod error;
mod input;
mod overlay;
mod screenshot;
mod shot_toast;
mod window_bounds;
mod pad_led;
mod hidpads;
mod devident;
mod images;
mod retro;
mod steam;
mod sources;
mod accounts;
mod backup;
mod net;
mod vault;
mod apikeys;
mod pad_notify;
#[cfg(debug_assertions)]
mod dev;

use commands::{activity, backup as bk, collections, controllers, emulators, scan, gallery, games, info, launcher, profiles, retro as ra, steam as st, settings, startup, stats, storage, system, webshots};
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    tauri::Builder::default()
        // Precisa ser o primeiro plugin: abrir o NEXSTAGE de novo (atalho, início com o Windows) só traz
        // a janela já aberta para a frente, em vez de criar uma segunda cópia disputando o banco e os controles.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("nexstage.db"))?;
            // Capas e fundos em arquivos (ver `images`): migra as que ainda estão no banco e limpa as sem uso.
            images::init(&dir)?;
            // Sobras de um backup/restauração interrompidos (nenhuma operação roda antes da abertura).
            let _ = std::fs::remove_dir_all(dir.join("backup-work"));
            let _ = std::fs::remove_dir_all(dir.join("restore-staging"));
            if let Ok(mut conn) = db.conn() {
                if let Ok(n) = images::migrate_and_clean(&mut conn) {
                    if n > 0 {
                        // devolve ao disco o espaço que as imagens ocupavam dentro do banco
                        let _ = conn.execute_batch("VACUUM;");
                    }
                }
            }
            // Perfis protegidos (carregados uma vez; o desbloqueio vive só na memória desta sessão).
            app.manage(db);
            app.manage(overlay::Runtime::default());
            app.manage(scan::ScanState::default());

            // Atalho global do overlay (configurável em Configurações; padrão Ctrl+Shift+O).
            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            #[cfg(debug_assertions)]
                            if dev::handle(app, shortcut) {
                                return;
                            }
                            if overlay::is_shot_shortcut(app, shortcut) {
                                let a = app.clone();
                                std::thread::spawn(move || { let _ = screenshot::take(&a); });
                            } else {
                                overlay::toggle(app);
                            }
                        }
                    })
                    .build(),
            )?;
            // Atualizações pelo app: o plugin só é ativado depois que `plugins.updater` existe no
            // tauri.conf.json (feito por `node scripts/setup-updater.mjs`). Sem isso, o app roda normal.
            if app.config().plugins.0.contains_key("updater") {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            app.handle().plugin(tauri_plugin_autostart::init(
                tauri_plugin_autostart::MacosLauncher::LaunchAgent,
                None,
            ))?;
            overlay::register_saved(app.handle());
            #[cfg(debug_assertions)]
            dev::register(app.handle());

            // Leitura nativa dos controles (Xbox/XInput e PlayStation/HID), desde a abertura do app,
            // independente de perfil.
            input::spawn(app.handle().clone());
            // Windows.Gaming.Input preenche a lista de controles de forma assíncrona: começa já, para a
            // bateria do Xbox estar disponível na primeira consulta.
            #[cfg(windows)]
            std::thread::spawn(controllers::wgi::warm_up);
            // RetroAchievements: sincronização automática e gatilho pelos logs dos emuladores (sem rede
            // na inicialização: a primeira verificação espera 60 s e só ocorre com conta conectada).
            retro::watch::spawn(app.handle().clone());
            // Steam: sincronização automática (primeira verificação após 90 s; só com conta conectada).
            steam::sync::spawn(app.handle().clone());
            // Steam: aviso de conquista quase em tempo real (cartão por cima do jogo).
            steam::watch::spawn(app.handle().clone());
            // Janela da notificação de captura (oculta até a primeira captura).
            shot_toast::create(app.handle());
            // Avisos de controle conectado/desconectado e bateria baixa (mesma janela de notificação).
            pad_notify::spawn(app.handle().clone());
            // Backup automático semanal (verificação 2 min após abrir e depois a cada 30 min).
            backup::auto::spawn(app.handle().clone());
            // Por último: aplica minimizado/janela/tela cheia e exibe a janela.
            startup::apply_start_mode(app.handle());
            Ok(())
        })
        .invoke_handler({
            let handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
            profiles::list_profiles,
            profiles::create_profile,
            profiles::update_profile,
            profiles::delete_profile,
            games::list_library,
            games::add_game,
            games::update_game,
            games::remove_game,
            games::set_favorite,
            games::set_completed,
            emulators::list_emulators,
            emulators::save_emulator,
            emulators::delete_emulator,
            emulators::set_game_emulator,
            scan::scan_folder,
            scan::cancel_scan,
            scan::import_scan_items,
            scan::list_library_folders,
            scan::remove_library_folder,
            activity::profile_activity,
            gallery::delete_gallery_items,
            gallery::reveal_capture,
            info::fandom_link,
            startup::get_startup_info,
            startup::set_startup_mode,
            launcher::launch_game,
            launcher::check_launch,
            launcher::gameboot_window,
            stats::gamer_stats,
            stats::get_stats_visibility,
            stats::set_stats_visibility,
            stats::public_gamer_stats,
            settings::get_app_setting,
            settings::set_app_setting,
            settings::get_profile_setting,
            settings::set_profile_setting,
            settings::set_fullscreen,
            settings::get_home_background,
            settings::set_home_background,
            settings::set_home_background_from_capture,
            settings::installer_language,
            overlay::close_overlay,
            overlay::running_games,
            overlay::quit_game,
            overlay::get_overlay_shortcut,
            overlay::set_overlay_shortcut,
            overlay::reset_overlay_shortcut,
            overlay::get_screenshot_shortcut,
            overlay::set_screenshot_shortcut,
            overlay::reset_screenshot_shortcut,
            overlay::set_active_profile,
            input::get_pad_combos,
            input::set_pad_combo,
            input::reset_pad_combo,
            input::set_pad_capture,
            input::set_pad_led,
            input::set_pad_configs,
            hidpads::set_joycon_pairs,
            pad_notify::set_pad_names,
            input::set_pad_monitor,
            input::set_pad_button_repeat,
            input::pad_rumble,
            screenshot::take_screenshot,
            screenshot::captures_folder,
            screenshot::open_captures_folder,
            info::get_game_info,
            info::fetch_game_info,
            info::steam_store_search,
            info::apply_steam_cover,
            info::save_manual_info,
            info::auto_classify,
            info::save_manual_hltb,
            commands::metacritic::fetch_metacritic,
            commands::metacritic::save_manual_metacritic,
            sources::sources_list,
            sources::source_set_enabled,
            sources::source_connect,
            sources::source_test,
            sources::source_disconnect,
            sources::source_clear_cache,
            sources::igdb::igdb_fetch,
            sources::igdb::igdb_apply_image,
            sources::nexus::nexus_link_status,
            sources::nexus::nexus_search_games,
            sources::nexus::nexus_set_link,
            sources::nexus::nexus_mods,
            sources::nexus::nexus_mod_detail,
            sources::nexus::nexus_categories,
            sources::nexus::nexus_favorites,
            sources::nexus::nexus_toggle_favorite,
            webshots::search_games,
            webshots::game_images,
            webshots::add_web_screenshots,
            webshots::fetch_image_data,
            webshots::remove_game_screenshot,
            webshots::restore_removed_screenshots,
            collections::list_collections,
            collections::create_collection,
            collections::rename_collection,
            collections::delete_collection,
            collections::set_in_collection,
            controllers::get_controllers,
            gallery::import_screenshots,
            gallery::list_gallery,
            gallery::set_gallery_favorite,
            gallery::delete_gallery_item,
            storage::get_storage,
            system::power_action,
            bk::backup_info,
            bk::backup_target_status,
            bk::backup_create,
            bk::backup_cancel,
            bk::backup_inspect,
            bk::backup_verify,
            bk::backup_restore,
            bk::backup_restart,
            bk::backup_auto_get,
            bk::backup_auto_set,
            bk::backup_auto_password,
            bk::backup_auto_run,
            bk::backup_reveal,
            ra::ra_get_account,
            ra::ra_connect,
            ra::ra_test,
            ra::ra_disconnect,
            ra::ra_sync,
            ra::ra_overview,
            ra::ra_list_achievements,
            ra::ra_game,
            ra::ra_link_status,
            ra::ra_set_link,
            ra::ra_search_games,
            ra::ra_scan_library,
            ra::ra_emulator_support,
            st::steam_get_account,
            st::steam_login,
            st::steam_cancel_login,
            st::steam_set_key,
            st::steam_disconnect,
            st::steam_sync,
            st::steam_owned,
            st::steam_preview,
            st::steam_import,
            st::steam_set_link,
            st::steam_app_status,
            st::steam_install,
            st::steam_open_page,
            st::steam_game,
            st::steam_client_status,
            st::steam_news_overview,
            st::steam_news_refresh,
            st::steam_news_seen,
            st::steam_ach_overview,
            st::steam_ach_list,
            shot_toast::preview_achievement_toast,
            shot_toast::preview_pad_toast,
            ];
            handler
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o NEXSTAGE");
}
