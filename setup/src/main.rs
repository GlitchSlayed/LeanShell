#![windows_subsystem = "windows"]
#![allow(dead_code)]

//! Native Win32 + Direct2D installer, uninstaller, and elevated swap helper for LeanShell.
//!
//! Installs machine-wide into `%ProgramFiles%\LeanShell`, registers Windows 11 Installed Apps
//! uninstall metadata, creates Start Menu shortcuts, configures optional exclusive-shell and
//! IFEO host neutering, and handles `--silent`, `--uninstall`, and `--apply-staged-update` CLI flows.

#[path = "../../src/channel.rs"]
mod channel;
#[path = "../../src/install.rs"]
mod install;

use std::io::{Cursor, Read};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use leanshell_core::updater::{ALL_MANAGED_FILES, apply_staged_dir};
use toml_edit::{DocumentMut, Item, Value};
use windows::Win32::Foundation::{
    CloseHandle, D2DERR_RECREATE_TARGET, ERROR_FILE_NOT_FOUND, HANDLE, HWND, LPARAM, LRESULT, RECT, WAIT_OBJECT_0,
    WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_SYNCHRONIZE, WaitForSingleObject,
};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::Shell::{IShellLinkW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW, ShellLink};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{HSTRING, Interface, PCWSTR, w};

const EMBEDDED_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.zip"));
const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\LeanShell";
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");
const SETUP_CLASS: PCWSTR = w!("LeanShell.Setup");
const WIN_W_DIP: f32 = 700.0;
const WIN_H_DIP: f32 = 540.0;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum CliMode {
    Gui,
    SilentInstall { launch: bool },
    Uninstall { silent: bool },
    ApplyStaged { stage_dir: PathBuf, target_dir: PathBuf },
}

#[derive(Clone, Copy, Debug)]
struct InstallOptions {
    autostart: bool,
    exclusive_shell: bool,
    neuter_hosts: bool,
    auto_update: bool,
    launch_after: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        Self {
            autostart: true,
            exclusive_shell: true,
            neuter_hosts: true,
            auto_update: true,
            launch_after: true,
        }
    }
}

fn parse_cli(args: &[String]) -> CliMode {
    if let Some(stage_dir) = arg_value(args, "--apply-staged-update") {
        let target_dir = arg_value(args, "--target-dir")
            .map(PathBuf::from)
            .unwrap_or_else(install::default_install_dir);
        return CliMode::ApplyStaged {
            stage_dir: PathBuf::from(stage_dir),
            target_dir,
        };
    }
    let silent = args.iter().any(|a| {
        a.eq_ignore_ascii_case("--silent")
            || a.eq_ignore_ascii_case("/silent")
            || a.eq_ignore_ascii_case("/s")
            || a.eq_ignore_ascii_case("/quiet")
            || a.eq_ignore_ascii_case("/qn")
    });
    if args.iter().any(|a| a.eq_ignore_ascii_case("--uninstall")) {
        return CliMode::Uninstall { silent };
    }
    if silent {
        let no_launch = args.iter().any(|a| a.eq_ignore_ascii_case("--no-launch"));
        return CliMode::SilentInstall { launch: !no_launch };
    }
    CliMode::Gui
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = parse_cli(&args);
    if !matches!(mode, CliMode::Gui) {
        // SAFETY: Attach to caller console if launched from PowerShell/cmd/WinGet for CLI output.
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }

    match mode {
        CliMode::ApplyStaged { stage_dir, target_dir } => {
            if let Err(e) = apply_staged_dir(&stage_dir, &target_dir) {
                eprintln!("LeanShell Setup: staged update failed: {e}");
                std::process::exit(1);
            }
        }
        CliMode::SilentInstall { launch } => {
            if !is_elevated() {
                match relaunch_elevated(&args[1..], true) {
                    Ok(code) => std::process::exit(code),
                    Err(e) => {
                        eprintln!("LeanShell Setup elevation failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            let opts = InstallOptions {
                launch_after: launch,
                exclusive_shell: true,
                ..InstallOptions::default()
            };
            if let Err(e) = perform_install(&install::default_install_dir(), opts) {
                eprintln!("LeanShell Setup failed: {e}");
                std::process::exit(1);
            }
            println!("Installed LeanShell {} successfully.", env!("CARGO_PKG_VERSION"));
        }
        CliMode::Uninstall { silent } => {
            if !is_elevated() {
                match relaunch_elevated(&args[1..], true) {
                    Ok(code) => std::process::exit(code),
                    Err(e) => {
                        eprintln!("LeanShell Uninstall elevation failed: {e}");
                        std::process::exit(1);
                    }
                }
            }
            if let Err(e) = perform_uninstall(&install::default_install_dir()) {
                eprintln!("LeanShell Uninstall failed: {e}");
                if !silent {
                    show_error_box(&format!("LeanShell Uninstall failed:\n\n{e}"));
                }
                std::process::exit(1);
            }
            println!("Uninstalled LeanShell and restored stock Windows shell.");
        }
        CliMode::Gui => {
            if !is_elevated() {
                if let Err(e) = relaunch_elevated(&[], false) {
                    show_error_box(&format!(
                        "Administrator permission is required to install into Program Files:\n\n{e}"
                    ));
                }
                return;
            }
            // SAFETY: Configure per-monitor DPI awareness before creating Direct2D setup window.
            unsafe {
                let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            }
            if let Err(e) = run_gui() {
                show_error_box(&format!("LeanShell Setup error:\n\n{e}"));
            }
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn show_error_box(msg: &str) {
    let text = wide(msg);
    let cap = wide("LeanShell Setup");
    // SAFETY: Displays modal Win32 error box with null-terminated UTF-16 slices.
    unsafe {
        let _ = MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(cap.as_ptr()), MB_ICONERROR | MB_OK);
    }
}

fn is_elevated() -> bool {
    let mut token = HANDLE::default();
    // SAFETY: Queries elevation token information for the current process.
    unsafe {
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

fn relaunch_elevated(args: &[String], wait: bool) -> Result<i32, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let params: Vec<String> = args.iter().map(|a| quote_arg(a)).collect();
    let params_str = params.join(" ");
    let exe_w = wide(&exe.to_string_lossy());
    let params_w = wide(&params_str);

    // SAFETY: Triggers UAC elevation for current installer executable and closes child handle if requested.
    unsafe {
        let mut info = SHELLEXECUTEINFOW {
            cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: if wait {
                SEE_MASK_NOCLOSEPROCESS
            } else {
                Default::default()
            },
            lpVerb: w!("runas"),
            lpFile: PCWSTR(exe_w.as_ptr()),
            lpParameters: if params_str.is_empty() {
                PCWSTR::null()
            } else {
                PCWSTR(params_w.as_ptr())
            },
            nShow: if wait { SW_HIDE.0 } else { SW_SHOWNORMAL.0 },
            ..Default::default()
        };
        ShellExecuteExW(&mut info).map_err(|e| e.to_string())?;
        if wait && !info.hProcess.is_invalid() {
            let _ = WaitForSingleObject(info.hProcess, u32::MAX);
            let mut code = 0u32;
            let _ = windows::Win32::System::Threading::GetExitCodeProcess(info.hProcess, &mut code);
            let _ = CloseHandle(info.hProcess);
            return Ok(code as i32);
        }
        if !info.hProcess.is_invalid() {
            let _ = CloseHandle(info.hProcess);
        }
    }
    Ok(0)
}

fn quote_arg(arg: &str) -> String {
    if arg.contains([' ', '\t', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn collect_payload(setup_exe: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    if !EMBEDDED_ZIP.is_empty() {
        let mut archive =
            zip::ZipArchive::new(Cursor::new(EMBEDDED_ZIP)).map_err(|e| format!("Corrupt embedded archive: {e}"))?;
        let mut out = Vec::new();
        for name in ["leanshell.exe", "leanshell-settings.exe", "LICENSE", "README.md"] {
            if let Ok(mut entry) = archive.by_name(name)
                && !entry.is_dir()
            {
                let mut buf = Vec::new();
                entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
                out.push((name.to_string(), buf));
            }
        }
        if !out.iter().any(|(n, _)| n == "leanshell.exe") {
            return Err("Embedded installer archive does not contain leanshell.exe.".into());
        }
        let setup_bytes = std::fs::read(setup_exe).map_err(|e| format!("Cannot read setup executable: {e}"))?;
        out.push(("leanshell-setup.exe".into(), setup_bytes));
        return Ok(out);
    }

    let source_dir = setup_exe
        .parent()
        .ok_or("Cannot determine installer source directory.")?;
    let mut out = Vec::new();
    for required in ["leanshell.exe", "leanshell-settings.exe"] {
        let src = source_dir.join(required);
        let bytes = std::fs::read(&src).map_err(|e| {
            format!(
                "Cannot find {required} beside leanshell-setup.exe ({}): {e}",
                src.display()
            )
        })?;
        out.push((required.to_string(), bytes));
    }
    for optional in ["LICENSE", "README.md"] {
        let src = source_dir.join(optional);
        if let Ok(bytes) = std::fs::read(&src) {
            out.push((optional.to_string(), bytes));
        }
    }
    let setup_bytes = std::fs::read(setup_exe).map_err(|e| format!("Cannot read setup executable: {e}"))?;
    out.push(("leanshell-setup.exe".into(), setup_bytes));
    Ok(out)
}

fn stop_running_instances(hot_restart: bool) {
    // SAFETY: Requests any running LeanShell launcher instance to close gracefully so files can be swapped.
    unsafe {
        for class in [w!("LeanShell.Launcher"), w!("LeanShell.Nightly.Launcher")] {
            if let Ok(hwnd) = FindWindowW(class, None)
                && !hwnd.is_invalid()
            {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let proc_handle = OpenProcess(PROCESS_SYNCHRONIZE, false, pid).ok();
                // WPARAM(1) signals hot restart so LeanShell preserves Winlogon\Shell and does not respawn Explorer.
                let mode = if hot_restart { 1 } else { 0 };
                let _ = PostMessageW(Some(hwnd), WM_APP + 4, WPARAM(mode), LPARAM(0));
                if let Some(h) = proc_handle {
                    let _ = WaitForSingleObject(h, 4000) == WAIT_OBJECT_0;
                    let _ = CloseHandle(h);
                }
            }
        }
    }
}

fn perform_install(target_dir: &Path, opts: InstallOptions) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let payload = collect_payload(&current_exe)?;

    stop_running_instances(true);

    std::fs::create_dir_all(target_dir).map_err(|e| format!("Cannot create {}: {e}", target_dir.display()))?;

    let stage_dir = std::env::temp_dir().join(format!("leanshell-setup-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage_dir);
    std::fs::create_dir_all(&stage_dir).map_err(|e| format!("Cannot create staging folder: {e}"))?;

    let mut total_bytes = 0u64;
    for (name, bytes) in &payload {
        total_bytes += bytes.len() as u64;
        if ALL_MANAGED_FILES.contains(&name.as_str()) {
            std::fs::write(stage_dir.join(format!("{name}.new")), bytes)
                .map_err(|e| format!("Cannot stage {name}: {e}"))?;
        } else {
            let _ = std::fs::write(target_dir.join(name), bytes);
        }
    }

    let swap_res = apply_staged_dir(&stage_dir, target_dir);
    let _ = std::fs::remove_dir_all(&stage_dir);
    swap_res?;

    let installed_exe = target_dir.join("leanshell.exe");
    let settings_exe = target_dir.join("leanshell-settings.exe");

    write_auto_update_config(opts.auto_update);
    write_start_menu_shortcuts(&installed_exe, &settings_exe);
    write_uninstall_registry(target_dir, &installed_exe, total_bytes)?;
    set_autostart(&installed_exe, opts.autostart)?;

    if opts.exclusive_shell {
        leanshell_platform_win::register_exclusive_shell(&installed_exe)
            .map_err(|e| format!("Failed to register exclusive shell: {e}"))?;
        let _ = leanshell_platform_win::nuke_explorer_shell();
    }
    if opts.neuter_hosts {
        let _ = leanshell_platform_win::neuter_shell_hosts(leanshell_core::DEFAULT_NEUTERED_HOSTS);
    }

    if opts.launch_after {
        launch_unelevated(&installed_exe);
    }
    Ok(())
}

fn perform_uninstall(target_dir: &Path) -> Result<(), String> {
    let installed_exe = target_dir.join("leanshell.exe");
    if installed_exe.is_file() {
        let _ = std::process::Command::new(&installed_exe)
            .arg("--restore-shell")
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    } else {
        stop_running_instances(false);
        let _ = leanshell_platform_win::unregister_exclusive_shell();
        let _ = leanshell_platform_win::restore_shell_hosts();
    }

    let _ = set_autostart(&installed_exe, false);
    remove_start_menu_shortcuts();
    delete_uninstall_registry()?;

    // Clean up non-locked binaries and .old backups; if running from target_dir,
    // schedule delayed directory removal immediately after this process exits.
    for file in ALL_MANAGED_FILES {
        let p = target_dir.join(file);
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(leanshell_core::updater::old_path(&p));
    }
    for doc in ["LICENSE", "README.md"] {
        let _ = std::fs::remove_file(target_dir.join(doc));
    }

    if std::fs::remove_dir(target_dir).is_err() && target_dir.exists() {
        let _ = std::process::Command::new("cmd.exe")
            .args([
                "/C",
                "ping",
                "127.0.0.1",
                "-n",
                "2",
                ">nul",
                "&",
                "rmdir",
                "/S",
                "/Q",
                &target_dir.to_string_lossy(),
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
    }
    Ok(())
}

fn launch_unelevated(exe: &Path) {
    // Launch directly in a detached process group without explorer.exe so Windows Explorer's shell/taskbar never starts.
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    let _ = std::process::Command::new(exe)
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn();
}

fn write_auto_update_config(enabled: bool) {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return;
    };
    let dir = appdata.join(channel::DATA_DIR);
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("config.toml");
    let raw = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => leanshell_core::config::DEFAULT_CONFIG.to_string(),
    };
    if let Ok(mut doc) = raw.parse::<DocumentMut>() {
        let mode = if enabled { "auto" } else { "off" };
        doc["auto_update"] = Item::Value(Value::from(mode));
        let _ = std::fs::write(&path, doc.to_string());
    }
}

fn set_autostart(launcher: &Path, enabled: bool) -> Result<(), String> {
    // SAFETY: Updates current-user Run entry for LeanShell autostart.
    unsafe {
        if enabled {
            let cmd = wide(&format!("\"{}\"", launcher.display()));
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                RUN_KEY,
                channel::RUN_VALUE,
                REG_SZ.0,
                Some(cmd.as_ptr().cast()),
                (cmd.len() * 2) as u32,
            )
            .ok()
            .map_err(|e| format!("Failed writing Run key: {e}"))
        } else {
            let res = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, channel::RUN_VALUE);
            if res == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                res.ok().map_err(|e| format!("Failed removing Run key: {e}"))
            }
        }
    }
}

fn write_reg_sz(root: HKEY, subkey: &HSTRING, name: PCWSTR, val: &str) -> Result<(), String> {
    let w = wide(val);
    // SAFETY: Writes null-terminated UTF-16 REG_SZ value under HKLM uninstall key.
    unsafe {
        RegSetKeyValueW(
            root,
            subkey,
            name,
            REG_SZ.0,
            Some(w.as_ptr().cast()),
            (w.len() * 2) as u32,
        )
        .ok()
        .map_err(|e| format!("Registry write failed: {e}"))
    }
}

fn write_reg_dword(root: HKEY, subkey: &HSTRING, name: PCWSTR, val: u32) -> Result<(), String> {
    // SAFETY: Writes 4-byte REG_DWORD value under HKLM uninstall key.
    unsafe {
        RegSetKeyValueW(
            root,
            subkey,
            name,
            REG_DWORD.0,
            Some((&raw const val).cast()),
            size_of::<u32>() as u32,
        )
        .ok()
        .map_err(|e| format!("Registry write failed: {e}"))
    }
}

fn format_uninstall_cmd(setup_exe: &Path, silent: bool) -> String {
    if silent {
        format!("\"{}\" --uninstall --silent", setup_exe.display())
    } else {
        format!("\"{}\" --uninstall", setup_exe.display())
    }
}

fn write_uninstall_registry(target_dir: &Path, launcher_exe: &Path, total_bytes: u64) -> Result<(), String> {
    let subkey = HSTRING::from(UNINSTALL_KEY);
    let setup_exe = target_dir.join("leanshell-setup.exe");
    write_reg_sz(HKEY_LOCAL_MACHINE, &subkey, w!("DisplayName"), channel::NAME)?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("DisplayVersion"),
        env!("CARGO_PKG_VERSION"),
    )?;
    write_reg_sz(HKEY_LOCAL_MACHINE, &subkey, w!("Publisher"), "LeanShell Contributors")?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("DisplayIcon"),
        &format!("{},0", launcher_exe.display()),
    )?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("InstallLocation"),
        &target_dir.to_string_lossy(),
    )?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("UninstallString"),
        &format_uninstall_cmd(&setup_exe, false),
    )?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("QuietUninstallString"),
        &format_uninstall_cmd(&setup_exe, true),
    )?;
    write_reg_sz(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("URLInfoAbout"),
        "https://github.com/GlitchSlayed/LeanShell",
    )?;
    write_reg_dword(HKEY_LOCAL_MACHINE, &subkey, w!("NoModify"), 1)?;
    write_reg_dword(HKEY_LOCAL_MACHINE, &subkey, w!("NoRepair"), 1)?;
    write_reg_dword(
        HKEY_LOCAL_MACHINE,
        &subkey,
        w!("EstimatedSize"),
        (total_bytes / 1024).max(1) as u32,
    )?;
    Ok(())
}

fn delete_uninstall_registry() -> Result<(), String> {
    let subkey = HSTRING::from(UNINSTALL_KEY);
    // SAFETY: Removes LeanShell's Uninstall key tree on uninstall.
    let err = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &subkey) };
    if err == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        err.ok().map_err(|e| format!("Failed removing Uninstall entry: {e}"))
    }
}

fn start_menu_programs_dir() -> Option<PathBuf> {
    std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .map(|p| p.join(r"Microsoft\Windows\Start Menu\Programs"))
}

fn write_start_menu_shortcuts(launcher: &Path, settings: &Path) {
    let Some(dir) = start_menu_programs_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    // SAFETY: Initializes COM STA for IShellLinkW shortcut creation and balances CoUninitialize.
    unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() {
            return;
        }
        create_shortcut(
            &dir.join("LeanShell.lnk"),
            launcher,
            "LeanShell desktop shell and launcher",
        );
        create_shortcut(&dir.join("LeanShell Settings.lnk"), settings, "Configure LeanShell");
        CoUninitialize();
    }
}

unsafe fn create_shortcut(lnk_path: &Path, target: &Path, desc: &str) {
    // SAFETY: Creates COM ShellLink object, sets target/workdir/description, and saves via IPersistFile.
    unsafe {
        let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else {
            return;
        };
        let target_w = wide(&target.to_string_lossy());
        let _ = link.SetPath(PCWSTR(target_w.as_ptr()));
        if let Some(work_dir) = target.parent() {
            let work_w = wide(&work_dir.to_string_lossy());
            let _ = link.SetWorkingDirectory(PCWSTR(work_w.as_ptr()));
        }
        let desc_w = wide(desc);
        let _ = link.SetDescription(PCWSTR(desc_w.as_ptr()));
        if let Ok(persist) = link.cast::<IPersistFile>() {
            let lnk_w = wide(&lnk_path.to_string_lossy());
            let _ = persist.Save(PCWSTR(lnk_w.as_ptr()), true);
        }
    }
}

fn remove_start_menu_shortcuts() {
    if let Some(dir) = start_menu_programs_dir() {
        for name in ["LeanShell.lnk", "LeanShell Settings.lnk"] {
            let _ = std::fs::remove_file(dir.join(name));
        }
    }
}

// ============================================================================
// Native Win32 + Direct2D Setup Wizard UI
// ============================================================================

#[derive(Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }

    fn rounded(self, radius: f32) -> D2D1_ROUNDED_RECT {
        D2D1_ROUNDED_RECT {
            rect: self.d2d(),
            radiusX: radius,
            radiusY: radius,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitAction {
    ToggleAutostart,
    ToggleExclusive,
    ToggleHosts,
    ToggleAutoUpdate,
    ToggleLaunch,
    Install,
    Uninstall,
    RestoreShell,
}

#[derive(Clone)]
struct Formats {
    h1: IDWriteTextFormat,
    body: IDWriteTextFormat,
    body_semi: IDWriteTextFormat,
    small: IDWriteTextFormat,
    center: IDWriteTextFormat,
    glyph: IDWriteTextFormat,
}

struct Target {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}

struct SetupWindow {
    hwnd: HWND,
    d2d: ID2D1Factory,
    formats: Formats,
    target: Option<Target>,
    dpi: f32,
    opts: InstallOptions,
    target_dir: PathBuf,
    status: String,
    is_error: bool,
    hits: Vec<(Rect, HitAction)>,
    hovered: Option<usize>,
    tracking_mouse: bool,
}

fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

impl SetupWindow {
    fn new(hwnd: HWND, dpi: f32) -> Result<Self, String> {
        // SAFETY: Creates Direct2D and DirectWrite factories on the UI thread.
        unsafe {
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(|e| e.to_string())?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| e.to_string())?;
            let fmt = |size: f32, weight: DWRITE_FONT_WEIGHT, align: DWRITE_TEXT_ALIGNMENT, family: PCWSTR| {
                let f = dw
                    .CreateTextFormat(
                        family,
                        None,
                        weight,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size,
                        w!("en-us"),
                    )
                    .map_err(|e| e.to_string())?;
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)
                    .map_err(|e| e.to_string())?;
                f.SetTextAlignment(align).map_err(|e| e.to_string())?;
                Ok::<_, String>(f)
            };
            let formats = Formats {
                h1: fmt(
                    22.0,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    w!("Segoe UI"),
                )?,
                body: fmt(
                    13.5,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    w!("Segoe UI"),
                )?,
                body_semi: fmt(
                    13.5,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    w!("Segoe UI"),
                )?,
                small: fmt(
                    11.5,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    w!("Segoe UI"),
                )?,
                center: fmt(
                    13.0,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    w!("Segoe UI"),
                )?,
                glyph: fmt(
                    15.0,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    w!("Segoe MDL2 Assets"),
                )?,
            };
            style_dark_title_bar(hwnd);
            let target_dir = install::default_install_dir();
            let status = if target_dir.join("leanshell.exe").is_file() {
                format!(
                    "Existing installation found at {}. Ready to upgrade.",
                    target_dir.display()
                )
            } else {
                format!(
                    "Ready to install LeanShell {} to {}.",
                    env!("CARGO_PKG_VERSION"),
                    target_dir.display()
                )
            };
            Ok(Self {
                hwnd,
                d2d,
                formats,
                target: None,
                dpi,
                opts: InstallOptions::default(),
                target_dir,
                status,
                is_error: false,
                hits: Vec::new(),
                hovered: None,
                tracking_mouse: false,
            })
        }
    }

    fn invalidate(&self) {
        // SAFETY: Marks valid setup HWND invalid for immediate repaint.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        if let Some(t) = &self.target {
            // SAFETY: Resizes live Direct2D HWND target to non-zero pixel bounds.
            unsafe {
                let _ = t.rt.Resize(&D2D_SIZE_U { width: w, height: h });
            }
        }
    }

    fn paint(&mut self) {
        if self.target.is_none() {
            // SAFETY: Creates software Direct2D HWND render target sized to client rect.
            let created = unsafe {
                let mut rc = RECT::default();
                let _ = GetClientRect(self.hwnd, &mut rc);
                let props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    ..Default::default()
                };
                let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd: self.hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: rc.right.max(1) as u32,
                        height: rc.bottom.max(1) as u32,
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                };
                self.d2d.CreateHwndRenderTarget(&props, &hp).and_then(|rt| {
                    rt.SetDpi(self.dpi, self.dpi);
                    rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
                    let brush = rt.CreateSolidColorBrush(&rgba(0xFFFFFF, 1.0), None)?;
                    Ok(Target { rt, brush })
                })
            };
            match created {
                Ok(t) => self.target = Some(t),
                Err(_) => return,
            }
        }

        let Some(target) = &self.target else {
            return;
        };
        let rt = target.rt.clone();
        let brush = target.brush.clone();
        let fm = &self.formats;
        self.hits.clear();

        let bg = rgba(0x12151C, 1.0);
        let card = rgba(0x1E232E, 1.0);
        let border = rgba(0x2E3545, 1.0);
        let text = rgba(0xF3F5F8, 1.0);
        let sub = rgba(0x9BA3B4, 1.0);
        let accent = rgba(0x0891B2, 1.0);
        let danger = rgba(0xDC2626, 1.0);
        let err_bg = rgba(0x451A1A, 1.0);
        let err_text = rgba(0xFCA5A5, 1.0);

        // SAFETY: Paired BeginDraw / EndDraw on valid HWND render target.
        let end_res = unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&bg));
            let size = rt.GetSize();
            let pad = 28.0;
            let cw = size.width - pad * 2.0;

            brush.SetColor(&text);
            draw_text(
                &rt,
                &format!("{} Setup (v{})", channel::NAME, env!("CARGO_PKG_VERSION")),
                &fm.h1,
                Rect {
                    x: pad,
                    y: 20.0,
                    w: cw,
                    h: 30.0,
                },
                &brush,
            );
            brush.SetColor(&sub);
            draw_text(
                &rt,
                &format!(
                    "Install folder: {}   |   100% Native Win32 + Direct2D",
                    self.target_dir.display()
                ),
                &fm.small,
                Rect {
                    x: pad,
                    y: 52.0,
                    w: cw,
                    h: 20.0,
                },
                &brush,
            );

            let items = [
                (
                    "Start LeanShell automatically at Windows sign-in",
                    self.opts.autostart,
                    HitAction::ToggleAutostart,
                ),
                (
                    "Register as Exclusive Windows 11 Shell (HKCU Winlogon)",
                    self.opts.exclusive_shell,
                    HitAction::ToggleExclusive,
                ),
                (
                    "Neuter bloated Windows Shell hosts via IFEO (reversible)",
                    self.opts.neuter_hosts,
                    HitAction::ToggleHosts,
                ),
                (
                    "Enable zero-flash background auto-updates",
                    self.opts.auto_update,
                    HitAction::ToggleAutoUpdate,
                ),
                (
                    "Launch LeanShell when setup completes",
                    self.opts.launch_after,
                    HitAction::ToggleLaunch,
                ),
            ];

            let mut y = 92.0;
            for (label, checked, action) in items {
                let r = Rect {
                    x: pad,
                    y,
                    w: cw,
                    h: 44.0,
                };
                brush.SetColor(&card);
                rt.FillRoundedRectangle(&r.rounded(8.0), &brush);
                brush.SetColor(&border);
                rt.DrawRoundedRectangle(&r.rounded(8.0), &brush, 1.0, None);

                let box_r = Rect {
                    x: r.x + 14.0,
                    y: r.y + 11.0,
                    w: 22.0,
                    h: 22.0,
                };
                brush.SetColor(if checked { &accent } else { &bg });
                rt.FillRoundedRectangle(&box_r.rounded(5.0), &brush);
                if !checked {
                    brush.SetColor(&border);
                    rt.DrawRoundedRectangle(&box_r.rounded(5.0), &brush, 1.0, None);
                } else {
                    brush.SetColor(&text);
                    draw_text(&rt, "\u{E73E}", &fm.glyph, box_r, &brush);
                }
                brush.SetColor(&text);
                draw_text(
                    &rt,
                    label,
                    &fm.body,
                    Rect {
                        x: r.x + 48.0,
                        y: r.y,
                        w: r.w - 60.0,
                        h: r.h,
                    },
                    &brush,
                );
                self.hits.push((r, action));
                y += 52.0;
            }

            let status_r = Rect {
                x: pad,
                y: y + 6.0,
                w: cw,
                h: 46.0,
            };
            brush.SetColor(if self.is_error { &err_bg } else { &card });
            rt.FillRoundedRectangle(&status_r.rounded(8.0), &brush);
            brush.SetColor(&border);
            rt.DrawRoundedRectangle(&status_r.rounded(8.0), &brush, 1.0, None);
            brush.SetColor(if self.is_error { &err_text } else { &sub });
            draw_text(
                &rt,
                &self.status,
                &fm.small,
                Rect {
                    x: status_r.x + 12.0,
                    y: status_r.y,
                    w: status_r.w - 24.0,
                    h: status_r.h,
                },
                &brush,
            );

            let btn_y = size.height - 64.0;
            let install_label = if self.target_dir.join("leanshell.exe").is_file() {
                "Upgrade / Repair"
            } else {
                "Install LeanShell"
            };
            let btn_install = Rect {
                x: size.width - pad - 170.0,
                y: btn_y,
                w: 170.0,
                h: 38.0,
            };
            let btn_uninstall = Rect {
                x: pad,
                y: btn_y,
                w: 140.0,
                h: 38.0,
            };
            let btn_restore = Rect {
                x: pad + 152.0,
                y: btn_y,
                w: 165.0,
                h: 38.0,
            };

            for (r, label, fill) in [
                (btn_install, install_label, &accent),
                (btn_uninstall, "Uninstall", &danger),
                (btn_restore, "Restore Stock Shell", &card),
            ] {
                brush.SetColor(fill);
                rt.FillRoundedRectangle(&r.rounded(8.0), &brush);
                if fill.r == card.r {
                    brush.SetColor(&border);
                    rt.DrawRoundedRectangle(&r.rounded(8.0), &brush, 1.0, None);
                }
                brush.SetColor(&text);
                draw_text(&rt, label, &fm.center, r, &brush);
            }
            self.hits.push((btn_install, HitAction::Install));
            self.hits.push((btn_uninstall, HitAction::Uninstall));
            self.hits.push((btn_restore, HitAction::RestoreShell));

            brush.SetColor(&sub);
            let _ = &fm.body_semi;
            rt.EndDraw(None, None)
        };
        if let Err(e) = end_res
            && e.code() == D2DERR_RECREATE_TARGET
        {
            self.target = None;
            self.invalidate();
        }
    }

    fn handle_click(&mut self, dx: f32, dy: f32) {
        let Some(action) = self
            .hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(dx, dy))
            .map(|(_, a)| *a)
        else {
            return;
        };
        match action {
            HitAction::ToggleAutostart => self.opts.autostart = !self.opts.autostart,
            HitAction::ToggleExclusive => self.opts.exclusive_shell = !self.opts.exclusive_shell,
            HitAction::ToggleHosts => self.opts.neuter_hosts = !self.opts.neuter_hosts,
            HitAction::ToggleAutoUpdate => self.opts.auto_update = !self.opts.auto_update,
            HitAction::ToggleLaunch => self.opts.launch_after = !self.opts.launch_after,
            HitAction::Install => {
                self.is_error = false;
                self.status = "Installing LeanShell...".into();
                self.paint();
                match perform_install(&self.target_dir, self.opts) {
                    Ok(()) => {
                        self.is_error = false;
                        self.status = format!(
                            "Installed LeanShell {} to {}.{}",
                            env!("CARGO_PKG_VERSION"),
                            self.target_dir.display(),
                            if self.opts.exclusive_shell {
                                " Sign out and back in to boot directly into LeanShell."
                            } else {
                                ""
                            }
                        );
                    }
                    Err(e) => {
                        self.is_error = true;
                        self.status = format!("Install error: {e}");
                    }
                }
            }
            HitAction::Uninstall => {
                self.is_error = false;
                match perform_uninstall(&self.target_dir) {
                    Ok(()) => {
                        self.status = "Uninstalled LeanShell and restored stock Windows shell.".into();
                    }
                    Err(e) => {
                        self.is_error = true;
                        self.status = format!("Uninstall error: {e}");
                    }
                }
            }
            HitAction::RestoreShell => {
                self.is_error = false;
                let exe = self.target_dir.join("leanshell.exe");
                if exe.is_file() {
                    let _ = std::process::Command::new(&exe)
                        .arg("--restore-shell")
                        .creation_flags(CREATE_NO_WINDOW)
                        .status();
                } else {
                    let _ = leanshell_platform_win::unregister_exclusive_shell();
                    let _ = leanshell_platform_win::restore_shell_hosts();
                }
                self.status = "Restored stock Windows Explorer shell, taskbar, and IFEO host entries.".into();
            }
        }
        self.invalidate();
    }
}

unsafe fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    text: &str,
    fmt: &IDWriteTextFormat,
    r: Rect,
    brush: &ID2D1SolidColorBrush,
) {
    // SAFETY: Caller has active BeginDraw on rt.
    unsafe {
        rt.DrawText(
            &wide(text),
            fmt,
            &r.d2d(),
            brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

fn style_dark_title_bar(hwnd: HWND) {
    let bgr = |rgb: u32| ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff);
    // SAFETY: Sets immersive dark caption colors on owned Win32 HWND.
    unsafe {
        let set = |attr: DWMWINDOWATTRIBUTE, val: u32| {
            let _ = DwmSetWindowAttribute(hwnd, attr, (&raw const val).cast(), size_of::<u32>() as u32);
        };
        set(DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
        set(DWMWA_CAPTION_COLOR, bgr(0x12151C));
        set(DWMWA_BORDER_COLOR, bgr(0x12151C));
        set(DWMWA_TEXT_COLOR, bgr(0xF3F5F8));
    }
}

fn run_gui() -> Result<(), String> {
    // SAFETY: Registers window class, creates centered Direct2D setup window, and runs Win32 message loop.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let fallback = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let h_icon = LoadImageW(
            Some(instance.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            32,
            32,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .filter(|h| !h.is_invalid())
        .map(|h| HICON(h.0))
        .unwrap_or(fallback);
        let h_icon_sm = LoadImageW(
            Some(instance.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR,
        )
        .ok()
        .filter(|h| !h.is_invalid())
        .map(|h| HICON(h.0))
        .unwrap_or(fallback);

        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(setup_wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hIcon: h_icon,
            hIconSm: h_icon_sm,
            lpszClassName: SETUP_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err("RegisterClassExW failed".into());
        }

        let title = wide(&format!("{} Setup", channel::NAME));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            SETUP_CLASS,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            700,
            540,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| e.to_string())?;

        let dpi = GetDpiForWindow(hwnd).max(96) as f32;
        let scale = dpi / 96.0;
        let w = (WIN_W_DIP * scale).round() as i32;
        let h = (WIN_H_DIP * scale).round() as i32;
        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let x = work.left + ((work.right - work.left - w) / 2).max(0);
        let y = work.top + ((work.bottom - work.top - h) / 2).max(0);
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);

        let state = Box::new(SetupWindow::new(hwnd, dpi)?);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(state) as isize);
        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn setup_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCDESTROY {
        // SAFETY: Drops heap-allocated SetupWindow exactly once on destroy.
        unsafe {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut SetupWindow;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }

    // SAFETY: GWLP_USERDATA owns SetupWindow for the lifetime of this HWND.
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SetupWindow };
    let Some(app) = (unsafe { ptr.as_mut() }) else {
        // SAFETY: Default window procedure prior to attachment.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };

    match msg {
        WM_SIZE => {
            app.resize((lparam.0 & 0xFFFF) as u32, ((lparam.0 >> 16) & 0xFFFF) as u32);
            app.invalidate();
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: Paired BeginPaint / EndPaint around Direct2D render cycle.
            unsafe {
                let _ = BeginPaint(hwnd, &mut ps);
            }
            app.paint();
            // SAFETY: Matches BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let scale = 96.0 / app.dpi;
            app.handle_click(
                (lparam.0 as i16) as f32 * scale,
                ((lparam.0 >> 16) as i16) as f32 * scale,
            );
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !app.tracking_mouse {
                app.tracking_mouse = true;
                // SAFETY: Registers one-shot TME_LEAVE on setup window.
                unsafe {
                    let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    });
                }
            }
            let scale = 96.0 / app.dpi;
            let (dx, dy) = (
                (lparam.0 as i16) as f32 * scale,
                ((lparam.0 >> 16) as i16) as f32 * scale,
            );
            let next = app.hits.iter().rposition(|(r, _)| r.contains(dx, dy));
            if next != app.hovered {
                app.hovered = next;
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            app.tracking_mouse = false;
            app.hovered = None;
            app.invalidate();
            LRESULT(0)
        }
        _ => {
            // SAFETY: Standard Win32 default handling.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installer_cli_modes() {
        assert_eq!(parse_cli(&["leanshell-setup.exe".into()]), CliMode::Gui);
        assert_eq!(
            parse_cli(&["leanshell-setup.exe".into(), "--silent".into()]),
            CliMode::SilentInstall { launch: true }
        );
        assert_eq!(
            parse_cli(&["leanshell-setup.exe".into(), "/S".into(), "--no-launch".into()]),
            CliMode::SilentInstall { launch: false }
        );
        assert_eq!(
            parse_cli(&["leanshell-setup.exe".into(), "--uninstall".into(), "--silent".into()]),
            CliMode::Uninstall { silent: true }
        );
        assert_eq!(
            parse_cli(&[
                "leanshell-setup.exe".into(),
                "--apply-staged-update".into(),
                r"C:\Temp\stage".into(),
                "--target-dir".into(),
                r"C:\Program Files\LeanShell".into(),
            ]),
            CliMode::ApplyStaged {
                stage_dir: PathBuf::from(r"C:\Temp\stage"),
                target_dir: PathBuf::from(r"C:\Program Files\LeanShell"),
            }
        );
    }

    #[test]
    fn formats_installed_apps_uninstall_commands() {
        let exe = Path::new(r"C:\Program Files\LeanShell\leanshell-setup.exe");
        assert_eq!(
            format_uninstall_cmd(exe, false),
            r#""C:\Program Files\LeanShell\leanshell-setup.exe" --uninstall"#
        );
        assert_eq!(
            format_uninstall_cmd(exe, true),
            r#""C:\Program Files\LeanShell\leanshell-setup.exe" --uninstall --silent"#
        );
    }
}
