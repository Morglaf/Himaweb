//! Icône de barre système : ouvrir, démarrer au login, quitter.
//!
//! Sous Windows, tray-icon / muda exigent une boucle de messages Win32
//! sur le même thread que l’icône — sinon le menu ne s’affiche jamais.

use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder, TrayIconEvent};

static QUIT: AtomicBool = AtomicBool::new(false);

pub fn request_quit() {
    QUIT.store(true, Ordering::SeqCst);
}

pub fn should_quit() -> bool {
    QUIT.load(Ordering::SeqCst)
}

pub fn spawn(url: String) {
    std::thread::Builder::new()
        .name("himaweb-tray".into())
        .spawn(move || {
            if let Err(e) = run_tray(url) {
                tracing::warn!("tray: {e}");
            }
        })
        .ok();
}

fn run_tray(url: String) -> Result<(), String> {
    let icon = make_icon().map_err(|e| e.to_string())?;

    let open_item = MenuItem::with_id("open", "Ouvrir HimaWeb", true, None);
    let autostart_item = CheckMenuItem::with_id(
        "autostart",
        "Lancer au démarrage",
        true,
        is_autostart_enabled(),
        None,
    );
    let restart_item = MenuItem::with_id("restart", "Relancer", true, None);
    let quit_item = MenuItem::with_id("quit", "Quitter", true, None);

    let menu = Menu::new();
    menu.append(&open_item).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&autostart_item).map_err(|e| e.to_string())?;
    menu.append(&restart_item).map_err(|e| e.to_string())?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|e| e.to_string())?;
    menu.append(&quit_item).map_err(|e| e.to_string())?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("HimaWeb")
        .with_icon(icon)
        .with_menu_on_left_click(true)
        .build()
        .map_err(|e| e.to_string())?;

    let menu_channel = MenuEvent::receiver();
    let tray_channel = TrayIconEvent::receiver();
    let url = Arc::new(url);

    loop {
        if should_quit() {
            break;
        }

        // Indispensable sous Windows : dispatcher les messages pour le menu.
        pump_native_events();

        while let Ok(event) = menu_channel.try_recv() {
            match event.id.0.as_str() {
                "open" => {
                    let _ = open::that(url.as_str());
                }
                "autostart" => {
                    let next = !is_autostart_enabled();
                    if let Err(e) = set_autostart(next) {
                        tracing::warn!("autostart: {e}");
                    } else {
                        autostart_item.set_checked(next);
                    }
                }
                "restart" => {
                    restart_self();
                    request_quit();
                }
                "quit" => {
                    request_quit();
                }
                _ => {}
            }
        }
        while let Ok(event) = tray_channel.try_recv() {
            if let TrayIconEvent::DoubleClick { .. } = event {
                let _ = open::that(url.as_str());
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn pump_native_events() {
    use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage, MSG,
        PM_REMOVE, QS_ALLINPUT,
    };

    unsafe {
        // Attendre jusqu’à 100 ms un message (menu / clic tray).
        let wait = MsgWaitForMultipleObjects(0, std::ptr::null(), 0, 100, QS_ALLINPUT);
        if wait == WAIT_TIMEOUT {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn pump_native_events() {
    std::thread::sleep(std::time::Duration::from_millis(100));
}

fn make_icon() -> Result<Icon, tray_icon::BadIcon> {
    // Montagne HimaWeb 32×32 (fond sombre, sommet blanc, accent bleu)
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let bg = [15u8, 17, 21, 255];
    let peak = [245u8, 247, 250, 255];
    let accent = [26u8, 115, 232, 255];

    let in_mountain = |x: i32, y: i32| -> bool {
        let left = {
            let apex_x = 11;
            let apex_y = 8;
            let base_y = 26;
            if y < apex_y || y > base_y {
                false
            } else {
                let t = (y - apex_y) as f32 / (base_y - apex_y) as f32;
                let half = 1.0 + t * 8.0;
                (x as f32 - apex_x as f32).abs() <= half
            }
        };
        let right = {
            let apex_x = 21;
            let apex_y = 5;
            let base_y = 26;
            if y < apex_y || y > base_y {
                false
            } else {
                let t = (y - apex_y) as f32 / (base_y - apex_y) as f32;
                let half = 1.0 + t * 9.0;
                (x as f32 - apex_x as f32).abs() <= half
            }
        };
        left || right
    };

    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let i = ((y as u32 * size + x as u32) * 4) as usize;
            let c = if in_mountain(x, y) {
                if y < 12 {
                    peak
                } else if y < 16 && ((x - 11).abs() < 3 || (x - 21).abs() < 3) {
                    peak
                } else {
                    accent
                }
            } else {
                bg
            };
            rgba[i] = c[0];
            rgba[i + 1] = c[1];
            rgba[i + 2] = c[2];
            rgba[i + 3] = c[3];
        }
    }
    Icon::from_rgba(rgba, size, size)
}

fn exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

#[cfg(windows)]
fn is_autostart_enabled() -> bool {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey_with_flags(
        r"Software\Microsoft\Windows\CurrentVersion\Run",
        KEY_READ,
    ) else {
        return false;
    };
    key.get_value::<String, _>("HimaWeb").is_ok()
}

#[cfg(not(windows))]
fn is_autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
fn set_autostart(enabled: bool) -> Result<(), String> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run")
        .map_err(|e| e.to_string())?;
    if enabled {
        let exe = exe_path()?;
        let val = format!("\"{}\"", exe.display());
        key.set_value("HimaWeb", &val).map_err(|e| e.to_string())?;
    } else {
        let _ = key.delete_value("HimaWeb");
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_autostart(_enabled: bool) -> Result<(), String> {
    Err("autostart non supporté sur cette plateforme".into())
}

fn restart_self() {
    if let Ok(exe) = exe_path() {
        let _ = Command::new(exe).spawn();
    }
}
