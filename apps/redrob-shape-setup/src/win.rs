//! The Win32 window: a borderless, DPI-aware, dark card after the Redrob design system (deep
//! ground `#0A0B0C`, muted grey text, a white pill call to action, Redrob Blue `#2B52FF` progress).
//! States: Welcome -> Installing -> Done (or Error), and for `--uninstall` Confirm -> Removing ->
//! Removed. The work runs on a thread and reports progress through `Shared`; a 30 ms timer
//! repaints. Everything is drawn by hand with GDI so there is nothing to theme.

#![allow(clippy::missing_safety_doc)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::*;
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_LocalAppData, FOLDERID_Programs, SHGetKnownFolderPath, ShellExecuteW};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::install::{self, Layout, PRODUCT, PUBLISHER, UNINSTALL_KEY, VERSION};

const W: i32 = 520;
const H: i32 = 320;
const GROUND: COLORREF = rgb(0x0A, 0x0B, 0x0C);
const TEXT: COLORREF = rgb(0xE6, 0xE7, 0xEA);
const MUTED: COLORREF = rgb(0x8A, 0x8F, 0x98);
const TRACK: COLORREF = rgb(0x1C, 0x1E, 0x22);
const BLUE: COLORREF = rgb(0x2B, 0x52, 0xFF);
const WHITE: COLORREF = rgb(0xFF, 0xFF, 0xFF);
const BLACK: COLORREF = rgb(0x0A, 0x0B, 0x0C);
const ERROR: COLORREF = rgb(0xFF, 0x5A, 0x5A);
const TIMER_ID: usize = 1;

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    r as u32 | (g as u32) << 8 | (b as u32) << 16
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Install,
    Uninstall,
}

#[derive(Clone, Debug, PartialEq)]
enum Screen {
    Welcome,
    Working,
    Done,
    Error(String),
}

/// Progress the worker thread writes and the window reads.
#[derive(Default)]
struct Shared {
    progress: f32,
    label: String,
    finished: Option<Result<(), String>>,
}

struct App {
    mode: Mode,
    demo: bool,
    screen: Screen,
    desktop_shortcut: bool,
    delete_prefs: bool,
    shown_progress: f32,
    shared: Arc<Mutex<Shared>>,
    hwnd: HWND,
    dpi: u32,
    hover: Option<Hit>,
    layout: Option<Layout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Primary,
    Secondary,
    Close,
    Checkbox,
    Caption,
}

static APP: OnceLock<Mutex<App>> = OnceLock::new();

fn app() -> std::sync::MutexGuard<'static, App> {
    APP.get().expect("app").lock().unwrap_or_else(|e| e.into_inner())
}

/// Run the window; returns the process exit code.
pub fn run(mode: Mode, demo: bool) -> i32 {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let layout = if demo { None } else { known_layout() };
        if layout.is_none() && !demo {
            message_box("Could not find the user folders (LOCALAPPDATA).", true);
            return 2;
        }
        let _ = APP.set(Mutex::new(App {
            mode,
            demo,
            screen: Screen::Welcome,
            desktop_shortcut: false,
            delete_prefs: false,
            shown_progress: 0.0,
            shared: Arc::new(Mutex::new(Shared::default())),
            hwnd: std::ptr::null_mut(),
            dpi: 96,
            hover: None,
            layout,
        }));
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("RedrobShapeSetup");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: LoadIconW(instance, 1 as _),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        RegisterClassW(&wc);
        let title = wide(if mode == Mode::Install { "Redrob Shape Setup" } else { "Uninstall Redrob Shape" });
        let sw = GetSystemMetrics(SM_CXSCREEN);
        let sh = GetSystemMetrics(SM_CYSCREEN);
        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            class.as_ptr(),
            title.as_ptr(),
            WS_POPUP | WS_MINIMIZEBOX | WS_SYSMENU,
            (sw - W) / 2,
            (sh - H) / 2,
            W,
            H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return 3;
        }
        let dpi = GetDpiForWindow(hwnd).max(96);
        let (w, h) = (W * dpi as i32 / 96, H * dpi as i32 / 96);
        SetWindowPos(hwnd, std::ptr::null_mut(), (sw - w) / 2, (sh - h) / 2, w, h, SWP_NOZORDER);
        let round = DWMWCP_ROUND;
        DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE as u32, &round as *const _ as *const _, 4);
        {
            let mut a = app();
            a.hwnd = hwnd;
            a.dpi = dpi;
        }
        ShowWindow(hwnd, SW_SHOW);
        SetTimer(hwnd, TIMER_ID, 30, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam as i32
    }
}

unsafe fn known_folder(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    unsafe {
        let mut p: windows_sys::core::PWSTR = std::ptr::null_mut();
        if SHGetKnownFolderPath(id, 0, std::ptr::null_mut(), &mut p) != 0 || p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p as *const _);
        Some(PathBuf::from(s))
    }
}

unsafe fn known_layout() -> Option<Layout> {
    unsafe {
        let lad = known_folder(&FOLDERID_LocalAppData)?;
        let programs = known_folder(&FOLDERID_Programs)?;
        let desktop = known_folder(&FOLDERID_Desktop).unwrap_or_else(|| lad.clone());
        Some(Layout::new(&lad, &programs, &desktop))
    }
}

unsafe fn message_box(text: &str, error: bool) {
    unsafe {
        MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide(PRODUCT).as_ptr(), if error { MB_ICONERROR } else { MB_ICONINFORMATION });
    }
}

// ---- the work -------------------------------------------------------------------------------

fn start_work() {
    let (mode, demo, layout, shared, desktop_shortcut, delete_prefs) = {
        let mut a = app();
        a.screen = Screen::Working;
        a.shown_progress = 0.0;
        *a.shared.lock().unwrap_or_else(|e| e.into_inner()) = Shared::default();
        (a.mode, a.demo, a.layout.clone(), a.shared.clone(), a.desktop_shortcut, a.delete_prefs)
    };
    std::thread::spawn(move || {
        let report_to = shared.clone();
        let mut report = move |p: f32, label: &str| {
            let mut s = report_to.lock().unwrap_or_else(|e| e.into_inner());
            s.progress = p;
            s.label = label.to_string();
        };
        let result = if demo {
            demo_work(mode, &mut report)
        } else {
            match (mode, layout) {
                (Mode::Install, Some(layout)) => do_install(&layout, desktop_shortcut, &mut report),
                (Mode::Uninstall, Some(layout)) => do_uninstall(&layout, delete_prefs, &mut report),
                _ => Err("no folders".into()),
            }
        };
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        s.progress = 1.0;
        s.finished = Some(result);
    });
}

fn demo_work(mode: Mode, report: &mut dyn FnMut(f32, &str)) -> Result<(), String> {
    let labels: &[&str] = match mode {
        Mode::Install => &["Preparing", "Installing Redrob Shape", "Registering"],
        Mode::Uninstall => &["Removing Redrob Shape", "Removing Redrob Shape", "Cleaning up"],
    };
    for i in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(40));
        report(i as f32 / 60.0, labels[(i * labels.len() / 60).min(labels.len() - 1)]);
    }
    Ok(())
}

fn do_install(layout: &Layout, desktop_shortcut: bool, report: &mut dyn FnMut(f32, &str)) -> Result<(), String> {
    if !install::has_payload() {
        return Err("This setup was built without Redrob Shape inside it.".into());
    }
    let self_exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bytes = install::install_files(layout, &self_exe, report).map_err(|e| format!("Could not write files: {e}"))?;
    report(0.85, "Registering");
    write_uninstall_key(layout, bytes).map_err(|e| format!("Could not register the app: {e}"))?;
    report(0.9, "Creating shortcuts");
    create_shortcut(&layout.start_menu_link(), &layout.app_exe(), &layout.app_dir);
    if desktop_shortcut {
        create_shortcut(&layout.desktop_link(), &layout.app_exe(), &layout.app_dir);
    }
    report(1.0, "Done");
    Ok(())
}

fn do_uninstall(layout: &Layout, delete_prefs: bool, report: &mut dyn FnMut(f32, &str)) -> Result<(), String> {
    install::remove_files(layout, report).map_err(|e| format!("Could not remove files: {e}"))?;
    if delete_prefs && let Some(cfg) = std::env::var_os("APPDATA") {
        let _ = std::fs::remove_dir_all(Path::new(&cfg).join("redrob-shape"));
    }
    report(0.9, "Cleaning up");
    unsafe {
        let key = wide(&format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{UNINSTALL_KEY}"));
        RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
    }
    // This exe is the uninstaller: a hidden PowerShell deletes it once the window has gone.
    if let Ok(me) = std::env::current_exe() {
        let script = format!(
            "Start-Sleep -Seconds 2; Remove-Item -LiteralPath '{}' -Force -ErrorAction SilentlyContinue; $d='{}'; if ((Get-ChildItem -LiteralPath $d -Force | Measure-Object).Count -eq 0) {{ Remove-Item -LiteralPath $d -Force }}",
            me.display().to_string().replace('\'', "''"),
            layout.uninstall_dir.display().to_string().replace('\'', "''")
        );
        hidden_powershell(&script);
    }
    report(1.0, "Removed");
    Ok(())
}

fn hidden_powershell(script: &str) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// A `.lnk` through the Shell COM object; the one place PowerShell is simpler than COM from Rust.
fn create_shortcut(link: &Path, target: &Path, workdir: &Path) {
    let q = |p: &Path| p.display().to_string().replace('\'', "''");
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.IconLocation='{},0'; $s.Description='{}'; $s.Save()",
        q(link),
        q(target),
        q(workdir),
        q(target),
        PRODUCT
    );
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

fn write_uninstall_key(layout: &Layout, bytes: u64) -> Result<(), String> {
    unsafe {
        let sub = wide(&format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{UNINSTALL_KEY}"));
        let mut key: HKEY = std::ptr::null_mut();
        let err = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_ALL_ACCESS,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        );
        if err != 0 {
            return Err(format!("registry error {err}"));
        }
        let set_sz = |name: &str, value: &str| {
            let n = wide(name);
            let v = wide(value);
            RegSetValueExW(key, n.as_ptr(), 0, REG_SZ, v.as_ptr() as *const u8, (v.len() * 2) as u32);
        };
        let set_dw = |name: &str, value: u32| {
            let n = wide(name);
            RegSetValueExW(key, n.as_ptr(), 0, REG_DWORD, &value as *const u32 as *const u8, 4);
        };
        set_sz("DisplayName", PRODUCT);
        set_sz("DisplayVersion", VERSION);
        set_sz("Publisher", PUBLISHER);
        set_sz("InstallLocation", &layout.app_dir.display().to_string());
        set_sz("DisplayIcon", &layout.app_exe().display().to_string());
        set_sz("UninstallString", &install::uninstall_command(layout));
        set_sz("URLInfoAbout", "https://github.com/redrob-labs/redrob-shape");
        set_dw("NoModify", 1);
        set_dw("NoRepair", 1);
        set_dw("EstimatedSize", install::estimated_size_kb(bytes));
        RegCloseKey(key);
    }
    Ok(())
}

fn launch_app() {
    let Some(layout) = app().layout.clone() else { return };
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            wide(&layout.app_exe().display().to_string()).as_ptr(),
            std::ptr::null(),
            wide(&layout.app_dir.display().to_string()).as_ptr(),
            SW_SHOWNORMAL as i32,
        );
    }
}

// ---- geometry ------------------------------------------------------------------------------

struct Geometry {
    scale: f32,
    client: RECT,
}

impl Geometry {
    fn px(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }
    fn rect(&self, x: i32, y: i32, w: i32, h: i32) -> RECT {
        RECT { left: self.px(x), top: self.px(y), right: self.px(x + w), bottom: self.px(y + h) }
    }
    fn close(&self) -> RECT {
        self.rect(W - 44, 8, 36, 32)
    }
    fn primary(&self) -> RECT {
        self.rect(W - 32 - 160, H - 32 - 40, 160, 40)
    }
    fn secondary(&self) -> RECT {
        self.rect(W - 32 - 160 - 16 - 110, H - 32 - 40, 110, 40)
    }
    fn checkbox(&self) -> RECT {
        self.rect(32, H - 32 - 40 + 10, 20, 20)
    }
    fn caption(&self) -> RECT {
        RECT { left: 0, top: 0, right: self.client.right, bottom: self.px(48) }
    }
}

fn contains(r: &RECT, p: POINT) -> bool {
    p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom
}

fn hit_test(a: &App, g: &Geometry, p: POINT) -> Option<Hit> {
    if contains(&g.close(), p) {
        return Some(Hit::Close);
    }
    let (primary, secondary, checkbox) = buttons(a);
    if primary.is_some() && contains(&g.primary(), p) {
        return Some(Hit::Primary);
    }
    if secondary.is_some() && contains(&g.secondary(), p) {
        return Some(Hit::Secondary);
    }
    if checkbox.is_some() {
        let mut r = g.checkbox();
        r.right += g.px(260);
        if contains(&r, p) {
            return Some(Hit::Checkbox);
        }
    }
    if contains(&g.caption(), p) {
        return Some(Hit::Caption);
    }
    None
}

/// Labels of the primary button, the secondary button and the checkbox for the current screen.
fn buttons(a: &App) -> (Option<&'static str>, Option<&'static str>, Option<&'static str>) {
    match (&a.screen, a.mode) {
        (Screen::Welcome, Mode::Install) => (Some("Install"), Some("Cancel"), Some("Also add a desktop shortcut")),
        (Screen::Welcome, Mode::Uninstall) => (Some("Uninstall"), Some("Cancel"), Some("Also delete my preferences")),
        (Screen::Working, _) => (None, None, None),
        (Screen::Done, Mode::Install) => (Some("Open Redrob Shape"), Some("Close"), None),
        (Screen::Done, Mode::Uninstall) => (Some("Close"), None, None),
        (Screen::Error(_), _) => (Some("Retry"), Some("Close"), None),
    }
}

fn texts(a: &App) -> (String, String) {
    let demo = if a.demo { " (demo)" } else { "" };
    match (&a.screen, a.mode) {
        (Screen::Welcome, Mode::Install) => (
            format!("Install {PRODUCT}{demo}"),
            format!(
                "Version {VERSION}. Installs for you only, into your local app data, with no administrator prompt. Blender-style 3D modelling, shading and rendering, all in one window."
            ),
        ),
        (Screen::Welcome, Mode::Uninstall) => {
            (format!("Uninstall {PRODUCT}{demo}"), "Removes the app and its Start menu entry. Your files stay where you saved them.".into())
        }
        (Screen::Working, Mode::Install) => (format!("Installing {PRODUCT}"), "This takes a moment.".into()),
        (Screen::Working, Mode::Uninstall) => (format!("Removing {PRODUCT}"), "This takes a moment.".into()),
        (Screen::Done, Mode::Install) => {
            (format!("{PRODUCT} is ready"), "Find it in the Start menu any time. Uninstall from Settings > Apps.".into())
        }
        (Screen::Done, Mode::Uninstall) => (format!("{PRODUCT} was removed"), "Thanks for trying it.".into()),
        (Screen::Error(e), _) => ("Something went wrong".into(), e.clone()),
    }
}

// ---- painting ------------------------------------------------------------------------------

unsafe fn font(g: &Geometry, pt: i32, weight: i32) -> HFONT {
    unsafe {
        let face = wide("Segoe UI");
        CreateFontW(-(pt as f32 * g.scale * 96.0 / 72.0).round() as i32, 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr())
    }
}

unsafe fn text(hdc: HDC, s: &str, mut r: RECT, color: COLORREF, f: HFONT, flags: u32) {
    unsafe {
        let old = SelectObject(hdc, f as HGDIOBJ);
        SetTextColor(hdc, color);
        let w = wide(s);
        DrawTextW(hdc, w.as_ptr(), -1, &mut r, flags);
        SelectObject(hdc, old);
    }
}

unsafe fn pill(hdc: HDC, r: RECT, fill: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(fill);
        let pen = CreatePen(PS_SOLID, 1, fill);
        let ob = SelectObject(hdc, brush as HGDIOBJ);
        let op = SelectObject(hdc, pen as HGDIOBJ);
        let radius = r.bottom - r.top;
        RoundRect(hdc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(hdc, ob);
        SelectObject(hdc, op);
        DeleteObject(brush as HGDIOBJ);
        DeleteObject(pen as HGDIOBJ);
    }
}

unsafe fn paint(a: &App, hwnd: HWND) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let screen = BeginPaint(hwnd, &mut ps);
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);
        let g = Geometry { scale: a.dpi as f32 / 96.0, client };
        let (cw, ch) = (client.right - client.left, client.bottom - client.top);
        let hdc = CreateCompatibleDC(screen);
        let bmp = CreateCompatibleBitmap(screen, cw, ch);
        let old_bmp = SelectObject(hdc, bmp as HGDIOBJ);
        SetBkMode(hdc, TRANSPARENT as i32);

        let ground = CreateSolidBrush(GROUND);
        FillRect(hdc, &client, ground);
        DeleteObject(ground as HGDIOBJ);
        // Foot light: a thin brand-blue line fading at the bottom edge.
        for i in 0..6 {
            let shade = 0x2B * (6 - i) / 6;
            let b = CreateSolidBrush(rgb(shade as u8 / 3, (0x52 * (6 - i) / 6) as u8 / 3, (0xFF * (6 - i) / 6) as u8 / 2));
            let r = RECT { left: 0, top: ch - g.px(6 - i), right: cw, bottom: ch - g.px(5 - i) };
            FillRect(hdc, &r, b);
            DeleteObject(b as HGDIOBJ);
        }

        let title_f = font(&g, 10, 600);
        let h1 = font(&g, 20, 600);
        let body = font(&g, 10, 400);
        let small = font(&g, 9, 400);

        // Caption strip: product mark + name, close glyph.
        pill(hdc, g.rect(32, 18, 12, 12), BLUE);
        text(hdc, "Redrob", g.rect(52, 12, 200, 24), MUTED, title_f, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        let close = g.close();
        let close_color = if a.hover == Some(Hit::Close) { TEXT } else { MUTED };
        text(hdc, "\u{2715}", close, close_color, body, DT_CENTER | DT_SINGLELINE | DT_VCENTER);

        let (heading, sub) = texts(a);
        text(hdc, &heading, g.rect(32, 64, W - 64, 40), TEXT, h1, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        let sub_color = if matches!(a.screen, Screen::Error(_)) { ERROR } else { MUTED };
        text(hdc, &sub, g.rect(32, 108, W - 64, 70), sub_color, body, DT_LEFT | DT_WORDBREAK);

        if a.screen == Screen::Working {
            let track = g.rect(32, 196, W - 64, 6);
            pill(hdc, track, TRACK);
            let mut fill = track;
            let width = ((track.right - track.left) as f32 * a.shown_progress.clamp(0.0, 1.0)) as i32;
            fill.right = fill.left + width.max(if width > 0 { track.bottom - track.top } else { 0 });
            if width > 0 {
                pill(hdc, fill, BLUE);
            }
            let label = a.shared.lock().map(|s| s.label.clone()).unwrap_or_default();
            text(hdc, &label, g.rect(32, 210, W - 64, 24), MUTED, small, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        }

        let (primary, secondary, checkbox) = buttons(a);
        if let Some(label) = primary {
            let r = g.primary();
            let fill = if a.hover == Some(Hit::Primary) { rgb(0xE6, 0xE7, 0xEA) } else { WHITE };
            pill(hdc, r, fill);
            text(hdc, label, r, BLACK, title_f, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        }
        if let Some(label) = secondary {
            let r = g.secondary();
            let color = if a.hover == Some(Hit::Secondary) { TEXT } else { MUTED };
            text(hdc, label, r, color, title_f, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        }
        if let Some(label) = checkbox {
            let r = g.checkbox();
            let checked = match a.mode {
                Mode::Install => a.desktop_shortcut,
                Mode::Uninstall => a.delete_prefs,
            };
            let border = CreatePen(PS_SOLID, g.px(1).max(1), MUTED);
            let fill = CreateSolidBrush(if checked { BLUE } else { GROUND });
            let op = SelectObject(hdc, border as HGDIOBJ);
            let ob = SelectObject(hdc, fill as HGDIOBJ);
            RoundRect(hdc, r.left, r.top, r.right, r.bottom, g.px(5), g.px(5));
            SelectObject(hdc, op);
            SelectObject(hdc, ob);
            DeleteObject(border as HGDIOBJ);
            DeleteObject(fill as HGDIOBJ);
            if checked {
                text(hdc, "\u{2713}", r, WHITE, small, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
            }
            let mut lr = r;
            lr.left = r.right + g.px(10);
            lr.right = lr.left + g.px(260);
            text(hdc, label, lr, MUTED, small, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
        }
        text(
            hdc,
            &format!("{PUBLISHER} · {VERSION}"),
            g.rect(32, H - 32 - 40 + 48, 200, 16),
            rgb(0x4A, 0x4E, 0x56),
            small,
            DT_LEFT | DT_SINGLELINE | DT_VCENTER,
        );

        for f in [title_f, h1, body, small] {
            DeleteObject(f as HGDIOBJ);
        }
        BitBlt(screen, 0, 0, cw, ch, hdc, 0, 0, SRCCOPY);
        SelectObject(hdc, old_bmp);
        DeleteObject(bmp as HGDIOBJ);
        DeleteDC(hdc);
        EndPaint(hwnd, &ps);
    }
}

// ---- messages ------------------------------------------------------------------------------

fn pointer(hwnd: HWND) -> POINT {
    unsafe {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        ScreenToClient(hwnd, &mut p);
        p
    }
}

fn geometry(a: &App, hwnd: HWND) -> Geometry {
    unsafe {
        let mut client: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut client);
        Geometry { scale: a.dpi as f32 / 96.0, client }
    }
}

fn on_click(hwnd: HWND, hit: Option<Hit>) {
    let action = {
        let mut a = app();
        match hit {
            Some(Hit::Close) => Some("close"),
            Some(Hit::Checkbox) => {
                match a.mode {
                    Mode::Install => a.desktop_shortcut = !a.desktop_shortcut,
                    Mode::Uninstall => a.delete_prefs = !a.delete_prefs,
                }
                None
            }
            Some(Hit::Primary) => match (&a.screen, a.mode) {
                (Screen::Welcome, _) | (Screen::Error(_), _) => Some("start"),
                (Screen::Done, Mode::Install) => Some("launch"),
                (Screen::Done, Mode::Uninstall) => Some("close"),
                _ => None,
            },
            Some(Hit::Secondary) => match a.screen {
                Screen::Working => None,
                _ => Some("close"),
            },
            _ => None,
        }
    };
    match action {
        Some("start") => start_work(),
        Some("launch") => {
            if !app().demo {
                launch_app();
            }
            unsafe { DestroyWindow(hwnd) };
        }
        Some("close") => {
            let working = app().screen == Screen::Working;
            if working {
                let ok = unsafe {
                    MessageBoxW(hwnd, wide("Setup is still working. Close anyway?").as_ptr(), wide(PRODUCT).as_ptr(), MB_YESNO | MB_ICONWARNING)
                } == IDYES;
                if !ok {
                    return;
                }
            }
            unsafe { DestroyWindow(hwnd) };
        }
        _ => {}
    }
    unsafe { InvalidateRect(hwnd, std::ptr::null(), 0) };
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let a = app();
                paint(&a, hwnd);
                0
            }
            WM_ERASEBKGND => 1,
            WM_TIMER => {
                let mut a = app();
                if a.screen == Screen::Working {
                    let (target, finished) = {
                        let s = a.shared.lock().unwrap_or_else(|e| e.into_inner());
                        (s.progress, s.finished.clone())
                    };
                    // Eased ramp towards the reported progress; snap when the work is done.
                    a.shown_progress += (target - a.shown_progress) * 0.15;
                    if let Some(result) = finished
                        && a.shown_progress > 0.98
                    {
                        a.screen = match result {
                            Ok(()) => Screen::Done,
                            Err(e) => Screen::Error(e),
                        };
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                0
            }
            WM_MOUSEMOVE => {
                let mut a = app();
                let g = geometry(&a, hwnd);
                let hit = hit_test(&a, &g, pointer(hwnd)).filter(|h| *h != Hit::Caption);
                if hit != a.hover {
                    a.hover = hit;
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                0
            }
            WM_NCHITTEST => {
                let a = app();
                let g = geometry(&a, hwnd);
                let mut p = POINT { x: (lparam & 0xffff) as i16 as i32, y: ((lparam >> 16) & 0xffff) as i16 as i32 };
                ScreenToClient(hwnd, &mut p);
                match hit_test(&a, &g, p) {
                    Some(Hit::Caption) => HTCAPTION as LRESULT,
                    _ => HTCLIENT as LRESULT,
                }
            }
            WM_LBUTTONUP => {
                let hit = {
                    let a = app();
                    let g = geometry(&a, hwnd);
                    hit_test(&a, &g, pointer(hwnd))
                };
                on_click(hwnd, hit);
                0
            }
            WM_KEYDOWN => {
                match wparam as u16 {
                    0x1B => on_click(hwnd, Some(Hit::Close)),   // Esc
                    0x0D => on_click(hwnd, Some(Hit::Primary)), // Enter
                    _ => {}
                }
                0
            }
            WM_DPICHANGED => {
                {
                    let mut a = app();
                    a.dpi = (wparam & 0xffff) as u32;
                }
                let r = &*(lparam as *const RECT);
                SetWindowPos(hwnd, std::ptr::null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                0
            }
            WM_CLOSE => {
                on_click(hwnd, Some(Hit::Close));
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, TIMER_ID);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
