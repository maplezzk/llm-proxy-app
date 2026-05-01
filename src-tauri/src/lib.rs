use std::process::{Child, Command};
use std::sync::Mutex;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

struct ProxyProcess(Mutex<Option<Child>>);

fn proxy_binary_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let resource_dir = app.path().resource_dir().unwrap();
    resource_dir.join("binaries").join(binary_name())
}

fn binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "llm-proxy.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "llm-proxy"
    }
}

fn start_proxy(app: &tauri::AppHandle) -> Option<Child> {
    let path = proxy_binary_path(app);
    if !path.exists() {
        log::warn!("Proxy binary not found: {:?}", path);
        return None;
    }
    log::info!("Starting llm-proxy: {:?}", path);
    Command::new(&path)
        .arg("start")
        .current_dir(path.parent().unwrap())
        .spawn()
        .ok()
}

fn stop_proxy(process: &Mutex<Option<Child>>) {
    if let Some(mut child) = process.lock().unwrap().take() {
        log::info!("Stopping llm-proxy");
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn is_proxy_running() -> bool {
    // Simple health check on default port
    std::net::TcpStream::connect("127.0.0.1:9000").is_ok()
}

fn update_tray_menu(app: &tauri::AppHandle) {
    let running = is_proxy_running();
    let tray = app.tray_by_id("main").unwrap();

    let status_text = if running {
        "🟢 Proxy Running"
    } else {
        "🔴 Proxy Stopped"
    };
    let toggle_text = if running {
        "Stop Proxy"
    } else {
        "Start Proxy"
    };

    let status = MenuItemBuilder::with_id("status", status_text)
        .enabled(false)
        .build(app)
        .unwrap();
    let open_admin = MenuItemBuilder::with_id("open", "Open Admin UI")
        .enabled(running)
        .build(app)
        .unwrap();
    let toggle = MenuItemBuilder::with_id("toggle", toggle_text)
        .build(app)
        .unwrap();
    let quit = MenuItemBuilder::with_id("quit", "Quit")
        .build(app)
        .unwrap();

    let menu = MenuBuilder::new(app)
        .items(&[&status, &open_admin, &toggle, &quit])
        .build()
        .unwrap();

    tray.set_menu(Some(menu)).unwrap();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(ProxyProcess(Mutex::new(None)))
        .setup(|app| {
            // Create tray
            let _tray = TrayIconBuilder::with_id("main")
                .tooltip("LLM Proxy")
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| {
                    match event.id().as_ref() {
                        "open" => {
                            let _ = open::that("http://127.0.0.1:9000/admin/");
                        }
                        "toggle" => {
                            let state = app.state::<ProxyProcess>();
                            if is_proxy_running() {
                                stop_proxy(&state.0);
                            } else {
                                let handle = app.app_handle().clone();
                                let child = start_proxy(&handle);
                                *state.0.lock().unwrap() = child;
                            }
                            // Give the process a moment to start/stop
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            update_tray_menu(app.app_handle());
                        }
                        "quit" => {
                            let state = app.state::<ProxyProcess>();
                            stop_proxy(&state.0);
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let _ = tray.app_handle();
                        let _ = open::that("http://127.0.0.1:9000/admin/");
                    }
                })
                .build(app)?;

            update_tray_menu(app.app_handle());

            // Auto-start proxy
            let handle = app.app_handle().clone();
            let child = start_proxy(&handle);
            let state = app.state::<ProxyProcess>();
            *state.0.lock().unwrap() = child;

            // Update menu after startup
            std::thread::sleep(std::time::Duration::from_millis(1000));
            update_tray_menu(app.app_handle());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
