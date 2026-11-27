//! LiveVahdin oma ilmoitusikkuna ja äänet. Ei riipu Windowsin ilmoitusasetuksista.

use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};

pub const POPUP_W: f64 = 400.0;
const MARGIN: f64 = 6.0;

#[derive(Clone, Serialize, Default)]
pub struct PopupItem {
    pub name: String,
    pub login: String,
    pub title: String,
    pub game: String,
    pub viewers: Option<u64>,
    pub image: String,
    pub secs: u64,
    /// "" = live-ilmoitus, "clip" = klippi tehty
    pub kind: String,
    pub clip_url: String,
    pub edit_url: String,
}

/// Valittavat sisäänrakennetut äänet → tiedosto C:\Windows\Media -kansiossa.
pub fn builtin_sound(key: &str) -> Option<&'static str> {
    Some(match key {
        "win:Default" => "Windows Notify System Generic.wav",
        "win:Messaging" | "win:IM" => "Windows Notify Messaging.wav",
        "win:Email" | "win:Mail" => "Windows Notify Email.wav",
        "win:Calendar" | "win:Reminder" => "Windows Notify Calendar.wav",
        "win:Nudge" | "win:SMS" => "Windows Message Nudge.wav",
        "win:Proximity" => "Windows Proximity Notification.wav",
        "win:Chimes" => "chimes.wav",
        "win:Tada" => "tada.wav",
        _ => return None,
    })
}

pub fn play_sound(choice: &str) {
    let path = if let Some(f) = choice.strip_prefix("file:") {
        f.to_string()
    } else if let Some(f) = builtin_sound(choice) {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
        format!("{windir}\\Media\\{f}")
    } else {
        return; // "none" tai tuntematon
    };
    std::thread::spawn(move || {
        let Ok((_stream, handle)) = rodio::OutputStream::try_default() else { return };
        let Ok(sink) = rodio::Sink::try_new(&handle) else { return };
        let Ok(f) = std::fs::File::open(&path) else { return };
        let Ok(src) = rodio::Decoder::new(std::io::BufReader::new(f)) else { return };
        sink.append(src);
        sink.sleep_until_end();
    });
}

pub fn create_popup(app: &AppHandle) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, "popup", WebviewUrl::App("popup.html".into()))
        .title("Chilly-ilmoitus")
        .additional_browser_args(crate::viewer::browser_args())
        .inner_size(POPUP_W, 140.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focusable(false)
        .focused(false)
        .visible(false)
        .build()?;
    Ok(())
}

/// Kertoo ilmoitusikkunalle, että jonossa on uusia ilmoituksia.
pub fn poke(app: &AppHandle) {
    let _ = app.emit_to("popup", "popup-poke", ());
}

/// Asettaa ilmoitusikkunan korkeuden ja sijoittaa sen työpöydän oikeaan alakulmaan.
/// Korkeus 0 piilottaa ikkunan.
pub fn layout(app: &AppHandle, height: f64) {
    let Some(w) = app.get_webview_window("popup") else { return };
    if height <= 0.0 {
        hide_raw(&w);
        return;
    }
    let mon = w.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    let Some(mon) = mon else { return };
    let scale = mon.scale_factor();
    let wa = mon.work_area();
    let _ = w.set_size(LogicalSize::new(POPUP_W, height));
    let x = wa.position.x + wa.size.width as i32 - ((POPUP_W + MARGIN) * scale) as i32;
    let y = wa.position.y + wa.size.height as i32 - ((height + MARGIN) * scale) as i32;
    let _ = w.set_position(PhysicalPosition::new(x, y));
    show_no_activate(&w);
}

#[cfg(windows)]
fn show_no_activate(w: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if let Ok(h) = w.hwnd() {
        let h = h.0 as windows_sys::Win32::Foundation::HWND;
        unsafe {
            ShowWindow(h, SW_SHOWNOACTIVATE);
            SetWindowPos(h, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }
}

/// Piilotus samalla Win32-kutsulla kuin näyttö: Tauri ei tiedä ikkunan olevan
/// näkyvissä (näytettiin ohi sen), joten sen oma hide() ei tekisi mitään.
#[cfg(windows)]
fn hide_raw(w: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    if let Ok(h) = w.hwnd() {
        unsafe { ShowWindow(h.0 as windows_sys::Win32::Foundation::HWND, SW_HIDE) };
    }
}

#[cfg(not(windows))]
fn show_no_activate(w: &tauri::WebviewWindow) {
    let _ = w.show();
}

#[cfg(not(windows))]
fn hide_raw(w: &tauri::WebviewWindow) {
    let _ = w.hide();
}
