// Hide the console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, RunEvent, Url};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;

// Locale Drive is an Express server that also serves the web app, so the desktop app
// runs it with a bundled Node.js (the "node" sidecar, see scripts/prepare-server.js)
// and points the window at it once it is listening. The window starts on the splash
// page (../splash) and comes back to it with the server's output if the server stops.

/// The running server, killed when the app exits
struct Server(Mutex<Option<CommandChild>>);

/// Lines of server output kept to show if it stops
const LOG_LINES: usize = 30;

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
fn parse_port(line: &str) -> Option<u16> {
    let url = line.split("Server listening on:").nth(1)?.trim();
    url.rsplit(':').next()?.parse().ok()
}

/// Opens the splash page again, which shows the error passed in the URL hash
fn show_error(app: &AppHandle, splash: &Url, message: &str) {
    if let Some(window) = app.get_webview_window("main") {
        // Percent-encoded by hand: URL parsing would drop the line breaks
        let encoded: String = message
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (byte as char).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect();
        let mut url = splash.clone();
        url.set_fragment(Some(&encoded));
        let _ = window.navigate(url);
    }
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
    let window = app
        .get_webview_window("main")
        .ok_or("the main window is missing")?;
    let splash = window.url()?;

    let entry = app
        .path()
        .resolve("server/src/index.js", BaseDirectory::Resource)?;
    let storage = storage_dir(app)?;

    let (mut events, child) = app
        .shell()
        .sidecar("node")?
        .args([plain_path(&entry), plain_path(&storage), "--no-open".into()])
        .spawn()?;

    #[cfg(windows)]
    kill_with_app(child.pid());

    app.state::<Server>().0.lock().unwrap().replace(child);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut log: VecDeque<String> = VecDeque::new();
        let mut ready = false;

        while let Some(event) = events.recv().await {
            match event {
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    let line = String::from_utf8_lossy(&bytes).trim_end().to_string();

                    #[cfg(debug_assertions)]
                    println!("[server] {line}");

                    if !ready {
                        if let Some(port) = parse_port(&line) {
                            ready = true;
                            if let Some(window) = app.get_webview_window("main") {
                                let url = format!("http://127.0.0.1:{port}");
                                let _ = window.navigate(Url::parse(&url).unwrap());
                            }
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
                    app.state::<Server>().0.lock().unwrap().take();

                    let code = payload
                        .code
                        .map_or("unknown".to_string(), |code| code.to_string());
                    let output = Vec::from(log.clone()).join("\n");
                    show_error(&app, &splash, &format!("Exit code: {code}\n\n{output}"));
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
        .plugin(tauri_plugin_shell::init())
        .manage(Server(Mutex::new(None)))
        .setup(|app| {
            start_server(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Locale Drive");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            if let Some(child) = app.state::<Server>().0.lock().unwrap().take() {
                let _ = child.kill();
            }
        }
    });
}
