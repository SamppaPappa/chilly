//! Chillyn pääikkuna: oma UI-webview ("main-ui": sivupalkki, välilehdet, asetukset) +
//! jokaiselle streamille oma WebView2, johon ladataan twitch.tv-kanavasivu riisuttuna
//! pelkäksi videoksi. Streamin voi irrottaa omaan ikkunaansa ("pop{id}") ja palauttaa.

use crate::i18n::tr;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Rect, Url, WebviewUrl, WebviewWindowBuilder};

/// Kaikilla Chillyn webvieweillä on oltava samat selainvalitsimet (sama WebView2-ympäristö),
/// joten ne lukitaan käynnistyksessä. Kieli määrää Twitchin sivujen kielen (chat, kirjautuminen).
static BROWSER_ARGS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn init_browser_args(lang: &str, hw_accel: bool) {
    let l = if lang == "en" { "en-US" } else { "fi-FI" };
    let accept = if lang == "en" { "en-US,en" } else { "fi-FI,fi,en" };
    // Laitekiihdytys pois: video puretaan ja kuva piirretään prosessorilla (apu näytönohjainongelmiin)
    let gpu = if hw_accel { "" } else { " --disable-gpu --disable-gpu-compositing --disable-accelerated-video-decode" };
    let _ = BROWSER_ARGS.set(format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required --lang={l} --accept-lang={accept}{gpu}"
    ));
}

pub fn browser_args() -> &'static str {
    BROWSER_ARGS.get().map(String::as_str).unwrap_or("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required")
}
pub const MAX_STREAMS: usize = 10;
/// Asetus "Ei välilehtirajaa": kun päällä, MAX_STREAMS ei rajoita (käyttäjä varoitettu suorituskyvystä).
static UNLIMITED_TABS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn set_unlimited_tabs(on: bool) {
    UNLIMITED_TABS.store(on, std::sync::atomic::Ordering::Relaxed);
}
fn tab_limit() -> Option<usize> {
    (!UNLIMITED_TABS.load(std::sync::atomic::Ordering::Relaxed)).then_some(MAX_STREAMS)
}
const STRIP_JS: &str = include_str!("strip.js");
const VISIBLE_JS: &str = include_str!("visible.js");
const CHAT_JS: &str = include_str!("chat.js");

/// Chatin emojilisäosat (BTTV, 7TV). Päivitetään asetuksista.
static EMOTE_BTTV: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static EMOTE_7TV: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Mainosesto (vaft) striimisivuille. Luetaan, kun striimin webview luodaan.
static ADBLOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const ADBLOCK_JS: &str = include_str!("adblock_vaft.js");

pub fn set_adblock(on: bool) {
    ADBLOCK.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Äänenvahvistin prosentteina (100 = pois). Sama kaikille streameille; strip.js vahvistaa
/// äänen WebAudiolla ja rajoitin estää säröilyn.
static BOOST: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

pub fn store_boost(pct: u32) {
    BOOST.store(pct.clamp(100, 300), std::sync::atomic::Ordering::Relaxed);
}

pub fn set_boost(app: &AppHandle, pct: u32) {
    BOOST.store(pct.clamp(100, 300), std::sync::atomic::Ordering::Relaxed);
    apply(app);
}

/// Striimisivun alustusskripti: kieli, (valinnainen) mainosesto ja Chillyn soitinskripti.
fn stream_init_script() -> String {
    let lang = if crate::i18n::en() { "en" } else { "fi" };
    let ad = if ADBLOCK.load(std::sync::atomic::Ordering::Relaxed) { ADBLOCK_JS } else { "" };
    #[cfg(debug_assertions)]
    let log = if std::env::var("CHILLY_ADLOG").is_ok() { include_str!("adlog.js") } else { "" };
    #[cfg(not(debug_assertions))]
    let log = "";
    format!("window.__chillyLang={lang:?};\n{VISIBLE_JS}\n{log}\n{ad}\n{STRIP_JS}")
}

fn addons_js() -> String {
    use std::sync::atomic::Ordering::Relaxed;
    format!(
        r#"window.__chillyAddons = {{ "ffzap-bttv": {}, "7tv-emotes": {} }};"#,
        EMOTE_BTTV.load(Relaxed),
        EMOTE_7TV.load(Relaxed)
    )
}

/// Asettaa emojilisäosat. `reload` = ladataan avoimet chatit uudelleen, jotta muutos näkyy heti.
pub fn set_emotes(app: &AppHandle, bttv: bool, seventv: bool, reload: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    EMOTE_BTTV.store(bttv, Relaxed);
    EMOTE_7TV.store(seventv, Relaxed);
    if !reload {
        return;
    }
    let js = format!("{}{}; location.reload();", addons_js(), CHAT_JS.replace("window.__chillyChat = true;", ""));
    let ids: Vec<u32> = app.state::<ViewerSt>().lock().unwrap().tabs.iter().map(|t| t.id).collect();
    for id in ids {
        if let Some(cw) = app.get_webview(&chat_label(id)) {
            let _ = cw.eval(&js);
        }
    }
}
/// Tätä leveämpi chat-alue jaetaan kahdelle chatille rinnakkain, kapeampi allekkain (sama raja UI:ssa).
const CHAT_SPLIT_MIN_W: f64 = 560.0;

const BG: tauri::window::Color = tauri::window::Color(14, 14, 16, 255);

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

#[derive(Clone, Copy, Serialize, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AutoKind {
    #[default]
    Manual,
    Streak,
    RaidGuest,
}

#[derive(Clone, Serialize, Default)]
pub struct Tab {
    pub id: u32,
    pub login: String,
    pub name: String,
    pub image: String,
    pub muted: bool,
    /// Irrotettu omaan ikkunaansa
    pub popped: bool,
    /// Kanava ei ole livenä → UI näyttää Chillyn oman kanavasivun ja stream piilotetaan
    pub offline: bool,
    /// Multistream: toinen stream samalla välilehdellä (sen Tab-id)
    pub partner: Option<u32>,
    /// Tämä on toisen välilehden partner-stream (ei omaa välilehteä)
    pub child: bool,
    /// Multistream-asettelu: "side" (rinnakkain), "pip" (iso + pieni), "stack" (allekkain)
    pub layout: String,
    /// Ääni kuuluu partner-streamista (muuten päästreamista)
    pub audio_partner: bool,
    /// Chat-välilehti: partnerin chat valittuna
    pub chat_partner: bool,
    /// Multistream: ääni kuuluu molemmista (kumpikin mykistettävissä erikseen)
    pub audio_both: bool,
    /// Multistream: molemmille oma chat rinnakkain (kun chat on videon alla)
    pub chat_both: bool,
    /// Tämän streamin oma mykistys "ääni molemmissa" -tilassa
    pub mute_self: bool,
    /// Multistream: streamien paikat vaihdettu (partner vasemmalla/isona). Pelkkä paikkojen vaihto,
    /// mitään ei ladata uudelleen.
    pub swapped: bool,
    /// Välilehden alkuperä: käyttäjän oma, Katseluputken avaama, vai Katseluputken raidivieras
    pub auto_kind: AutoKind,
    /// Järjestysnumero kun välilehti muuttui raidivieraaksi (vanhin suljetaan ensin)
    pub raid_seq: u64,
    /// Mainosesto oli päällä, kun striimi luotiin
    pub adblock: bool,
}

#[derive(Default)]
pub struct Viewer {
    tabs: Vec<Tab>,
    active: Option<u32>,
    next: u32,
    raid_seq: u64,
    /// Videoalueen sijainti UI:n ilmoittamana (looginen px): x, y, w, h
    stage: Option<(f64, f64, f64, f64)>,
    /// Chat-alueen sijainti, None = chat piilossa
    chat: Option<(f64, f64, f64, f64)>,
    /// UI näyttää jotain videoalueen päällä (asetukset) → streamit piiloon
    covered: bool,
}

pub type ViewerSt = Mutex<Viewer>;

fn label(id: u32) -> String {
    format!("s{id}")
}

fn chat_label(id: u32) -> String {
    format!("c{id}")
}

fn pop_label(id: u32) -> String {
    format!("pop{id}")
}

pub fn pop_id(window_label: &str) -> Option<u32> {
    window_label.strip_prefix("pop")?.parse().ok()
}

fn is_twitch(u: &Url) -> bool {
    matches!(u.scheme(), "https" | "about" | "blob" | "data")
        && u.host_str().map_or(true, |h| h == "twitch.tv" || h.ends_with(".twitch.tv"))
}

/// Luo pääikkunan (kutsutaan käynnistyksessä) ja palauttaa tallennetun koon/sijainnin jos se on näkyvällä näytöllä.
pub fn create_main(app: &AppHandle, visible: bool, bounds: Option<&WindowBounds>) -> tauri::Result<()> {
    let w = WindowBuilder::new(app, "main")
        .title("Chilly")
        .inner_size(1280.0, 800.0)
        .min_inner_size(820.0, 520.0)
        .theme(Some(tauri::Theme::Dark))
        .background_color(BG)
        .visible(false)
        .focused(visible)
        .build()?;
    if let Some(b) = bounds {
        if b.width >= 820 && b.height >= 520 {
            let _ = w.set_size(PhysicalSize::new(b.width, b.height));
        }
        // Sijainti palautetaan vain, jos ikkunan yläreuna osuu jollekin nykyiselle näytölle
        if let Ok(mons) = w.available_monitors() {
            let ok = mons.iter().any(|m| {
                let wa = m.work_area();
                let x0 = wa.position.x - (b.width as i32 / 2);
                let x1 = wa.position.x + wa.size.width as i32 - 80;
                let y0 = wa.position.y - 10;
                let y1 = wa.position.y + wa.size.height as i32 - 60;
                b.x >= x0 && b.x <= x1 && b.y >= y0 && b.y <= y1
            });
            if ok {
                let _ = w.set_position(PhysicalPosition::new(b.x, b.y));
            }
        }
        if b.maximized {
            let _ = w.maximize();
        }
    }
    if visible {
        let _ = w.show();
    }
    let size = w.inner_size()?.to_logical::<f64>(w.scale_factor()?);
    let ui = WebviewBuilder::new("main-ui", WebviewUrl::App("index.html".into()))
        .additional_browser_args(browser_args())
        .auto_resize();
    // add_child ajaa luonnin pääsäikeessä; setupista kutsuttaessa se tapahtuu suoraan (ei jumitu)
    w.add_child(ui, LogicalPosition::new(0.0, 0.0), LogicalSize::new(size.width, size.height))?;
    Ok(())
}

/// Tallentaa pääikkunan nykyisen koon ja sijainnin (maksimoituna säilytetään normaali koko/paikka).
pub fn capture_main_bounds(w: &tauri::Window, prev: Option<&WindowBounds>) -> Option<WindowBounds> {
    if w.is_minimized().unwrap_or(false) {
        return None;
    }
    let maximized = w.is_maximized().unwrap_or(false);
    if maximized {
        let mut b = prev.cloned().unwrap_or(WindowBounds { x: 100, y: 100, width: 1280, height: 800, maximized: true });
        b.maximized = true;
        return Some(b);
    }
    let pos = w.outer_position().ok()?;
    let size = w.inner_size().ok()?;
    if size.width < 400 || size.height < 300 {
        return None;
    }
    Some(WindowBounds { x: pos.x, y: pos.y, width: size.width, height: size.height, maximized: false })
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    apply(app);
}

fn to_rect((x, y, w, h): (f64, f64, f64, f64)) -> Rect {
    Rect { position: LogicalPosition::new(x, y).into(), size: LogicalSize::new(w.max(1.0), h.max(1.0)).into() }
}

fn stage_rect(v: &Viewer) -> Option<Rect> {
    v.stage.map(to_rect)
}

/// Chat-sivulla sallitaan vain chat itse ja Twitchin kirjautumissivut; muu avataan selaimeen.
fn chat_nav_ok(u: &Url) -> bool {
    if !is_twitch(u) {
        return false;
    }
    match u.host_str() {
        Some("www.twitch.tv") | Some("twitch.tv") => {
            let p = u.path();
            p.starts_with("/popout/") || p.starts_with("/login") || p.starts_with("/passport")
        }
        _ => true,
    }
}

/// Twitchin oma siirtymä chat-näkymään (esim. raid): /<kanava>/chat tai ?referrer=raid
fn is_chat_redirect(u: &Url) -> bool {
    if !matches!(u.host_str(), Some("www.twitch.tv") | Some("twitch.tv")) {
        return false;
    }
    let p = u.path().trim_end_matches('/');
    let segs: Vec<&str> = p.split('/').filter(|s| !s.is_empty()).collect();
    (segs.len() == 2 && segs[1] == "chat") || u.query().map_or(false, |q| q.contains("referrer=raid"))
}

fn full_rect(win: &tauri::Window) -> Option<Rect> {
    let s = win.inner_size().ok()?.to_logical::<f64>(win.scale_factor().ok()?);
    Some(Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(s.width, s.height).into() })
}

/// WebView2-tason mykistys: sivu ei tiedä olevansa mykistetty, joten Twitch laskee katselun normaalisti.
pub fn set_muted(wv: &tauri::Webview, muted: bool) {
    let _ = wv.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
        use windows_core::Interface;
        if let Ok(core) = pw.controller().CoreWebView2() {
            if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
                let _ = c8.SetIsMuted(muted);
            }
        }
    });
}

/// WebView2:n muistitavoite: piilossa olevat näkymät saavat vapauttaa välimuistinsa.
/// Sivu jatkaa toimintaansa (stream pyörii, pisteet kertyvät), vain muistia säästetään.
pub fn set_memory_low(wv: &tauri::Webview, low: bool) {
    let _ = wv.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        if let Ok(core) = pw.controller().CoreWebView2() {
            if let Ok(c19) = core.cast::<ICoreWebView2_19>() {
                let lvl = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
                let _ = c19.SetMemoryUsageTargetLevel(lvl);
            }
        }
    });
}

/// Nostaa lapsi-webviewin Z-järjestyksessä päällimmäiseksi (multistream PiP-paikanvaihto).
#[cfg(windows)]
fn bring_to_top(app: &AppHandle, wv_label: &str) {
    let Some(wv) = app.get_webview(wv_label) else { return };
    let main_hwnd = app.get_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
    let _ = wv.with_webview(move |pw| unsafe {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
        let mut parent = std::mem::zeroed();
        if pw.controller().ParentWindow(&mut parent).is_ok() {
            let p = parent.0 as isize;
            if p != 0 && p != main_hwnd {
                let _ = SetWindowPos(p as HWND, HWND_TOP, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
        }
    });
}

#[cfg(not(windows))]
fn bring_to_top(_app: &AppHandle, _wv_label: &str) {}

/// Päivittää streamien näkyvyyden, paikan ja mykistyksen.
/// Irrotetut näkyvät omassa ikkunassaan; muista vain aktiivinen näkyy, muut pyörivät mykistettyinä.
/// Multistreamin kahden streamin paikat videoalueella.
fn split((x, y, w, h): (f64, f64, f64, f64), layout: &str) -> ((f64, f64, f64, f64), (f64, f64, f64, f64)) {
    const GAP: f64 = 2.0;
    match layout {
        "stack" => ((x, y, w, h / 2.0 - GAP), (x, y + h / 2.0 + GAP, w, h / 2.0 - GAP)),
        "pip" => {
            let bw = (w * 0.32).max(260.0).min(w - 28.0);
            let bh = bw * 9.0 / 16.0;
            ((x, y, w, h), (x + w - bw - 14.0, y + h - bh - 14.0, bw, bh))
        }
        _ => ((x, y, w / 2.0 - GAP, h), (x + w / 2.0 + GAP, y, w / 2.0 - GAP, h)),
    }
}

fn apply(app: &AppHandle) {
    let main_visible = app.get_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    let st = app.state::<ViewerSt>();
    let v = st.lock().unwrap();
    let active = v.active.and_then(|a| v.tabs.iter().find(|t| t.id == a)).cloned();
    let partner = active.as_ref().and_then(|a| a.partner).and_then(|p| v.tabs.iter().find(|t| t.id == p)).cloned();
    let (ra, rb) = match (v.stage, &partner, &active) {
        (Some(s), Some(_), Some(a)) => {
            let (a_, b_) = split(s, &a.layout);
            if a.swapped { (Some(b_), Some(a_)) } else { (Some(a_), Some(b_)) }
        }
        (Some(s), None, _) => (Some(s), None),
        _ => (None, None),
    };
    let multi = partner.is_some();
    for t in &v.tabs {
        let Some(wv) = app.get_webview(&label(t.id)) else { continue };
        let is_a = active.as_ref().map_or(false, |a| a.id == t.id);
        let is_b = partner.as_ref().map_or(false, |p| p.id == t.id);
        let mut btn = false;
        if t.popped {
            if let Some(r) = app.get_window(&pop_label(t.id)).and_then(|w| full_rect(&w)) {
                let _ = wv.set_bounds(r);
            }
            let _ = wv.show();
            set_muted(&wv, t.muted);
            set_memory_low(&wv, false);
        } else if is_a && !v.covered && (multi || !t.offline) && ra.is_some() {
            let a = active.as_ref().unwrap();
            let _ = wv.set_bounds(to_rect(ra.unwrap()));
            let _ = wv.show();
            // Piilotettu pääikkuna (trayssa) on aina mykistetty, ettei taustalta kuulu koskaan ääntä
            set_muted(&wv, !main_visible || a.muted || (multi && if a.audio_both { t.mute_self } else { a.audio_partner }));
            btn = multi && a.audio_both;
            set_memory_low(&wv, false);
        } else if is_b && !v.covered && rb.is_some() {
            let a = active.as_ref().unwrap();
            let _ = wv.set_bounds(to_rect(rb.unwrap()));
            let _ = wv.show();
            set_muted(&wv, !main_visible || a.muted || if a.audio_both { t.mute_self } else { !a.audio_partner });
            btn = a.audio_both;
            set_memory_low(&wv, false);
        } else {
            // Taustastriimi pidetään "näkyvissä" pienenä ikkunan ulkopuolella: piilotetussa
            // WebView2:ssa Twitchin soitin ei käynnisty lainkaan (ei katseluaikaa, putkea, pisteitä).
            // Laatu lasketaan matalimmaksi strip.js:ssä (window.__chillyBg).
            let _ = wv.set_bounds(Rect {
                position: LogicalPosition::new(-400.0, -400.0).into(),
                size: LogicalSize::new(320.0, 180.0).into(),
            });
            let _ = wv.show();
            set_muted(&wv, true);
            set_memory_low(&wv, false);
        }
        // Videon nurkan mykistysnappi (strip.js piirtää sen), null = ei nappia
        let state = if btn { if t.mute_self { "true" } else { "false" } } else { "null" };
        // Taustalla (ei aktiivinen, ei multistreamin pari, ei irrotettu) → strip.js laskee laadun matalimmaksi
        let bg = !(t.popped || is_a || is_b);
        // Multistream: striimin oma sulkunappi (näkyy hiiren ollessa videon päällä)
        let close = !t.popped && (is_a || is_b) && multi;
        let gain = BOOST.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0;
        let _ = wv.eval(format!("window.__chillyBg={bg};window.__chillyGain={gain};window.__chillyCloseBtn={close};window.__chillyMuteState={state};window.__chillyMuteBtn&&window.__chillyMuteBtn({state})"));
    }
    // Multistream "Iso + pieni" (PiP): varmistetaan, että pieni ikkuna pysyy ison päällä myös paikkojen vaihdon jälkeen
    if multi && !v.covered {
        if let (Some(a), Some(p)) = (&active, &partner) {
            if a.layout == "pip" {
                let small_id = if a.swapped { a.id } else { p.id };
                bring_to_top(app, &label(small_id));
            }
        }
    }
    // Chat: multistreamissa joko välilehdet (yksi chat kerrallaan) tai molemmat yhtä aikaa.
    // Leveä chat-alue → rinnakkain, kapea (chat sivulla) → allekkain.
    let both = matches!((&active, &partner), (Some(a), Some(_)) if a.chat_both);
    let chat_rect = |id: u32| -> Option<(f64, f64, f64, f64)> {
        let r = v.chat?;
        let a = active.as_ref()?;
        let p = partner.as_ref();
        if both {
            let (x, y, w, h) = r;
            let (ra, rb) = if w >= CHAT_SPLIT_MIN_W {
                ((x, y, w / 2.0 - 1.0, h), (x + w / 2.0 + 1.0, y, w / 2.0 - 1.0, h))
            } else {
                ((x, y, w, h / 2.0 - 1.0), (x, y + h / 2.0 + 1.0, w, h / 2.0 - 1.0))
            };
            let (ra, rb) = if a.swapped { (rb, ra) } else { (ra, rb) };
            if id == a.id {
                return Some(ra);
            }
            if p.map_or(false, |p| p.id == id) {
                return Some(rb);
            }
            return None;
        }
        let shown = match p {
            Some(p) if a.chat_partner => p.id,
            _ => a.id,
        };
        (id == shown).then_some(r)
    };
    for t in &v.tabs {
        let Some(cw) = app.get_webview(&chat_label(t.id)) else { continue };
        match chat_rect(t.id) {
            Some(r) if !v.covered => {
                let _ = cw.set_bounds(to_rect(r));
                let _ = cw.show();
                set_memory_low(&cw, false);
            }
            _ => {
                let _ = cw.hide();
                set_memory_low(&cw, true);
            }
        }
    }
    drop(v);
    let _ = app.emit("viewer-state", ());
}

/// Auki olevat streamit (id, login) automaattisia lunastuksia varten.
pub fn tab_list(app: &AppHandle) -> Vec<(u32, String)> {
    app.state::<ViewerSt>().lock().unwrap().tabs.iter().map(|t| (t.id, t.login.clone())).collect()
}

/// Nykyisten välilehtien tila Katseluputken kiintiölaskentaa varten.
pub fn snapshot_tabs(app: &AppHandle) -> (Vec<Tab>, Option<u32>, bool) {
    let main_visible = app.get_window("main").and_then(|w| w.is_visible().ok()).unwrap_or(false);
    let st = app.state::<ViewerSt>();
    let v = st.lock().unwrap();
    (v.tabs.clone(), v.active, main_visible)
}

pub fn state_json(app: &AppHandle) -> serde_json::Value {
    let st = app.state::<ViewerSt>();
    let v = st.lock().unwrap();
    serde_json::json!({ "tabs": v.tabs, "active": v.active, "max": tab_limit() })
}

/// Avaa kanavan uuteen välilehteen (tai aktivoi sen, jos se on jo auki).
pub fn open(app: &AppHandle, login: &str, name: &str, image: &str) -> Result<(), String> {
    open_with(app, login, name, image, false, false)
}

/// Auto-katselu: avaa kanavan taustavälilehdelle mykistettynä, ei tuo ikkunaa esiin eikä vaihda
/// katsottavaa välilehteä. Jos kanava on jo auki, ei tehdä mitään.
pub fn open_auto(app: &AppHandle, login: &str, name: &str, image: &str) -> Result<(), String> {
    if app.state::<ViewerSt>().lock().unwrap().tabs.iter().any(|t| t.login == login) {
        return Ok(());
    }
    open_with(app, login, name, image, true, false)
}

/// Testi: avaa kanavan uudelle taustavälilehdelle, vaikka se olisi jo auki (mainosvertailu).
#[cfg(debug_assertions)]
pub fn dev_open_dup(app: &AppHandle, login: &str) -> Result<(), String> {
    open_with(app, login, login, "", true, true)
}

/// Testiloki: yksi JSON-rivi per tapahtuma kansioon CHILLY_ADLOG.
#[cfg(debug_assertions)]
fn dev_adlog(app: &AppHandle, id: u32, json: &str) {
    use std::io::Write;
    let Ok(dir) = std::env::var("CHILLY_ADLOG") else { return };
    let (login, adblock) = app.state::<ViewerSt>().lock().unwrap().tabs.iter().find(|t| t.id == id)
        .map(|t| (t.login.clone(), t.adblock)).unwrap_or_default();
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let v = serde_json::json!({ "ts": ts, "tab": id, "login": login, "adblock": adblock,
        "data": serde_json::from_str::<serde_json::Value>(json).unwrap_or(serde_json::Value::Null) });
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::path::Path::new(&dir).join("adlog.jsonl")) {
        let _ = writeln!(f, "{v}");
    }
}

/// Sulkee auto-katselun välilehden tunnisteella (ei merkitä käyttäjän sulkemaksi).
pub fn close_tab_auto(app: &AppHandle, id: u32) {
    close_tab_inner(app, id);
}

/// Käyttäjä sulki välilehden itse: jos se oli Katseluputken avaama, sitä ei avata uudelleen
/// saman liven aikana, ja vapautuneelle paikalle voidaan nostaa seuraava liekitetty kanava.
pub fn close_tab_user(app: &AppHandle, id: u32) {
    let dismissed: Vec<String> = {
        let st = app.state::<ViewerSt>();
        let v = st.lock().unwrap();
        let mut out = Vec::new();
        if let Some(t) = v.tabs.iter().find(|t| t.id == id) {
            if t.auto_kind == AutoKind::Streak {
                out.push(t.login.clone());
            }
            if let Some(pid) = t.partner {
                if let Some(p) = v.tabs.iter().find(|x| x.id == pid && x.auto_kind == AutoKind::Streak) {
                    out.push(p.login.clone());
                }
            }
        }
        out
    };
    for login in dismissed {
        crate::mark_streak_dismissed(app, &login);
    }
    close_tab_inner(app, id);
    crate::wake_streak(app);
}

fn open_with(app: &AppHandle, login: &str, name: &str, image: &str, background: bool, dup: bool) -> Result<(), String> {
    if !background {
        show_main(app);
    }
    let win = app.get_window("main").ok_or(tr("Pääikkuna puuttuu", "Main window missing"))?;

    let st = app.state::<ViewerSt>();
    let (id, rect) = {
        let mut v = st.lock().unwrap();
        if !background {
            v.covered = false;
        }
        if let Some(pos) = v.tabs.iter().position(|t| t.login == login && !dup) {
            let (mut id, popped, child) = (v.tabs[pos].id, v.tabs[pos].popped, v.tabs[pos].child);
            if !background {
                // Käyttäjä avasi raidivieraan itse → siitä tulee käyttäjän oma välilehti
                if v.tabs[pos].auto_kind == AutoKind::RaidGuest {
                    v.tabs[pos].auto_kind = AutoKind::Manual;
                    v.tabs[pos].raid_seq = 0;
                }
                // Taustalla mykistettynä ollut Katseluputki-striimi avattiin käsin → ääni päälle
                if v.tabs[pos].auto_kind == AutoKind::Streak && v.tabs[pos].muted && v.active != Some(id) {
                    v.tabs[pos].muted = false;
                }
            }
            if child {
                if let Some(p) = v.tabs.iter().find(|p| p.partner == Some(id)) {
                    id = p.id;
                }
            }
            v.active = Some(id);
            drop(v);
            if popped {
                if let Some(w) = app.get_window(&pop_label(id)) {
                    let _ = w.set_focus();
                }
            }
            apply(app);
            return Ok(());
        }
        if tab_limit().map_or(false, |m| v.tabs.len() >= m) {
            return Err(if crate::i18n::en() { format!("You can have at most {MAX_STREAMS} streams open. Close a tab first, or remove the limit in Settings → Viewing.") } else { format!("Auki voi olla enintään {MAX_STREAMS} streamia. Sulje ensin jokin välilehti tai poista raja: Asetukset → Katselu.") });
        }
        v.next += 1;
        let id = v.next;
        v.tabs.push(Tab {
            id, login: login.into(), name: name.into(), image: image.into(),
            muted: background,
            auto_kind: if background && !dup { AutoKind::Streak } else { AutoKind::Manual },
            adblock: ADBLOCK.load(std::sync::atomic::Ordering::Relaxed),
            ..Default::default()
        });
        if !background || v.active.is_none() {
            v.active = Some(id);
        }
        // Jos UI ei ole vielä kertonut videoalueen paikkaa, luodaan piiloon ja apply siirtää paikalleen
        (id, stage_rect(&v).unwrap_or(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(1.0, 1.0).into(),
        }))
    };

    if let Err(e) = spawn_webviews(app, &win, id, login, rect) {
        let mut v = st.lock().unwrap();
        v.tabs.retain(|t| t.id != id);
        if v.active == Some(id) {
            v.active = v.tabs.last().map(|t| t.id);
        }
        return Err(e);
    }
    apply(app);
    if !background {
        crate::wake_streak(app);
    }
    Ok(())
}

/// Chatista klikattu linkki: Twitchin kirjautuminen Chillyn kirjautumisikkunaan, Twitch-kanava
/// Chillyn välilehdelle, kaikki muu järjestelmän selaimeen.
pub fn open_link(app: &AppHandle, url: &str) {
    let Ok(u) = Url::parse(url) else { return };
    if !matches!(u.scheme(), "http" | "https") {
        return;
    }
    let host = u.host_str().unwrap_or("");
    let twitch = host == "twitch.tv" || host.ends_with(".twitch.tv");
    if twitch && (u.path().starts_with("/login") || u.path().starts_with("/passport") || host == "passport.twitch.tv") {
        let _ = open_twitch_login(app, url);
        return;
    }
    if twitch && (host == "www.twitch.tv" || host == "twitch.tv" || host == "m.twitch.tv") {
        const RESERVED: &[&str] = &["directory", "search", "settings", "subscriptions", "inventory", "drops", "wallet",
            "friends", "messages", "downloads", "turbo", "prime", "jobs", "p", "store", "videos", "popout", "moderator", "u"];
        let segs: Vec<&str> = u.path().split('/').filter(|s| !s.is_empty()).collect();
        if segs.len() == 1 {
            let l = segs[0].to_lowercase();
            if l.len() >= 2 && l.len() <= 25 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !RESERVED.contains(&l.as_str()) {
                let _ = open(app, &l, &l, "");
                return;
            }
        }
    }
    let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
}

/// Uuteen ikkunaan avautuvat linkit (chatin linkit, target=_blank): Twitchin kirjautuminen
/// Chillyn kirjautumisikkunaan, kaikki muu järjestelmän selaimeen. WebView2:n omaa
/// ponnahdusikkunaa ei koskaan avata.
fn new_window_handler(app: &AppHandle) -> impl Fn(Url, tauri::webview::NewWindowFeatures) -> tauri::webview::NewWindowResponse<tauri::Wry> + Send + 'static {
    let app = app.clone();
    move |u, _| {
        let tw_login = matches!(u.host_str(), Some("www.twitch.tv") | Some("twitch.tv") | Some("passport.twitch.tv"))
            && (u.path().starts_with("/login") || u.path().starts_with("/passport") || u.host_str() == Some("passport.twitch.tv"));
        if tw_login {
            let app = app.clone();
            let s = u.to_string();
            tauri::async_runtime::spawn(async move { let _ = open_twitch_login(&app, &s); });
        } else if matches!(u.scheme(), "http" | "https") {
            let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
        }
        tauri::webview::NewWindowResponse::Deny
    }
}

/// Luo streamin ja sen chatin webviewit.
fn spawn_webviews(app: &AppHandle, win: &tauri::Window, id: u32, login: &str, rect: Rect) -> Result<(), String> {
    let st = app.state::<ViewerSt>();
    let url = Url::parse(&format!("https://www.twitch.tv/{login}")).map_err(|e| e.to_string())?;
    let wv = WebviewBuilder::new(label(id), WebviewUrl::External(url))
        .initialization_script(stream_init_script())
        .additional_browser_args(browser_args())
        .on_navigation(|u| {
            if is_twitch(u) {
                true
            } else {
                let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
                false
            }
        })
        .on_new_window(new_window_handler(app));
    let wv = win.add_child(wv, rect.position, rect.size).map_err(|e| e.to_string())?;
    listen_stream_messages(app, &wv, id);

    // Kanavan chat (Twitchin oma + FFZ)
    let chat_url = Url::parse(&format!("https://www.twitch.tv/popout/{login}/chat?darkpopout")).map_err(|e| e.to_string())?;
    let cw = WebviewBuilder::new(chat_label(id), WebviewUrl::External(chat_url))
        .initialization_script(format!("{}\n{}", addons_js(), CHAT_JS))
        .additional_browser_args(browser_args())
        .on_navigation(|u| {
            if chat_nav_ok(u) || matches!(u.scheme(), "about" | "blob" | "data") {
                true
            } else if is_chat_redirect(u) {
                // Raidissa Twitch siirtää chatin uuden kanavan chat-näkymään. Ei avata sitä selaimeen:
                // striminäkymä ilmoittaa raidista ja Chilly vaihtaa chatin itse (follow_nav).
                false
            } else {
                let _ = tauri_plugin_opener::open_url(u.as_str(), None::<&str>);
                false
            }
        })
        .on_new_window(new_window_handler(app));
    let crect = { let v = st.lock().unwrap(); v.chat.map(to_rect).unwrap_or(rect) };
    if let Ok(cw) = win.add_child(cw, crect.position, crect.size) {
        listen_stream_messages(app, &cw, id);
    }
    Ok(())
}

/// Multistream: lisää toinen stream välilehdelle.
pub fn add_partner(app: &AppHandle, parent: u32, login: &str, name: &str, image: &str, audio_both: bool, chat_both: bool) -> Result<(), String> {
    let win = app.get_window("main").ok_or(tr("Pääikkuna puuttuu", "Main window missing"))?;
    let st = app.state::<ViewerSt>();
    let id = {
        let v = st.lock().unwrap();
        let p = v.tabs.iter().find(|t| t.id == parent).ok_or(tr("Välilehteä ei löydy", "Tab not found"))?;
        if p.partner.is_some() {
            return Err(tr("Välilehdellä on jo kaksi streamia.", "This tab already has two streams.").into());
        }
        if p.popped {
            return Err(tr("Palauta irrotettu stream ensin välilehdelle.", "Bring the popped-out stream back to its tab first.").into());
        }
        if p.login == login {
            return Err(tr("Tämä kanava on jo tällä välilehdellä.", "This channel is already on this tab.").into());
        }
        if tab_limit().map_or(false, |m| v.tabs.len() >= m) {
            return Err(if crate::i18n::en() { format!("You can have at most {MAX_STREAMS} streams open. Close a tab first, or remove the limit in Settings → Viewing.") } else { format!("Auki voi olla enintään {MAX_STREAMS} streamia. Sulje ensin jokin välilehti tai poista raja: Asetukset → Katselu.") });
        }
        // Jos kanava on jo omalla välilehdellään, siirretään se tänne (suljetaan vanha)
        let existing = v.tabs.iter().find(|t| t.login == login && !t.child && t.partner.is_none()).map(|t| t.id);
        drop(v);
        if let Some(e) = existing {
            close_tab(app, e);
        }
        let mut v = st.lock().unwrap();
        v.next += 1;
        let id = v.next;
        v.tabs.push(Tab {
            id, login: login.into(), name: name.into(), image: image.into(),
            child: true,
            adblock: ADBLOCK.load(std::sync::atomic::Ordering::Relaxed),
            ..Default::default()
        });
        if let Some(p) = v.tabs.iter_mut().find(|t| t.id == parent) {
            p.partner = Some(id);
            if p.layout.is_empty() {
                p.layout = "side".into();
            }
            p.audio_partner = false;
            p.chat_partner = false;
            p.audio_both = audio_both;
            p.chat_both = chat_both;
            p.mute_self = false;
            p.swapped = false;
        }
        v.active = Some(parent);
        id
    };
    let rect = Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: LogicalSize::new(1.0, 1.0).into() };
    if let Err(e) = spawn_webviews(app, &win, id, login, rect) {
        let mut v = st.lock().unwrap();
        v.tabs.retain(|t| t.id != id);
        if let Some(p) = v.tabs.iter_mut().find(|t| t.id == parent) {
            p.partner = None;
        }
        return Err(e);
    }
    apply(app);
    Ok(())
}

pub fn remove_partner(app: &AppHandle, parent: u32) {
    let pid = app.state::<ViewerSt>().lock().unwrap().tabs.iter().find(|t| t.id == parent).and_then(|t| t.partner);
    if let Some(pid) = pid {
        close_tab(app, pid);
    }
}

/// Multistreamin asetukset: asettelu, äänilähde, valittu chat.
pub fn set_multi(app: &AppHandle, parent: u32, layout: Option<String>, audio_partner: Option<bool>, chat_partner: Option<bool>, audio_both: Option<bool>, chat_both: Option<bool>) {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        if let Some(t) = v.tabs.iter_mut().find(|t| t.id == parent) {
            if let Some(l) = layout {
                t.layout = match l.as_str() {
                    "pip" | "stack" => l,
                    _ => "side".into(),
                };
            }
            if let Some(a) = audio_partner {
                t.audio_partner = a;
            }
            if let Some(c) = chat_partner {
                t.chat_partner = c;
            }
            if let Some(b) = audio_both {
                t.audio_both = b;
            }
            if let Some(b) = chat_both {
                t.chat_both = b;
            }
        }
    }
    apply(app);
}

/// "Ääni molemmissa" -tilan oma mykistys yhdelle streamille.
pub fn toggle_self_mute(app: &AppHandle, id: u32) {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id) {
            t.mute_self = !t.mute_self;
        }
    }
    apply(app);
}

/// Videon päälle piirretty mykistysnappi lähettää viestin WebView2:n postMessagella.
/// Twitch-sivulla ei ole Tauri-IPC:tä, joten viesti kuunnellaan suoraan WebView2:sta.
fn listen_stream_messages(app: &AppHandle, wv: &tauri::Webview, id: u32) {
    let app = app.clone();
    let _ = wv.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2WebMessageReceivedEventArgs};
        use webview2_com::WebMessageReceivedEventHandler;
        let Ok(core) = pw.controller().CoreWebView2() else { return };
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |_: Option<ICoreWebView2>, args: Option<ICoreWebView2WebMessageReceivedEventArgs>| {
                let Some(args) = args else { return Ok(()) };
                let mut p = windows_core::PWSTR::null();
                if args.TryGetWebMessageAsString(&mut p).is_ok() && !p.is_null() {
                    let msg = webview2_com::take_pwstr(p);
                    let app = app.clone();
                    match msg.as_str() {
                        "chilly:mute" => {
                            tauri::async_runtime::spawn(async move { toggle_self_mute(&app, id) });
                        }
                        "chilly:close" => {
                            tauri::async_runtime::spawn(async move { close_slot(&app, id) });
                        }
                        "chilly:hello" => {
                            tauri::async_runtime::spawn(async move { apply(&app) });
                        }
                        #[cfg(debug_assertions)]
                        m if m.starts_with("chilly:log:") => {
                            dev_adlog(&app, id, &m["chilly:log:".len()..]);
                        }
                        m if m.starts_with("chilly:open:") => {
                            let url = m["chilly:open:".len()..].to_string();
                            tauri::async_runtime::spawn(async move { open_link(&app, &url) });
                        }
                        "chilly:ffzsettings" => {
                            tauri::async_runtime::spawn(async move { let _ = open_chat_settings(&app); });
                        }
                        m if m.starts_with("chilly:nav:") => {
                            let login = m["chilly:nav:".len()..].to_string();
                            tauri::async_runtime::spawn(async move { follow_nav(&app, id, &login) });
                        }
                        _ => {}
                    }
                }
                Ok(())
            },
        ));
        let mut token = 0i64;
        let _ = core.add_WebMessageReceived(&handler, &mut token);
    });
}

/// Vaihtaa multistreamin streamien paikat. Vain näkymien sijainnit vaihtuvat: streamit ja chatit
/// jatkavat keskeytyksettä, eikä mitään ladata uudelleen.
pub fn swap(app: &AppHandle, parent: u32) -> Result<(), String> {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        let t = v.tabs.iter_mut().find(|t| t.id == parent).ok_or(tr("Välilehteä ei löydy", "Tab not found"))?;
        if t.partner.is_none() {
            return Err(tr("Välilehdellä on vain yksi stream", "This tab has only one stream").into());
        }
        t.swapped = !t.swapped;
    }
    apply(app);
    Ok(())
}

pub fn activate(app: &AppHandle, id: u32) {
    let mut promoted = false;
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id) {
            // Käyttäjä klikkasi raidivieraan auki → siitä tulee käyttäjän oma välilehti
            if t.auto_kind == AutoKind::RaidGuest {
                t.auto_kind = AutoKind::Manual;
                t.raid_seq = 0;
                promoted = true;
            }
        }
        v.active = Some(id);
        v.covered = false;
    }
    apply(app);
    if promoted {
        crate::wake_streak(app);
    }
}

pub fn close_tab(app: &AppHandle, id: u32) {
    close_tab_inner(app, id);
}

fn close_tab_inner(app: &AppHandle, id: u32) {
    let partner = {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        let partner = v.tabs.iter().find(|t| t.id == id).and_then(|t| t.partner);
        // Jos suljetaan partner-stream, irrotetaan se pääkanavasta
        for t in v.tabs.iter_mut() {
            if t.partner == Some(id) {
                t.partner = None;
                t.audio_partner = false;
                t.chat_partner = false;
                t.swapped = false;
            }
        }
        v.tabs.retain(|t| t.id != id);
        if v.active == Some(id) {
            let tops: Vec<u32> = v.tabs.iter().filter(|t| !t.child).map(|t| t.id).collect();
            v.active = tops.last().copied();
        }
        partner
    };
    if let Some(p) = partner {
        close_tab_inner(app, p);
    }
    if let Some(wv) = app.get_webview(&label(id)) {
        let _ = wv.close();
    }
    if let Some(cw) = app.get_webview(&chat_label(id)) {
        let _ = cw.close();
    }
    if let Some(w) = app.get_window(&pop_label(id)) {
        let _ = w.destroy();
    }
    apply(app);
}

/// Multistream: sulkee yhden kahdesta streamista. Jäljelle jäävä täyttää alueen; jos suljetaan
/// välilehden ensimmäinen stream, pari nostetaan sen paikalle (sama kohta välilehtirivillä).
/// Tavallisella välilehdellä sama kuin close_tab_user.
pub fn close_slot(app: &AppHandle, id: u32) {
    let dismissed = {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        let Some(pos) = v.tabs.iter().position(|t| t.id == id) else { return };
        let d = (v.tabs[pos].auto_kind == AutoKind::Streak).then(|| v.tabs[pos].login.clone());
        if let Some(pid) = v.tabs[pos].partner {
            let parent = v.tabs[pos].clone();
            if let Some(ppos) = v.tabs.iter().position(|t| t.id == pid) {
                let mut p = v.tabs.remove(ppos);
                p.child = false;
                p.muted = parent.muted;
                p.mute_self = false;
                let pos = v.tabs.iter().position(|t| t.id == id).unwrap();
                v.tabs.insert(pos, p);
            }
            if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id) {
                t.partner = None;
            }
            if v.active == Some(id) {
                v.active = Some(pid);
            }
        }
        d
    };
    if let Some(login) = dismissed {
        crate::mark_streak_dismissed(app, &login);
    }
    close_tab_inner(app, id);
    crate::wake_streak(app);
}

/// Välilehtien uusi järjestys (raahaus). `ids` = päävälilehdet uudessa järjestyksessä;
/// multistreamin parit pysyvät listan lopussa, koska niillä ei ole omaa välilehteä.
pub fn reorder(app: &AppHandle, ids: Vec<u32>) {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        let mut old = std::mem::take(&mut v.tabs);
        let mut out = Vec::with_capacity(old.len());
        for id in ids {
            if let Some(i) = old.iter().position(|t| t.id == id && !t.child) {
                out.push(old.remove(i));
            }
        }
        let (rest_top, rest_child): (Vec<Tab>, Vec<Tab>) = old.into_iter().partition(|t| !t.child);
        out.extend(rest_top);
        out.extend(rest_child);
        v.tabs = out;
    }
    let _ = app.emit("viewer-state", ());
}

/// Pääikkuna suljettu trayhin: kaikki streamit lopetetaan.
pub fn close_all(app: &AppHandle) {
    let ids: Vec<u32> = app.state::<ViewerSt>().lock().unwrap().tabs.iter().map(|t| t.id).collect();
    for id in ids {
        close_tab(app, id);
    }
}

/// Pääikkuna suljettu trayhin: käyttäjän omat streamit lopetetaan, Katseluputki jatkaa taustalla.
pub fn close_all_but_auto(app: &AppHandle) {
    let (manual_ids, auto_partners): (Vec<u32>, Vec<u32>) = {
        let st = app.state::<ViewerSt>();
        let v = st.lock().unwrap();
        let manuals = v.tabs.iter()
            .filter(|t| t.auto_kind == AutoKind::Manual && !t.child)
            .map(|t| t.id)
            .collect();
        let partners = v.tabs.iter()
            .filter(|t| t.auto_kind != AutoKind::Manual)
            .filter_map(|t| t.partner)
            .collect();
        (manuals, partners)
    };
    for pid in auto_partners {
        close_tab_inner(app, pid);
    }
    for id in manual_ids {
        close_tab_inner(app, id);
    }
    // Irrotetut ikkunat takaisin pääikkunaan, ettei piilotettu Chilly jätä ikkunoita näkyviin
    let popped: Vec<u32> = app.state::<ViewerSt>().lock().unwrap().tabs.iter().filter(|t| t.popped).map(|t| t.id).collect();
    for id in popped {
        pop_in(app, id);
    }
    crate::wake_streak(app);
}

pub fn toggle_mute(app: &AppHandle, id: u32) {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id) {
            t.muted = !t.muted;
        }
    }
    apply(app);
}

pub fn set_layout(app: &AppHandle, video: (f64, f64, f64, f64), chat: Option<(f64, f64, f64, f64)>) {
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        v.stage = Some(video);
        v.chat = chat;
    }
    apply(app);
}

pub fn set_covered(app: &AppHandle, on: bool) {
    app.state::<ViewerSt>().lock().unwrap().covered = on;
    apply(app);
}

/// Irrottaa streamin omaan ikkunaansa (sama webview siirretään, joten stream ei lataudu uudelleen).
pub fn pop_out(app: &AppHandle, id: u32) -> Result<(), String> {
    let name = {
        let st = app.state::<ViewerSt>();
        let v = st.lock().unwrap();
        let t = v.tabs.iter().find(|t| t.id == id).ok_or(tr("Välilehteä ei löydy", "Tab not found"))?;
        if t.popped {
            return Ok(());
        }
        if t.partner.is_some() || t.child {
            return Err(tr("Multistream-välilehteä ei voi irrottaa. Poista toinen stream ensin.", "A multistream tab can't be popped out. Remove the second stream first.").into());
        }
        t.name.clone()
    };
    let wv = app.get_webview(&label(id)).ok_or(tr("Streamia ei löydy", "Stream not found"))?;
    let pw = WindowBuilder::new(app, pop_label(id))
        .title(format!("{name} – Chilly"))
        .inner_size(960.0, 560.0)
        .min_inner_size(320.0, 180.0)
        .theme(Some(tauri::Theme::Dark))
        .background_color(BG)
        .build()
        .map_err(|e| e.to_string())?;
    wv.reparent(&pw).map_err(|e| e.to_string())?;
    if let Some(t) = app.state::<ViewerSt>().lock().unwrap().tabs.iter_mut().find(|t| t.id == id) {
        t.popped = true;
    }
    apply(app);
    Ok(())
}

/// Palauttaa irrotetun streamin takaisin välilehdelleen ja sulkee sen ikkunan.
pub fn pop_in(app: &AppHandle, id: u32) {
    if let (Some(wv), Some(main)) = (app.get_webview(&label(id)), app.get_window("main")) {
        let _ = wv.reparent(&main);
    }
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id) {
            t.popped = false;
        }
        v.active = Some(id);
    }
    if let Some(w) = app.get_window(&pop_label(id)) {
        let _ = w.destroy();
    }
    apply(app);
}

/// Irrotetun ikkunan koko muuttui → stream täyttämään se.
pub fn fit_pop(app: &AppHandle, id: u32) {
    if let (Some(wv), Some(r)) = (app.get_webview(&label(id)), app.get_window(&pop_label(id)).and_then(|w| full_rect(&w))) {
        let _ = wv.set_bounds(r);
    }
}

pub fn reload_all(app: &AppHandle) {
    let ids: Vec<u32> = app.state::<ViewerSt>().lock().unwrap().tabs.iter().map(|t| t.id).collect();
    for id in ids {
        for l in [label(id), chat_label(id)] {
            if let Some(wv) = app.get_webview(&l) {
                let _ = wv.eval("location.reload()");
            }
        }
    }
}

/// Twitch-kirjautumisikkuna Chillyn sisällä. Selaindata on jaettu streamien ja chattien kanssa,
/// joten samalla kirjautumisella toimivat chat, kanavapisteet ja katseluaika.
pub fn open_twitch_login(app: &AppHandle, url: &str) -> Result<(), String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window("twlogin") {
        let _ = w.navigate(url);
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "twlogin", WebviewUrl::External(url))
        .title(tr("Kirjaudu Twitchiin – Chilly", "Log in to Twitch – Chilly"))
        .inner_size(520.0, 780.0)
        .center()
        .additional_browser_args(browser_args())
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn close_twitch_login(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("twlogin") {
        let _ = w.destroy();
    }
}

/// Onko twitch.tv-kirjautuminen voimassa (auth-token-eväste jaetussa selaindatassa).
pub async fn web_logged_in(app: &AppHandle) -> bool {
    use std::sync::Arc;
    let Some(wv) = app.get_webview("main-ui") else { return false };
    let (tx, rx) = tokio::sync::oneshot::channel::<bool>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    let send = {
        let tx = tx.clone();
        move |v: bool| {
            if let Some(t) = tx.lock().unwrap().take() {
                let _ = t.send(v);
            }
        }
    };
    let send_err = send.clone();
    let res = wv.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
        use webview2_com::{take_pwstr, GetCookiesCompletedHandler};
        use windows_core::{Interface, PWSTR};
        let Ok(core) = pw.controller().CoreWebView2() else { return send_err(false) };
        let Ok(c2) = core.cast::<ICoreWebView2_2>() else { return send_err(false) };
        let Ok(cm) = c2.CookieManager() else { return send_err(false) };
        let uri = windows_core::HSTRING::from("https://www.twitch.tv");
        let handler = GetCookiesCompletedHandler::create(Box::new(move |_r, list| {
            let mut found = false;
            if let Some(list) = list {
                let mut n = 0u32;
                let _ = list.Count(&mut n);
                for i in 0..n {
                    let Ok(c) = list.GetValueAtIndex(i) else { continue };
                    let mut name = PWSTR::null();
                    if c.Name(&mut name).is_ok() && take_pwstr(name) == "auth-token" {
                        let mut val = PWSTR::null();
                        if c.Value(&mut val).is_ok() && !take_pwstr(val).is_empty() {
                            found = true;
                        }
                    }
                }
            }
            send(found);
            Ok(())
        }));
        if cm.GetCookies(&uri, &handler).is_err() {
            send_err(false);
        }
    });
    if res.is_err() {
        return false;
    }
    tokio::time::timeout(std::time::Duration::from_secs(4), rx).await.ok().and_then(|r| r.ok()).unwrap_or(false)
}

/// Uloskirjautuessa poistetaan myös twitch.tv-istunto (esim. jos kone on yhteiskäytössä).
pub fn clear_web_session(app: &AppHandle) {
    if let Some(wv) = app.get_webview("main-ui") {
        let _ = wv.with_webview(|pw| unsafe {
            use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2;
            use windows_core::Interface;
            if let Ok(core) = pw.controller().CoreWebView2() {
                if let Ok(c2) = core.cast::<ICoreWebView2_2>() {
                    if let Ok(cm) = c2.CookieManager() {
                        let _ = cm.DeleteAllCookies();
                    }
                }
            }
        });
    }
}

/// Kehitystesti: vaihtaa aktiivisen välilehden mykistyksen.
#[cfg(debug_assertions)]
pub fn dev_toggle_active_mute(app: &AppHandle) {
    let id = app.state::<ViewerSt>().lock().unwrap().active;
    if let Some(id) = id {
        toggle_mute(app, id);
    }
}

/// Kehitystesti: irrottaa / palauttaa aktiivisen välilehden.
#[cfg(debug_assertions)]
pub fn dev_toggle_active_pop(app: &AppHandle) {
    let t = {
        let st = app.state::<ViewerSt>();
        let v = st.lock().unwrap();
        v.active.and_then(|a| v.tabs.iter().find(|t| t.id == a).map(|t| (t.id, t.popped)))
    };
    match t {
        Some((id, false)) => {
            let _ = pop_out(app, id);
        }
        Some((id, true)) => pop_in(app, id),
        None => {}
    }
}

/// Kehitystesti: aito hiiren klikkaus (käyttäjän ele) DevTools-protokollalla.
#[cfg(debug_assertions)]
pub fn dev_click(app: &AppHandle, label: &str, x: f64, y: f64) {
    let Some(wv) = app.get_webview(label) else { return };
    let _ = wv.with_webview(move |pw| unsafe {
        use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
        if let Ok(core) = pw.controller().CoreWebView2() {
            for t in ["mousePressed", "mouseReleased"] {
                let params = windows_core::HSTRING::from(format!(r#"{{"type":"{t}","x":{x},"y":{y},"button":"left","clickCount":1}}"#));
                let _ = core.CallDevToolsProtocolMethod(
                    &windows_core::HSTRING::from("Input.dispatchMouseEvent"),
                    &params,
                    &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(()))),
                );
            }
        }
    });
}

/// Kehitystesti: ajaa JS:n annetussa webviewissä ja kirjoittaa tuloksen %TEMP%\chilly-probe.txt.
#[cfg(debug_assertions)]
pub fn dev_probe(app: &AppHandle, label: &str, js: String) {
    let out = std::env::temp_dir().join("chilly-probe.txt");
    let Some(wv) = app.get_webview(label) else {
        let _ = std::fs::write(&out, format!("no webview {label}"));
        return;
    };
    let _ = wv.with_webview(move |pw| unsafe {
        use webview2_com::ExecuteScriptCompletedHandler;
        if let Ok(core) = pw.controller().CoreWebView2() {
            let js = windows_core::HSTRING::from(js);
            let _ = core.ExecuteScript(
                &js,
                &ExecuteScriptCompletedHandler::create(Box::new(move |_e, res| {
                    let _ = std::fs::write(&out, res);
                    Ok(())
                })),
            );
        }
    });
}

fn tab_login(app: &AppHandle, id: u32) -> Option<String> {
    app.state::<ViewerSt>().lock().unwrap().tabs.iter().find(|t| t.id == id).map(|t| t.login.clone())
}

/// Striminäkymä siirtyi toisen kanavan sivulle (raid): välilehti ja chat seuraavat mukana.
/// Jos kohdekanava on jo auki toisella välilehdellä, vältetään kaksoisvälilehti.
/// Katseluputken striimin raidatessa ei-liekitetylle kanavalle siitä tulee raidivieras.
pub fn follow_nav(app: &AppHandle, id: u32, login: &str) {
    let login = login.to_lowercase();
    if login.len() < 2 || login.len() > 25 || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return;
    }
    let is_streak = crate::is_streak_login(app, &login);
    let mut wake = false;
    let mut dedup_close = false;
    {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        let Some(pos) = v.tabs.iter().position(|t| t.id == id) else { return };
        if v.tabs[pos].login == login {
            return;
        }
        let old_login = v.tabs[pos].login.clone();
        let was_auto = v.tabs[pos].auto_kind != AutoKind::Manual;
        let is_multi = v.tabs[pos].child || v.tabs[pos].partner.is_some();

        // Kaksoisvälilehden esto: jos kohdekanava on jo auki toisella välilehdellä
        if !is_multi {
            if let Some(other_pos) = v.tabs.iter().position(|t| t.id != id && t.login == login) {
                let other_id = v.tabs[other_pos].id;
                let other_child = v.tabs[other_pos].child;
                if was_auto {
                    crate::log::info(format!("raid dedup (auto): {old_login} -> {login} already open"));
                    dedup_close = true;
                } else if !other_child {
                    crate::log::info(format!("raid dedup (manual): {old_login} -> {login} switching to tab {other_id}"));
                    if v.tabs[other_pos].auto_kind == AutoKind::RaidGuest {
                        v.tabs[other_pos].auto_kind = AutoKind::Manual;
                        v.tabs[other_pos].raid_seq = 0;
                    }
                    if v.active == Some(id) {
                        v.active = Some(other_id);
                    }
                    dedup_close = true;
                }
            }
        }
        if !dedup_close {
            let new_seq = if was_auto && !is_streak {
                v.raid_seq += 1;
                v.raid_seq
            } else {
                0
            };
            let t = &mut v.tabs[pos];
            t.login = login.clone();
            t.name = login.clone();
            t.image = String::new();
            t.offline = false;
            if was_auto {
                if is_streak {
                    t.auto_kind = AutoKind::Streak;
                    t.raid_seq = 0;
                    crate::log::info(format!("streak raid -> flamed channel: {old_login} -> {login}"));
                } else {
                    t.auto_kind = AutoKind::RaidGuest;
                    t.raid_seq = new_seq;
                    crate::log::info(format!("streak raid -> raid guest: {old_login} -> {login}"));
                }
                wake = true;
            }
        }
    }
    if dedup_close {
        close_tab_auto(app, id);
        crate::wake_streak(app);
        return;
    }
    if let Some(cw) = app.get_webview(&chat_label(id)) {
        if let Ok(u) = Url::parse(&format!("https://www.twitch.tv/popout/{login}/chat?darkpopout")) {
            let _ = cw.navigate(u);
        }
    }
    apply(app);
    if wake {
        crate::wake_streak(app);
    }
    // Nimi ja kuva haetaan Twitchistä taustalla
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some((name, image)) = crate::lookup_channel(&app, &login).await {
            {
                let st = app.state::<ViewerSt>();
                let mut v = st.lock().unwrap();
                if let Some(t) = v.tabs.iter_mut().find(|t| t.id == id && t.login == login) {
                    t.name = name;
                    t.image = image;
                }
            }
            apply(&app);
        }
    });
}

/// UI kertoo, onko välilehden kanava offline (näytetään oma kanavasivu).
pub fn set_offline(app: &AppHandle, id: u32, offline: bool) {
    let changed = {
        let st = app.state::<ViewerSt>();
        let mut v = st.lock().unwrap();
        match v.tabs.iter_mut().find(|t| t.id == id) {
            Some(t) if t.offline != offline => {
                t.offline = offline;
                true
            }
            _ => false,
        }
    };
    if changed {
        apply(app);
    }
}

/// Toistaa välilehdellä klipin (tai palaa kanavalle, kun url = None).
pub fn play_url(app: &AppHandle, id: u32, url: Option<&str>) -> Result<(), String> {
    let login = tab_login(app, id).ok_or(tr("Välilehteä ei löydy", "Tab not found"))?;
    let target = match url {
        Some(u) => u.to_string(),
        None => format!("https://www.twitch.tv/{login}"),
    };
    let wv = app.get_webview(&label(id)).ok_or(tr("Streamia ei löydy", "Stream not found"))?;
    wv.navigate(Url::parse(&target).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    set_offline(app, id, url.is_none());
    Ok(())
}

pub fn reload_tab(app: &AppHandle, id: u32) {
    if let Some(login) = tab_login(app, id) {
        if let Some(wv) = app.get_webview(&label(id)) {
            if let Ok(u) = Url::parse(&format!("https://www.twitch.tv/{login}")) {
                let _ = wv.navigate(u);
            }
        }
    }
}

/// Seuraa / lopeta seuraaminen painamalla Twitchin omaa nappia kanavasivulla
/// (Twitch ei salli seuraamista virallisen rajapinnan kautta).
pub fn press_follow(app: &AppHandle, id: u32, follow: bool) -> Result<(), String> {
    let wv = app.get_webview(&label(id)).ok_or(tr("Streamia ei löydy", "Stream not found"))?;
    let js = format!(
        r#"(async () => {{
  const want = {follow};
  const q = (s) => document.querySelector(s);
  for (let i = 0; i < 40; i++) {{
    const f = q('[data-a-target="follow-button"]'), u = q('[data-a-target="unfollow-button"]');
    if (want && f) {{ f.click(); return; }}
    if (!want && u) {{
      u.click();
      for (let j = 0; j < 30; j++) {{
        await new Promise((r) => setTimeout(r, 150));
        const c = q('[data-a-target="modal-unfollow-button"]');
        if (c) {{ c.click(); return; }}
      }}
      return;
    }}
    if ((want && u) || (!want && f)) return;
    await new Promise((r) => setTimeout(r, 250));
  }}
}})();"#
    );
    wv.eval(js).map_err(|e| e.to_string())
}

/// Avaa Chillyn sisäisen ikkunan (tilaus, klippieditori).
pub fn open_aux_window(app: &AppHandle, label: &str, title: &str, url: &str, w: f64, h: f64) -> Result<(), String> {
    open_aux(app, label, title, url, w, h).map(|_| ())
}

fn open_aux(app: &AppHandle, label: &str, title: &str, url: &str, w: f64, h: f64) -> Result<tauri::WebviewWindow, String> {
    let url = Url::parse(url).map_err(|e| e.to_string())?;
    if let Some(win) = app.get_webview_window(label) {
        let _ = win.navigate(url);
        let _ = win.set_title(title);
        let _ = win.unminimize();
        let _ = win.set_focus();
        return Ok(win);
    }
    WebviewWindowBuilder::new(app, label, WebviewUrl::External(url))
        .title(title)
        .inner_size(w, h)
        .center()
        .additional_browser_args(browser_args())
        // Ei omaa new_window-käsittelijää: tilaussivun maksuikkunat (esim. PayPal) avautuvat
        // WebView2:n omina ikkunoina samassa istunnossa.
        .build()
        .map_err(|e| e.to_string())
}

/// Chatin lisäasetukset (FrankerFaceZ:n ohjauskeskus) omaan ikkunaansa. Sivu on Twitchin chat-popout,
/// johon ladataan FFZ samalla tavalla kuin Chillyn chatteihin; asetukset tallentuvat samaan
/// twitch.tv-tallennustilaan, joten ne koskevat kaikkia Chillyn chatteja.
pub fn open_chat_settings(app: &AppHandle) -> Result<(), String> {
    const LABEL: &str = "ffzsettings";
    if let Some(win) = app.get_webview_window(LABEL) {
        let _ = win.unminimize();
        let _ = win.set_focus();
        return Ok(());
    }
    let url = Url::parse("https://www.twitch.tv/popout/frankerfacez/chat?ffz-settings=chat.input").map_err(|e| e.to_string())?;
    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url))
        .title(tr("Chatin lisäasetukset – Chilly", "Chat settings – Chilly"))
        .inner_size(1000.0, 760.0)
        .center()
        .additional_browser_args(browser_args())
        .initialization_script(format!("{}\n{}", addons_js(), CHAT_JS))
        .build()
        .map_err(|e| e.to_string())?;
    // Kuunnellaan chat.js:n chilly:open:-viestit, jotta FFZ-ohjauskeskuksen linkit aukeavat selaimeen
    let app2 = app.clone();
    let _ = win.with_webview(move |pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2, ICoreWebView2WebMessageReceivedEventArgs};
        use webview2_com::WebMessageReceivedEventHandler;
        let Ok(core) = pw.controller().CoreWebView2() else { return };
        let handler = WebMessageReceivedEventHandler::create(Box::new(
            move |_: Option<ICoreWebView2>, args: Option<ICoreWebView2WebMessageReceivedEventArgs>| {
                let Some(args) = args else { return Ok(()) };
                let mut p = windows_core::PWSTR::null();
                if args.TryGetWebMessageAsString(&mut p).is_ok() && !p.is_null() {
                    let msg = webview2_com::take_pwstr(p);
                    if let Some(url) = msg.strip_prefix("chilly:open:") {
                        let (app, url) = (app2.clone(), url.to_string());
                        tauri::async_runtime::spawn(async move { open_link(&app, &url) });
                    }
                }
                Ok(())
            },
        ));
        let mut token = 0i64;
        let _ = core.add_WebMessageReceived(&handler, &mut token);
    });
    Ok(())
}

/// Mod View omaan ikkunaansa. Sen oma soitin mykistetään (ääni kuuluu jo Chillyn streamista).
pub fn open_mod_view(app: &AppHandle, login: &str) -> Result<(), String> {
    let title = format!("Mod View: {login} – Chilly");
    let win = open_aux(app, "modview", &title, &format!("https://www.twitch.tv/moderator/{login}"), 1400.0, 900.0)?;
    let _ = win.with_webview(|pw| unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_8;
        use windows_core::Interface;
        if let Ok(core) = pw.controller().CoreWebView2() {
            if let Ok(c8) = core.cast::<ICoreWebView2_8>() {
                let _ = c8.SetIsMuted(true);
            }
        }
    });
    Ok(())
}
