// Hide the console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::path::BaseDirectory;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

// Locale Drive is an Express server that also serves the web app, so the desktop app
// runs it with a bundled Node.js (the "node" sidecar, see scripts/prepare-server.js)
// and opens it in the default browser, like the CLI does. The app has no window: it
// lives in the notification area, with a menu to open the page again or to exit.

const TRAY_ID: &str = "main";

/// Lines of server output kept to show if it stops
const LOG_LINES: usize = 30;

#[derive(Default)]
struct Server {
    /// The running server, killed when the app exits
    child: Mutex<Option<CommandChild>>,
    /// The LAN address the server prints once it is listening
    url: Mutex<Option<String>>,
    /// Set on exit, so the server stopping then isn't reported as an error
    quitting: AtomicBool,
}

/// Where files are stored: Documents/Locale Drive
fn storage_dir(app: &AppHandle) -> tauri::Result<std::path::PathBuf> {
    Ok(app.path().document_dir()?.join("Locale Drive"))
}

/// Node.js can't always handle Windows' verbatim "\\?\C:\..." paths, so use plain ones
fn plain_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    path.strip_prefix(r"\\?\").unwrap_or(&path).to_string()
}

/// The server prints "Server listening on: http://<LAN IP>:<port>" once it is up
fn parse_url(line: &str) -> Option<String> {
    let url = line.split("Server listening on:").nth(1)?.trim();
    url.starts_with("http").then(|| url.to_string())
}

/// Opens the app in the default browser (does nothing until the server is up,
/// which opens it by itself)
fn open_in_browser(app: &AppHandle) {
    let url = app.state::<Server>().url.lock().unwrap().clone();
    if let Some(url) = url {
        let _ = app.opener().open_url(url, None::<&str>);
    }
}

/// Shows the error in a message box and closes the app once it is dismissed
fn fail(app: &AppHandle, message: String) {
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title("Locale Drive")
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

/// The notification area icon: click to open the app, right click for the menu
fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let spanish = sys_locale::get_locale().is_some_and(|locale| locale.starts_with("es"));
    let (open_label, exit_label) = if spanish {
        ("Abrir", "Salir")
    } else {
        ("Open", "Exit")
    };

    let open = MenuItem::with_id(app, "open", open_label, true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", exit_label, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &exit])?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("Locale Drive")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_in_browser(app),
            "exit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_in_browser(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// The server is killed on a normal exit (see main), but if the app crashes or is
/// killed from the Task Manager it would keep running and sharing files. Putting it
/// in a job that kills its processes when closed ties it to the app's lifetime: the
/// job handle is never closed, so Windows closes it when the app's process ends
#[cfg(windows)]
fn kill_with_app(pid: u32) {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() {
            return;
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );

        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if !process.is_null() {
            AssignProcessToJobObject(job, process);
            CloseHandle(process);
        }
    }
}

fn start_server(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let entry = app
        .path()
        .resolve("server/src/index.js", BaseDirectory::Resource)?;
    let storage = storage_dir(app)?;

    // --no-open: the app opens the browser itself, once it knows the URL
    let (mut events, child) = app
        .shell()
        .sidecar("node")?
        .args([plain_path(&entry), plain_path(&storage), "--no-open".into()])
        .spawn()?;

    #[cfg(windows)]
    kill_with_app(child.pid());

    app.state::<Server>().child.lock().unwrap().replace(child);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut log: VecDeque<String> = VecDeque::new();

        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    let line = String::from_utf8_lossy(&bytes).trim_end().to_string();

                    #[cfg(debug_assertions)]
                    println!("[server] {line}");

                    let state = app.state::<Server>();
                    let first_url = state.url.lock().unwrap().is_none();

                    if first_url {
                        if let Some(url) = parse_url(&line) {
                            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                                let _ = tray.set_tooltip(Some(format!("Locale Drive\n{url}")));
                            }
                            state.url.lock().unwrap().replace(url);
                            open_in_browser(&app);
                        }
                    }

                    log.push_back(line);
                    if log.len() > LOG_LINES {
                        log.pop_front();
                    }
                }
                CommandEvent::Error(error) => {
                    log.push_back(error);
                }
                CommandEvent::Terminated(payload) => {
                    let state = app.state::<Server>();
                    state.child.lock().unwrap().take();

                    if !state.quitting.load(Ordering::SeqCst) {
                        let code = payload
                            .code
                            .map_or("unknown".to_string(), |code| code.to_string());
                        let output = Vec::from(log.clone()).join("\n");
                        fail(
                            &app,
                            format!("The server stopped (exit code {code}).\n\n{output}"),
                        );
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn main() {
    let app = tauri::Builder::default()
        // Opening the app again while it runs opens the page instead of starting
        // a second server
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            open_in_browser(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(Server::default())
        .setup(|app| {
            create_tray(app.handle())?;
            if let Err(error) = start_server(app.handle()) {
                fail(app.handle(), format!("Could not start the server.\n\n{error}"));
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Locale Drive");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            let state = app.state::<Server>();
            state.quitting.store(true, Ordering::SeqCst);
            let child = state.child.lock().unwrap().take();
            if let Some(child) = child {
                let _ = child.kill();
            }
        }
    });
}
