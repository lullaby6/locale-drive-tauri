# Locale Drive Desktop

The desktop app for [Locale Drive](https://github.com/lullaby6/locale-drive): share and transfer files across your local network, like a "Google Drive" for your LAN, from a Windows app instead of the terminal.

Open the app and it starts the Locale Drive server in the background and opens it in your default browser. Every device on the same Wi-Fi can connect with the QR code or link shown in the page.

The app lives in the notification area (system tray), next to the clock:

- **Click** the icon, or choose **Open**, to open the page again.
- **Right click** and choose **Exit** to stop the server and close the app.

Opening the app again while it is running just opens the page.

Node.js does not need to be installed: the app ships its own.

## Download

Get the installer (`Locale Drive_x.x.x_x64-setup.exe`) from the [releases](https://github.com/lullaby6/locale-drive-tauri/releases) and run it.

- Requires **Windows 10 or 11**.
- The first time it opens, Windows Firewall asks whether `node` may use the network: allow it on **private networks**, or other devices won't be able to connect.

Files are stored in `Documents\Locale Drive`.

## How it works

Locale Drive is an Express server that also serves its React web app, so the desktop app is a small [Tauri](https://tauri.app) app with no window that runs it:

1. The app adds its icon to the notification area and runs the server (`locale-drive/src/index.js`) with a bundled Node.js, as a Tauri sidecar.
2. Once the server prints its LAN address, the app opens it in the default browser, as the CLI does.
3. If the server stops on its own, a message box shows its output and the app closes.
4. The server is stopped when the app exits, and on Windows it is also tied to the app's process, so it never keeps running (and sharing files) after a crash.

## Project structure

```
locale-drive/              git submodule: the server, with its built web app in src/public
scripts/prepare-server.js  copies the server and its production dependencies into
                           src-tauri/server/ and Node.js into src-tauri/binaries/
src-tauri/                 the Tauri app (Rust)
```

`src-tauri/server/` and `src-tauri/binaries/` are generated, not committed.

## Development

Requirements: [Node.js](https://nodejs.org) 22.12 or newer (it is the runtime bundled into the app), [Rust](https://rustup.rs) and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
git clone --recursive https://github.com/lullaby6/locale-drive-tauri
cd locale-drive-tauri
npm run setup
```

| Command | What it does |
| --- | --- |
| `npm start` | Runs the app in development mode |
| `npm run build` | Builds the installer into `src-tauri/target/release/bundle/nsis/` |
| `npm run build-server` | Prepares the server and Node.js (runs automatically before `start` and `build`) |
| `npm run build-frontend` | Same, but first rebuilds the web app from the `locale-drive-frontend` submodule |
| `npm run icons` | Regenerates the app icons from the Locale Drive logo |

By default the app uses the web app already built in `locale-drive/src/public`. To ship a newer version of Locale Drive, update the submodule:

```bash
cd locale-drive
git pull origin main
cd ..
git add locale-drive
```

## License

MIT
