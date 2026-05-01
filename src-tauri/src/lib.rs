use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

// ── API types ──

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct AdapterModel {
    #[serde(rename = "sourceModelId")]
    source_model_id: String,
    provider: String,
    #[serde(rename = "targetModelId")]
    target_model_id: String,
    status: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Adapter {
    name: String,
    #[serde(rename = "type")]
    adapter_type: String,
    #[serde(rename = "baseUrl")]
    #[allow(dead_code)]
    base_url: Option<String>,
    models: Vec<AdapterModel>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct ProviderModel {
    id: String,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct Provider {
    name: String,
    #[serde(rename = "type")]
    #[allow(dead_code)]
    provider_type: String,
    models: Vec<ProviderModel>,
}

#[derive(Debug, serde::Deserialize)]
struct ConfigData {
    providers: Vec<Provider>,
}

#[derive(Debug, serde::Serialize)]
struct UpdateModelMapping {
    #[serde(rename = "sourceModelId")]
    source_model_id: String,
    provider: String,
    #[serde(rename = "targetModelId")]
    target_model_id: String,
}

#[derive(Debug, serde::Serialize)]
struct UpdateAdapterBody {
    name: String,
    #[serde(rename = "type")]
    adapter_type: String,
    models: Vec<UpdateModelMapping>,
}

// ── App state ──

struct ProxyProcess(Mutex<Option<Child>>);

struct AppData {
    adapters: Mutex<Vec<Adapter>>,
    providers: Mutex<Vec<Provider>>,
    log_level: Mutex<String>,
    running: AtomicBool,
}

impl AppData {
    fn new() -> Self {
        Self {
            adapters: Mutex::new(Vec::new()),
            providers: Mutex::new(Vec::new()),
            log_level: Mutex::new("info".to_string()),
            running: AtomicBool::new(false),
        }
    }
}

// ── Config ──

fn proxy_port() -> u16 {
    std::env::var("LLM_PROXY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000)
}

fn api_base() -> String {
    format!("http://127.0.0.1:{}", proxy_port())
}

fn admin_url() -> String {
    format!("http://127.0.0.1:{}/admin/", proxy_port())
}

fn get_json(path: &str) -> Option<serde_json::Value> {
    let resp = minreq::get(&format!("{}{}", api_base(), path))
        .with_header("Accept", "application/json")
        .send()
        .ok()?;
    let body = resp.as_str().ok()?;
    serde_json::from_str(body).ok()
}

fn is_proxy_port_open() -> bool {
    std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{}", proxy_port()).parse().unwrap(),
        Duration::from_secs(1),
    )
    .is_ok()
}

fn fetch_adapters() -> Vec<Adapter> {
    get_json("/admin/adapters")
        .and_then(|v| {
            v["data"]["adapters"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|a| serde_json::from_value::<Adapter>(a.clone()).ok()).collect())
        })
        .unwrap_or_default()
}

fn fetch_config() -> Option<ConfigData> {
    get_json("/admin/config").and_then(|v| {
        v["data"].as_object().map(|data| ConfigData {
            providers: data["providers"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|p| serde_json::from_value::<Provider>(p.clone()).ok()).collect())
                .unwrap_or_default(),
        })
    })
}

fn fetch_log_level() -> String {
    get_json("/admin/log-level")
        .and_then(|v| v["data"]["level"].as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "info".to_string())
}

fn put_log_level(level: &str) -> bool {
    minreq::put(&format!("{}/admin/log-level", api_base()))
        .with_header("Content-Type", "application/json")
        .with_body(serde_json::json!({ "level": level }).to_string())
        .send()
        .map(|r| r.status_code == 200)
        .unwrap_or(false)
}

fn put_adapter(name: &str, adapter_type: &str, models: &[AdapterModel]) -> bool {
    let mappings: Vec<UpdateModelMapping> = models
        .iter()
        .map(|m| UpdateModelMapping {
            source_model_id: m.source_model_id.clone(),
            provider: m.provider.clone(),
            target_model_id: m.target_model_id.clone(),
        })
        .collect();

    let body = UpdateAdapterBody {
        name: name.to_string(),
        adapter_type: adapter_type.to_string(),
        models: mappings,
    };

    minreq::put(&format!("{}/admin/adapters/{}", api_base(), name))
        .with_header("Content-Type", "application/json")
        .with_body(serde_json::to_string(&body).unwrap_or_default())
        .send()
        .map(|r| r.status_code == 200)
        .unwrap_or(false)
}

// ── Binary management ──

fn proxy_binary_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    app.path()
        .resource_dir()
        .unwrap()
        .join("binaries")
        .join(binary_name())
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

fn start_proxy_binary(app: &tauri::AppHandle) -> Option<Child> {
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

// ── Menu building ──

fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let data = app.state::<AppData>();
    let running = is_proxy_port_open();
    data.running.store(running, Ordering::SeqCst);

    let tray = app.tray_by_id("main").unwrap();

    // Header: status
    let status_text = if running {
        "●  llm-proxy 运行中"
    } else {
        "○  llm-proxy 未运行"
    };
    let status = MenuItemBuilder::with_id("status", status_text)
        .enabled(false)
        .build(app)
        .unwrap();

    // Service control
    let toggle_text = if running { "⏹ 停止服务" } else { "▶ 启动服务" };
    let toggle = MenuItemBuilder::with_id("toggle", toggle_text).build(app).unwrap();

    let restart = MenuItemBuilder::with_id("restart", "↺  重启服务")
        .enabled(running)
        .build(app)
        .unwrap();

    // Build adapter submenus
    let adapters = data.adapters.lock().unwrap();
    let providers = data.providers.lock().unwrap();

    let mut menu_builder = MenuBuilder::new(app);
    menu_builder = menu_builder.items(&[&status, &toggle, &restart]);
    menu_builder = menu_builder.separator();

    if adapters.is_empty() {
        let no_conn = MenuItemBuilder::with_id("no_conn", "无法连接到 llm-proxy")
            .enabled(false)
            .build(app)
            .unwrap();
        menu_builder = menu_builder.item(&no_conn);
    } else {
        for adapter in adapters.iter() {
            // Adapter name as disabled header
            let header = MenuItemBuilder::with_id("noop", &adapter.name)
                .enabled(false)
                .build(app)
                .unwrap();
            menu_builder = menu_builder.item(&header);

            for mapping in &adapter.models {
                // Each sourceModelId gets a flat submenu of provider/model options
                let mut sub = SubmenuBuilder::new(app, &format!("  {}", mapping.source_model_id));

                for provider in providers.iter() {
                    for model in &provider.models {
                        let label = format!("{}/{}", provider.name, model.id);
                        let checked = provider.name == mapping.provider && model.id == mapping.target_model_id;
                        let id = format!(
                            "switch:{}:{}:{}:{}",
                            adapter.name, mapping.source_model_id, provider.name, model.id
                        );
                        let item = MenuItemBuilder::with_id(
                            &id,
                            if checked { format!("✓ {}", label) } else { format!("  {}", label) },
                        )
                        .build(app)
                        .unwrap();
                        sub = sub.item(&item);
                    }
                    sub = sub.separator();
                }
                menu_builder = menu_builder.item(&sub.build().unwrap());
            }
            menu_builder = menu_builder.separator();
        }
    }

    menu_builder = menu_builder.separator();

    // Refresh
    let refresh = MenuItemBuilder::with_id("refresh", "刷新").build(app).unwrap();

    // Log level submenu
    let log_level_val = data.log_level.lock().unwrap().clone();
    let mut log_sub = SubmenuBuilder::new(app, &format!("日志级别: {}", log_level_val));
    for level in &["debug", "info", "warn", "error"] {
        let checked = *level == log_level_val;
        let item = MenuItemBuilder::with_id(
            &format!("loglevel:{}", level),
            if checked { format!("✓ {}", level) } else { format!("  {}", level) },
        )
        .build(app)
        .unwrap();
        log_sub = log_sub.item(&item);
    }

    // Open Admin
    let admin = MenuItemBuilder::with_id("open", "打开 Admin UI").build(app).unwrap();

    menu_builder = menu_builder.items(&[&refresh, &log_sub.build().unwrap(), &admin]);
    menu_builder = menu_builder.separator();

    let quit = MenuItemBuilder::with_id("quit", "退出").build(app).unwrap();
    menu_builder = menu_builder.item(&quit);

    tray.set_menu(Some(menu_builder.build().unwrap())).unwrap();
}

// ── Background polling ──

fn start_polling(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));

            let data = app.state::<AppData>();
            if !data.running.load(Ordering::SeqCst) {
                continue;
            }

            // Silently update state — don't rebuild menu (avoids closing open menus)
            *data.log_level.lock().unwrap() = fetch_log_level();
            if let Some(config) = fetch_config() {
                *data.providers.lock().unwrap() = config.providers;
            }
            *data.adapters.lock().unwrap() = fetch_adapters();
        }
    });
}

// ── Main ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(ProxyProcess(Mutex::new(None)))
        .manage(AppData::new())
        .setup(|app| {
            // Create tray
            let _tray = TrayIconBuilder::with_id("main")
                .tooltip("LLM Proxy")
                .icon(app.default_window_icon().unwrap().clone())
                .icon_as_template(true)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| {
                    let id = event.id().0.clone();

                    if id == "open" {
                        let _ = open::that(&admin_url());
                        return;
                    }

                    if id == "refresh" {
                        let data = app.state::<AppData>();
                        if let Some(config) = fetch_config() {
                            *data.providers.lock().unwrap() = config.providers;
                        }
                        *data.adapters.lock().unwrap() = fetch_adapters();
                        *data.log_level.lock().unwrap() = fetch_log_level();
                        rebuild_tray_menu(app);
                        return;
                    }

                    if id == "toggle" {
                        let h = app.app_handle().clone();
                        let is_running = app.state::<AppData>().running.load(Ordering::SeqCst);
                        std::thread::spawn(move || {
                            let process = h.state::<ProxyProcess>();
                            if is_running {
                                stop_proxy(&process.0);
                                std::thread::sleep(Duration::from_millis(800));
                            } else {
                                let child = start_proxy_binary(&h);
                                *process.0.lock().unwrap() = child;
                                std::thread::sleep(Duration::from_millis(2000));
                            }
                            let h2 = h.clone();
                            h.run_on_main_thread(move || rebuild_tray_menu(&h2)).ok();
                        });
                        return;
                    }

                    if id == "restart" {
                        let h = app.app_handle().clone();
                        std::thread::spawn(move || {
                            let process = h.state::<ProxyProcess>();
                            stop_proxy(&process.0);
                            std::thread::sleep(Duration::from_millis(500));
                            let child = start_proxy_binary(&h);
                            *process.0.lock().unwrap() = child;
                            std::thread::sleep(Duration::from_secs(2));
                            let h2 = h.clone();
                            h.run_on_main_thread(move || rebuild_tray_menu(&h2)).ok();
                        });
                        return;
                    }

                    if id.starts_with("loglevel:") {
                        let level = id.strip_prefix("loglevel:").unwrap();
                        put_log_level(level);
                        let data = app.state::<AppData>();
                        *data.log_level.lock().unwrap() = level.to_string();
                        rebuild_tray_menu(app);
                        return;
                    }

                    if id.starts_with("switch:") {
                        let parts: Vec<&str> = id.splitn(5, ':').collect();
                        if parts.len() == 5 {
                            let adapter_name = parts[1];
                            let source_model = parts[2];
                            let new_provider = parts[3];
                            let new_target = parts[4];

                            let data = app.state::<AppData>();
                            let adapters = data.adapters.lock().unwrap();
                            if let Some(adapter) = adapters.iter().find(|a| a.name == adapter_name) {
                                let updated_models: Vec<AdapterModel> = adapter
                                    .models
                                    .iter()
                                    .map(|m| {
                                        let mut model = AdapterModel {
                                            source_model_id: m.source_model_id.clone(),
                                            provider: m.provider.clone(),
                                            target_model_id: m.target_model_id.clone(),
                                            status: m.status.clone(),
                                        };
                                        if m.source_model_id == source_model {
                                            model.provider = new_provider.to_string();
                                            model.target_model_id = new_target.to_string();
                                        }
                                        model
                                    })
                                    .collect();

                                put_adapter(adapter_name, &adapter.adapter_type, &updated_models);
                                std::thread::sleep(Duration::from_millis(300));
                                let new_adapters = fetch_adapters();
                                *data.adapters.lock().unwrap() = new_adapters;
                            }
                            rebuild_tray_menu(app);
                        }
                        return;
                    }

                    if id == "quit" {
                        let process = app.state::<ProxyProcess>();
                        stop_proxy(&process.0);
                        app.exit(0);
                    }
                })

                .build(app)?;

            // Initial data fetch & menu
            rebuild_tray_menu(app.app_handle());

            // Auto-start proxy
            let handle = app.app_handle().clone();
            let child = start_proxy_binary(&handle);
            *app.state::<ProxyProcess>().0.lock().unwrap() = child;

            // Give proxy time to start, then refresh
            std::thread::sleep(Duration::from_secs(2));

            let data = app.state::<AppData>();
            if let Some(config) = fetch_config() {
                *data.providers.lock().unwrap() = config.providers;
            }
            *data.adapters.lock().unwrap() = fetch_adapters();
            *data.log_level.lock().unwrap() = fetch_log_level();
            rebuild_tray_menu(app.app_handle());

            // Start background polling
            start_polling(app.app_handle().clone());

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
