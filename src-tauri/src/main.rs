#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod claim;
mod i18n;
mod log;
mod notify;
mod twitch;
mod update;
mod viewer;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;
use twitch::{ApiError, Stream};

/// Palauttaa true, jos Chilly pyörii Microsoft Store / MSIX -pakettina.
pub fn is_msix() -> bool {
    static CACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn GetCurrentPackageFullName(len: *mut u32, name: *mut u16) -> i32;
            }
            let mut len: u32 = 0;
            // APPMODEL_ERROR_NO_PACKAGE = 15700; 0 (OK) tai 122 (ERROR_INSUFFICIENT_BUFFER) = MSIX-paketti
            let rc = unsafe { GetCurrentPackageFullName(&mut len, std::ptr::null_mut()) };
            rc != 15700
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    })
}

const FOLLOW_REFRESH: Duration = Duration::from_secs(10 * 60);
/// Jos stream katkeaa ja palaa tämän ajan sisällä, ei uutta ilmoitusta.
const RECONNECT_GRACE: Duration = Duration::from_secs(10 * 60);
/// Auto-katselun välilehti suljetaan, kun kanava on ollut offline näin kauan (lyhyt katkos ei sulje)
const AUTO_CLOSE_GRACE: Duration = Duration::from_secs(3 * 60);
/// Twitch laskee katsotuksi enintään 2 striimiä kerrallaan → Katseluputki tähtää tähän kiintiöön.
const STREAK_QUOTA: usize = 2;

// ---------- Tallennettavat asetukset ----------

#[derive(Serialize, Deserialize, Clone, Default)]
struct Auth {
    access_token: String,
    refresh_token: String,
    user_id: String,
    login: String,
    display_name: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
struct Pref {
    enabled: bool,
    /// "" = käytä yleistä ääntä
    sound: String,
    /// Auto-katselu: kanava avataan taustalle mykistettynä, kun se menee liveen
    auto: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Manual {
    id: String,
    login: String,
    name: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct Settings {
    auth: Option<Auth>,
    prefs: HashMap<String, Pref>,
    manual: Vec<Manual>,
    sound: String,
    interval_secs: u64,
    /// Kuinka kauan ilmoitus näkyy (s), 0 = kunnes suljetaan
    popup_secs: u64,
    /// "right" | "left" – kanavalista on aina vastakkaisella puolella
    chat_side: String,
    /// Automaattiset lunastukset (oletuksena päällä)
    auto_bonus: bool,
    auto_drops: bool,
    /// Chatin emojilisäosat (FFZ on aina päällä)
    emote_bttv: bool,
    emote_7tv: bool,
    /// "fi" | "en" ("" = tunnistetaan ensimmäisellä käynnistyksellä)
    lang: String,
    /// Multistreamin oletukset: ääni molemmista / oma chat kummallekin
    multi_audio_both: bool,
    multi_chat_both: bool,
    /// Laitekiihdytys (GPU) striimeille, oletuksena päällä. Muutos vaatii uudelleenkäynnistyksen.
    hw_accel: bool,
    /// Twitchin videomainosten esto (vaft), oletuksena pois. Koskee uusia striimejä / uudelleenkäynnistystä.
    adblock: bool,
    /// Äänenvahvistin prosentteina: 100 (pois), 150, 200 tai 300
    volume_boost: u32,
    /// Yläpalkin Mod View -nappi (oletuksena pois)
    mod_view: bool,
    /// Ei välilehtirajaa (oletuksena pois = enintään 10 striimiä)
    unlimited_tabs: bool,
    /// Katseluputken tärkeysjärjestys (kanavien user_id:t; ylimmät saavat etusijan 2 striimin kiintiössä)
    streak_order: Vec<String>,
    /// Tallennettu pääikkunan koko, sijainti ja maksimointi
    window: Option<viewer::WindowBounds>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            auth: None,
            prefs: HashMap::new(),
            manual: Vec::new(),
            sound: "win:Default".into(),
            interval_secs: 60,
            popup_secs: 10,
            chat_side: "right".into(),
            auto_bonus: true,
            auto_drops: true,
            emote_bttv: true,
            emote_7tv: true,
            lang: String::new(),
            multi_audio_both: false,
            multi_chat_both: false,
            hw_accel: true,
            adblock: false,
            volume_boost: 100,
            mod_view: false,
            unlimited_tabs: false,
            streak_order: Vec::new(),
            window: None,
        }
    }
}

/// Pitää `streak_order`-listan synkassa `prefs`-kartan `auto`-lippujen kanssa.
fn sync_streak_order(s: &mut Settings) {
    let mut seen = HashSet::new();
    s.streak_order.retain(|id| {
        s.prefs.get(id).map_or(false, |p| p.auto) && seen.insert(id.clone())
    });
    let mut extra: Vec<String> = s.prefs.iter()
        .filter(|(id, p)| p.auto && !seen.contains(*id))
        .map(|(id, _)| id.clone())
        .collect();
    extra.sort();
    s.streak_order.extend(extra);
}

// ---------- Ajonaikainen tila ----------

#[derive(Clone, Serialize)]
struct Channel {
    id: String,
    login: String,
    name: String,
    image: String,
    manual: bool,
}

#[derive(Default)]
struct Inner {
    settings: Settings,
    channels: Vec<Channel>,
    live: HashMap<String, Stream>,
    offline_since: HashMap<String, Instant>,
    seeded: bool,
    last_follow_refresh: Option<Instant>,
    status: String,
    login_pending: Option<(String, String)>,
    login_gen: u64,
    popups: Vec<notify::PopupItem>,
    /// Oma profiilikuva sivupalkin alalaitaan (haetaan kerran)
    me_image: String,
    /// Aloitusanimaatio näytetään kerran, jos ikkuna avattiin näkyvänä käynnistyksessä
    splash_pending: bool,
    /// Katseluputki: käyttäjän itse sulkemat kanavat (login) – ei avata uudelleen saman liven aikana
    auto_dismissed: HashSet<String>,
    /// Raidivieraat (login), joiden on havaittu olevan offline (suljetaan 3 min katkosvaran jälkeen)
    guest_offline_since: HashMap<String, Instant>,
    /// Ikkunan sijainnin tallennuksen rajoitin (ettei raahaus kirjoita levylle joka pikselillä)
    last_win_save: Option<Instant>,
}

/// Ilmoituksen kesto työkaluilmoituksille (klippi, drop): 0 = "Kunnes suljen" säilytetään.
fn tool_popup_secs(popup_secs: u64) -> u64 {
    if popup_secs == 0 { 0 } else { popup_secs.max(8) }
}

/// Onko kanavalla Katseluputki-liekki päällä (käytetään raidin luokitteluun viewer.rs:ssä).
pub fn is_streak_login(app: &AppHandle, login: &str) -> bool {
    let st = app.state::<St>();
    let i = st.inner.lock().unwrap();
    let Some(ch) = i.channels.iter().find(|c| c.login.eq_ignore_ascii_case(login)) else { return false };
    i.settings.prefs.get(&ch.id).map_or(false, |p| p.auto)
}

/// Merkitsee Katseluputki-kanavan käyttäjän sulkemaksi tämän liven ajaksi.
pub fn mark_streak_dismissed(app: &AppHandle, login: &str) {
    let st = app.state::<St>();
    st.inner.lock().unwrap().auto_dismissed.insert(login.to_lowercase());
}

/// Ajaa Katseluputken kiintiötasapainotuksen taustalla heti (ilman odotusta seuraavaan Twitch-kierrokseen).
pub fn wake_streak(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let st = app.state::<St>().inner().clone();
        reconcile_streaks(&st, &app);
    });
}

struct App {
    inner: Mutex<Inner>,
    wake: tokio::sync::Notify,
    http: reqwest::Client,
    data_dir: PathBuf,
}

type St = Arc<App>;

impl App {
    fn save(&self) {
        let s = self.inner.lock().unwrap().settings.clone();
        let _ = std::fs::create_dir_all(&self.data_dir);
        let tmp = self.data_dir.join("settings.json.tmp");
        if std::fs::write(&tmp, serde_json::to_vec_pretty(&s).unwrap()).is_ok() {
            let _ = std::fs::rename(&tmp, self.data_dir.join("settings.json"));
        }
    }

    fn load(data_dir: &PathBuf) -> Settings {
        std::fs::read(data_dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
}

// ---------- Twitch-kierros ----------

async fn poll_once(st: &App, app: &AppHandle) -> Result<(), ApiError> {
    let (token, user_id, manual, need_follows) = {
        let i = st.inner.lock().unwrap();
        let Some(a) = &i.settings.auth else { return Ok(()) };
        let need = i.channels.is_empty()
            || i.last_follow_refresh.map_or(true, |t| t.elapsed() > FOLLOW_REFRESH);
        (a.access_token.clone(), a.user_id.clone(), i.settings.manual.clone(), need)
    };

    if need_follows && st.inner.lock().unwrap().me_image.is_empty() {
        if let Ok(u) = twitch::me(&st.http, &token).await {
            st.inner.lock().unwrap().me_image = u.profile_image_url;
        }
    }

    if need_follows {
        let followed = twitch::followed_channels(&st.http, &token, &user_id).await?;
        let mut seen = HashSet::new();
        let mut chans: Vec<Channel> = Vec::new();
        for f in followed {
            seen.insert(f.id.clone());
            chans.push(Channel { id: f.id, login: f.login, name: f.name, image: String::new(), manual: false });
        }
        for m in &manual {
            if seen.insert(m.id.clone()) {
                chans.push(Channel { id: m.id.clone(), login: m.login.clone(), name: m.name.clone(), image: String::new(), manual: true });
            }
        }
        let ids: Vec<String> = chans.iter().map(|c| c.id.clone()).collect();
        let users = twitch::users_by_ids(&st.http, &token, &ids).await?;
        let imgs: HashMap<String, String> = users.into_iter().map(|u| (u.id, u.profile_image_url)).collect();
        for c in &mut chans {
            c.image = imgs.get(&c.id).cloned().unwrap_or_default();
        }
        chans.sort_by_key(|c| c.name.to_lowercase());
        let mut i = st.inner.lock().unwrap();
        i.channels = chans;
        i.last_follow_refresh = Some(Instant::now());
    }

    let mut streams = twitch::live_followed(&st.http, &token, &user_id).await?;
    let manual_ids: Vec<String> = {
        let i = st.inner.lock().unwrap();
        i.channels.iter().filter(|c| c.manual).map(|c| c.id.clone()).collect()
    };
    if !manual_ids.is_empty() {
        let have: HashSet<String> = streams.iter().map(|s| s.user_id.clone()).collect();
        let extra: Vec<String> = manual_ids.into_iter().filter(|i| !have.contains(i)).collect();
        if !extra.is_empty() {
            streams.extend(twitch::streams_by_ids(&st.http, &token, &extra).await?);
        }
    }

    // Vertaa edelliseen ja kerää ilmoitettavat
    let mut to_notify: Vec<(Stream, String, String)> = Vec::new(); // (stream, ääni, kuva-url)
    {
        let mut i = st.inner.lock().unwrap();
        let new_live: HashMap<String, Stream> = streams.into_iter().map(|s| (s.user_id.clone(), s)).collect();
        let now = Instant::now();
        let gone: Vec<String> = i.live.keys().filter(|k| !new_live.contains_key(*k)).cloned().collect();
        for id in gone {
            i.offline_since.insert(id, now);
        }
        // Siivotaan vanhoja offline_since-merkintöjä (>24 h)
        i.offline_since.retain(|id, t| !new_live.contains_key(id) && t.elapsed() < Duration::from_secs(24 * 3600));
        // Jos käyttäjän sulkema Katseluputki-kanava on ollut offline yli katkosvaran, sallitaan sen avautuminen taas seuraavalla livellä
        let reset_dismissed: Vec<String> = i.channels.iter()
            .filter(|c| !new_live.contains_key(&c.id) && i.offline_since.get(&c.id).map_or(true, |t| t.elapsed() >= AUTO_CLOSE_GRACE))
            .map(|c| c.login.clone())
            .collect();
        for login in reset_dismissed {
            i.auto_dismissed.remove(&login);
        }
        if i.seeded {
            for (id, s) in &new_live {
                if i.live.contains_key(id) {
                    continue;
                }
                let recent = i.offline_since.get(id).map_or(false, |t| t.elapsed() < RECONNECT_GRACE);
                let pref = i.settings.prefs.get(id).cloned().unwrap_or_default();
                if pref.enabled && !recent {
                    let sound = if pref.sound.is_empty() { i.settings.sound.clone() } else { pref.sound };
                    let img = i.channels.iter().find(|c| &c.id == id).map(|c| c.image.clone()).unwrap_or_default();
                    to_notify.push((s.clone(), sound, img));
                }
            }
        }
        i.live = new_live;
        i.seeded = true;
        i.status.clear();
    }

    // Tarkistetaan myös taustan raidivieraiden live-tila (voivat olla seuraamattomia kanavia)
    let guest_logins: Vec<String> = {
        let (tabs, _, _) = viewer::snapshot_tabs(app);
        tabs.into_iter()
            .filter(|t| t.auto_kind == viewer::AutoKind::RaidGuest)
            .map(|t| t.login)
            .collect()
    };
    if !guest_logins.is_empty() {
        let guest_live: HashSet<String> = match twitch::streams_by_logins(&st.http, &token, &guest_logins).await {
            Ok(v) => v.into_iter().map(|s| s.user_login.to_lowercase()).collect(),
            Err(_) => guest_logins.iter().cloned().collect(), // verkkovirhe → ei suljeta varmuuden vuoksi
        };
        let mut i = st.inner.lock().unwrap();
        let now = Instant::now();
        for gl in &guest_logins {
            if guest_live.contains(gl) {
                i.guest_offline_since.remove(gl);
            } else {
                i.guest_offline_since.entry(gl.clone()).or_insert(now);
            }
        }
        let active_set: HashSet<&String> = guest_logins.iter().collect();
        i.guest_offline_since.retain(|k, _| active_set.contains(k));
    } else {
        st.inner.lock().unwrap().guest_offline_since.clear();
    }

    reconcile_streaks(st, app);

    for (s, sound, img) in to_notify {
        notify_live(st, app, notify::PopupItem {
            name: s.user_name,
            login: s.user_login,
            title: s.title,
            game: s.game_name,
            viewers: Some(s.viewer_count),
            image: img,
            secs: 0,
            ..Default::default()
        }, &sound);
    }
    Ok(())
}

/// Katseluputken kiintiötasapainotus (Twitchin 2 striimin raja, tärkeysjärjestys & raidivieraat):
/// - Sulkee offlineksi menneet Katseluputki- ja raidivieras-välilehdet (3 min katkosvara).
/// - Laskee käyttäjän itse avaamat live-striimit (sekä näkyvän päällimmäisen välilehden) kiintiöön.
/// - Jos tilaa tarvitaan tärkeämmälle liekitetylle (tai live-striimejä on >2), sulkee ensin
///   vanhimmat taustan raidivieraat, sitten matalamman tärkeyden taustan liekitetyt.
/// - Käyttäjän itse avaamia striimejä ei suljeta koskaan.
fn reconcile_streaks(st: &App, app: &AppHandle) {
    let (tabs, active_id, main_visible) = viewer::snapshot_tabs(app);

    let (to_close_offline, desired_all, live_logins): (Vec<u32>, Vec<(String, String, String)>, HashSet<String>) = {
        let mut i = st.inner.lock().unwrap();
        if !i.seeded || i.settings.auth.is_none() {
            return;
        }
        sync_streak_order(&mut i.settings);

        let live_by_login: HashSet<String> = i.live.values().map(|s| s.user_login.to_lowercase()).collect();
        let ch_id_by_login: HashMap<String, String> = i.channels.iter().map(|c| (c.login.to_lowercase(), c.id.clone())).collect();

        // 1. Offline menneet auto-välilehdet (Streak + RaidGuest) kiinni 3 min katkosvaran jälkeen
        let mut offline_close = Vec::new();
        for t in &tabs {
            match t.auto_kind {
                viewer::AutoKind::Manual => {}
                viewer::AutoKind::Streak => {
                    let l = t.login.to_lowercase();
                    if !live_by_login.contains(&l) {
                        let ok_grace = ch_id_by_login.get(&l)
                            .and_then(|id| i.offline_since.get(id))
                            .map_or(true, |ts| ts.elapsed() >= AUTO_CLOSE_GRACE);
                        if ok_grace {
                            offline_close.push(t.id);
                        }
                    }
                }
                viewer::AutoKind::RaidGuest => {
                    let l = t.login.to_lowercase();
                    if let Some(ts) = i.guest_offline_since.get(&l) {
                        if ts.elapsed() >= AUTO_CLOSE_GRACE {
                            offline_close.push(t.id);
                        }
                    }
                }
            }
        }

        // 2. Live-tilassa olevat liekitetyt tärkeysjärjestyksessä (pois lukien käyttäjän sulkemat)
        let mut desired = Vec::new();
        for id in &i.settings.streak_order {
            if let Some(s) = i.live.get(id) {
                let l = s.user_login.to_lowercase();
                if i.auto_dismissed.contains(&l) {
                    continue;
                }
                let img = i.channels.iter().find(|c| &c.id == id).map(|c| c.image.clone()).unwrap_or_default();
                desired.push((l, s.user_name.clone(), img));
            }
        }
        (offline_close, desired, live_by_login)
    };

    for id in &to_close_offline {
        log::info(format!("streak: closing offline auto tab {id}"));
        viewer::close_tab_auto(app, *id);
    }

    // Luetaan ajantasainen välilehtilista offline-siivouksen jälkeen
    let (tabs, active_id, main_visible) = if to_close_offline.is_empty() {
        (tabs, active_id, main_visible)
    } else {
        viewer::snapshot_tabs(app)
    };

    let active_partner = active_id.and_then(|aid| tabs.iter().find(|t| t.id == aid)).and_then(|t| t.partner);
    let is_foreground = |t: &viewer::Tab| -> bool {
        t.popped || (main_visible && (Some(t.id) == active_id || Some(t.id) == active_partner))
    };

    // Suojatut live-välilehdet: käyttäjän omat (Manual) + näkyvässä ikkunassa katsottavat
    let mut protected_live_count: usize = 0;
    let mut protected_logins: HashSet<String> = HashSet::new();
    let mut bg_guests: Vec<viewer::Tab> = Vec::new();
    let mut bg_streaks: Vec<viewer::Tab> = Vec::new();

    for t in &tabs {
        let l = t.login.to_lowercase();
        let is_live_tab = !t.offline || live_logins.contains(&l);
        if t.auto_kind == viewer::AutoKind::Manual || is_foreground(t) {
            if is_live_tab {
                protected_live_count += 1;
            }
            protected_logins.insert(l);
        } else if t.auto_kind == viewer::AutoKind::RaidGuest {
            bg_guests.push(t.clone());
        } else {
            bg_streaks.push(t.clone());
        }
    }

    // Jäljellä olevat paikat Katseluputken taustastriimeille (max 2 yhteensä)
    let remaining = STREAK_QUOTA.saturating_sub(protected_live_count);
    let target_bg: Vec<(String, String, String)> = desired_all.into_iter()
        .filter(|(l, _, _)| !protected_logins.contains(l))
        .take(remaining)
        .collect();
    let target_set: HashSet<String> = target_bg.iter().map(|(l, _, _)| l.clone()).collect();

    // 3. Tilanteko:
    //    - Matalamman tärkeyden taustan liekitetyt (jotka eivät mahdu top-kiintiöön) suljetaan.
    //    - Taustan raidivieraat suljetaan vanhin ensin, kun tilaa tarvitaan (joko liekitetyille
    //      tai jos elossa olevia striimejä on yhteensä yli 2).
    let mut close_ids: Vec<u32> = Vec::new();
    let mut kept_bg_streaks = 0usize;
    for t in &bg_streaks {
        if target_set.contains(&t.login.to_lowercase()) {
            kept_bg_streaks += 1;
        } else {
            log::info(format!("streak: evicting lower-priority tab {} ({})", t.id, t.login));
            close_ids.push(t.id);
        }
    }

    let slots_needed_for_streaks = target_bg.len();
    let guest_budget = remaining.saturating_sub(slots_needed_for_streaks);
    bg_guests.sort_by_key(|t| (t.raid_seq, t.id)); // vanhin ensin
    if bg_guests.len() > guest_budget {
        let evict_n = bg_guests.len() - guest_budget;
        for g in bg_guests.iter().take(evict_n) {
            log::info(format!("streak: evicting raid guest tab {} ({})", g.id, g.login));
            close_ids.push(g.id);
        }
    }

    for id in close_ids {
        viewer::close_tab_auto(app, id);
    }

    // 4. Avataan puuttuvat tärkeimmät liekitetyt taustalle
    let open_now: HashSet<String> = if kept_bg_streaks < target_bg.len() {
        viewer::snapshot_tabs(app).0.into_iter().map(|t| t.login.to_lowercase()).collect()
    } else {
        return;
    };
    for (login, name, img) in target_bg {
        if open_now.contains(&login) {
            continue;
        }
        log::info(format!("streak: opening {login}"));
        if let Err(e) = viewer::open_auto(app, &login, &name, &img) {
            log::warn(format!("streak open failed for {login}: {e}"));
            let _ = app.emit("viewer-error", e);
        }
    }
}

/// Lisää ilmoituksen jonoon, soittaa äänen ja herättää ilmoitusikkunan.
fn notify_live(st: &App, app: &AppHandle, mut item: notify::PopupItem, sound: &str) {
    {
        let mut i = st.inner.lock().unwrap();
        item.secs = i.settings.popup_secs;
        i.popups.push(item);
    }
    notify::play_sound(sound);
    notify::poke(app);
}

async fn refresh_tokens(st: &App) -> bool {
    let Some(rt) = st.inner.lock().unwrap().settings.auth.as_ref().map(|a| a.refresh_token.clone()) else {
        return false;
    };
    match twitch::refresh(&st.http, &rt).await {
        Ok(t) => {
            if let Some(a) = st.inner.lock().unwrap().settings.auth.as_mut() {
                a.access_token = t.access_token;
                a.refresh_token = t.refresh_token;
            }
            st.save();
            true
        }
        Err(ApiError::Unauthorized) => {
            {
                let mut i = st.inner.lock().unwrap();
                i.settings.auth = None;
                i.channels.clear();
                i.live.clear();
                i.status = i18n::tr("Twitch-kirjautuminen vanheni – kirjaudu uudelleen.", "Your Twitch login expired – log in again.").into();
            }
            st.save();
            false
        }
        Err(e) => {
            st.inner.lock().unwrap().status = e.to_string();
            false
        }
    }
}

async fn run_cycle(st: &App, app: &AppHandle) {
    match poll_once(st, app).await {
        Ok(()) => {}
        Err(ApiError::Unauthorized) => {
            if refresh_tokens(st).await {
                if let Err(e) = poll_once(st, app).await {
                    st.inner.lock().unwrap().status = e.to_string();
                }
            }
        }
        Err(e) => {
            log::warn(format!("poll error: {e}"));
            st.inner.lock().unwrap().status = e.to_string();
        }
    }
    changed(app);
}

// ---------- Tray ja UI-päivitys ----------

fn changed(app: &AppHandle) {
    let _ = app.emit("state", ());
    let _ = app.emit("viewer-state", ());
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = update_tray(&app2) {
            eprintln!("tray: {e}");
        }
    });
}

fn update_tray(app: &AppHandle) -> tauri::Result<()> {
    let st = app.state::<St>();
    let (mut live, logged_in): (Vec<(bool, Stream)>, bool) = {
        let i = st.inner.lock().unwrap();
        (
            i.live
                .values()
                .map(|s| (i.settings.prefs.get(&s.user_id).map_or(false, |p| p.enabled), s.clone()))
                .collect(),
            i.settings.auth.is_some(),
        )
    };
    live.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.viewer_count.cmp(&a.1.viewer_count)));

    let menu = Menu::new(app)?;
    if !logged_in {
        menu.append(&MenuItem::with_id(app, "noop", i18n::tr("Ei kirjautunut", "Not logged in"), false, None::<&str>)?)?;
    } else if live.is_empty() {
        menu.append(&MenuItem::with_id(app, "noop", i18n::tr("Kukaan ei ole livenä", "Nobody is live"), false, None::<&str>)?)?;
    } else {
        menu.append(&MenuItem::with_id(app, "noop", format!("{} ({})", i18n::tr("Livenä nyt", "Live now"), live.len()), false, None::<&str>)?)?;
        for (on, s) in live.iter().take(25) {
            let mark = if *on { "🔔" } else { "●" };
            let game = if s.game_name.is_empty() { String::new() } else { format!("  ·  {}", s.game_name) };
            let label = format!("{mark}  {}{game}", s.user_name).replace('&', "&&");
            menu.append(&MenuItem::with_id(app, format!("live:{}", s.user_login), label, true, None::<&str>)?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "open", i18n::tr("Avaa Chilly", "Open Chilly"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "refresh", i18n::tr("Päivitä nyt", "Refresh now"), true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "quit", i18n::tr("Lopeta", "Quit"), true, None::<&str>)?)?;

    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
        let tip = if live.is_empty() { "Chilly".to_string() } else { format!("Chilly – {} {}", live.len(), i18n::tr("livenä", "live")) };
        tray.set_tooltip(Some(tip))?;
    }
    Ok(())
}

fn show_main(app: &AppHandle) {
    viewer::show_main(app);
}

// ---------- Komennot käyttöliittymälle ----------

#[tauri::command]
fn get_state(app: AppHandle, st: tauri::State<'_, St>) -> Value {
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);
    let i = st.inner.lock().unwrap();
    let channels: Vec<Value> = i
        .channels
        .iter()
        .map(|c| {
            let p = i.settings.prefs.get(&c.id).cloned().unwrap_or_default();
            json!({
                "id": c.id, "login": c.login, "name": c.name, "image": c.image, "manual": c.manual,
                "enabled": p.enabled, "sound": p.sound, "auto": p.auto,
                "live": i.live.get(&c.id),
            })
        })
        .collect();
    json!({
        "clientIdSet": twitch::client_id_set(),
        "user": i.settings.auth.as_ref().map(|a| json!({"login": a.login, "name": a.display_name, "image": i.me_image})),
        "loginPending": i.login_pending.as_ref().map(|(c, u)| json!({"code": c, "uri": u})),
        "status": i.status,
        "sound": i.settings.sound,
        "interval": i.settings.interval_secs,
        "popupSecs": i.settings.popup_secs,
        "chatSide": i.settings.chat_side,
        "autoBonus": i.settings.auto_bonus,
        "autoDrops": i.settings.auto_drops,
        "lang": i.settings.lang,
        "version": app.package_info().version.to_string(),
        "emoteBttv": i.settings.emote_bttv,
        "emote7tv": i.settings.emote_7tv,
        "multiAudioBoth": i.settings.multi_audio_both,
        "multiChatBoth": i.settings.multi_chat_both,
        "hwAccel": i.settings.hw_accel,
        "adblock": i.settings.adblock,
        "volumeBoost": i.settings.volume_boost,
        "modView": i.settings.mod_view,
        "unlimitedTabs": i.settings.unlimited_tabs,
        "streakOrder": i.settings.streak_order,
        "hwAccelActive": viewer::browser_args().contains("--disable-gpu") == false,
        "autostart": autostart,
        "isStore": is_msix(),
        "loaded": i.seeded,
        "channels": channels,
    })
}

#[tauri::command]
async fn start_login(app: AppHandle, st: tauri::State<'_, St>) -> Result<(), String> {
    if !twitch::client_id_set() {
        return Err(i18n::tr("Client ID puuttuu ohjelmasta.", "The app is missing its Client ID.").into());
    }
    let d = twitch::device_start(&st.http).await.map_err(|e| e.to_string())?;
    let gen = {
        let mut i = st.inner.lock().unwrap();
        i.login_gen += 1;
        i.login_pending = Some((d.user_code.clone(), d.verification_uri.clone()));
        i.status.clear();
        i.login_gen
    };
    // Kirjautuminen Chillyn omassa ikkunassa: Twitch kysyy ensin tunnukset (twitch.tv-istunto
    // chattia ja pisteitä varten) ja palaa sitten vahvistussivulle, jossa painetaan Aktivoi.
    if let Err(e) = viewer::open_twitch_login(&app, &d.verification_uri) {
        let _ = tauri_plugin_opener::open_url(&d.verification_uri, None::<&str>);
        eprintln!("login window: {e}");
    }
    changed(&app);

    let st: St = st.inner().clone();
    tauri::async_runtime::spawn(async move {
        let deadline = Instant::now() + Duration::from_secs(d.expires_in);
        let mut every = d.interval.max(1);
        loop {
            tokio::time::sleep(Duration::from_secs(every)).await;
            if st.inner.lock().unwrap().login_gen != gen {
                return; // peruttu tai uusi yritys
            }
            if Instant::now() > deadline {
                {
                    let mut i = st.inner.lock().unwrap();
                    i.login_pending = None;
                    i.status = i18n::tr("Kirjautumiskoodi vanheni, yritä uudelleen.", "The login code expired, try again.").into();
                }
                viewer::close_twitch_login(&app);
                changed(&app);
                return;
            }
            match twitch::device_poll(&st.http, &d.device_code).await {
                twitch::DevicePoll::Pending => {}
                twitch::DevicePoll::SlowDown => every += 5,
                twitch::DevicePoll::Failed(msg) => {
                    {
                        let mut i = st.inner.lock().unwrap();
                        i.login_pending = None;
                        i.status = msg;
                    }
                    viewer::close_twitch_login(&app);
                    changed(&app);
                    return;
                }
                twitch::DevicePoll::Done(t) => {
                    let me = twitch::me(&st.http, &t.access_token).await;
                    {
                        let mut i = st.inner.lock().unwrap();
                        i.login_pending = None;
                        match me {
                            Ok(u) => {
                                i.settings.auth = Some(Auth {
                                    access_token: t.access_token,
                                    refresh_token: t.refresh_token,
                                    user_id: u.id,
                                    login: u.login,
                                    display_name: u.display_name,
                                });
                                i.channels.clear();
                                i.live.clear();
                                i.seeded = false;
                                i.last_follow_refresh = None;
                            }
                            Err(e) => i.status = e.to_string(),
                        }
                    }
                    st.save();
                    st.wake.notify_one();
                    viewer::close_twitch_login(&app);
                    changed(&app);
                    return;
                }
            }
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_login(app: AppHandle, st: tauri::State<'_, St>) {
    {
        let mut i = st.inner.lock().unwrap();
        i.login_gen += 1;
        i.login_pending = None;
    }
    viewer::close_twitch_login(&app);
    changed(&app);
}

#[tauri::command]
fn logout(app: AppHandle, st: tauri::State<'_, St>) {
    {
        let mut i = st.inner.lock().unwrap();
        i.settings.auth = None;
        i.channels.clear();
        i.live.clear();
        i.auto_dismissed.clear();
        i.guest_offline_since.clear();
        i.seeded = false;
    }
    st.save();
    viewer::close_all(&app);
    viewer::clear_web_session(&app);
    changed(&app);
}

#[tauri::command]
fn set_channel(app: AppHandle, st: tauri::State<'_, St>, id: String, enabled: Option<bool>, sound: Option<String>, auto: Option<bool>) {
    {
        let mut i = st.inner.lock().unwrap();
        let login = i.channels.iter().find(|c| c.id == id).map(|c| c.login.to_lowercase());
        let p = i.settings.prefs.entry(id.clone()).or_default();
        if let Some(e) = enabled {
            p.enabled = e;
        }
        if let Some(s) = sound {
            p.sound = s;
        }
        if let Some(a) = auto {
            p.auto = a;
        }
        if !p.enabled && p.sound.is_empty() && !p.auto {
            i.settings.prefs.remove(&id);
        }
        if auto == Some(true) {
            if !i.settings.streak_order.contains(&id) {
                i.settings.streak_order.push(id.clone());
            }
            if let Some(l) = login {
                i.auto_dismissed.remove(&l);
            }
        } else if auto == Some(false) {
            i.settings.streak_order.retain(|x| x != &id);
            if let Some(l) = login {
                i.auto_dismissed.remove(&l);
            }
        }
        sync_streak_order(&mut i.settings);
    }
    st.save();
    if auto.is_some() {
        wake_streak(&app);
    }
    changed(&app);
}

#[tauri::command]
fn set_streak_order(app: AppHandle, st: tauri::State<'_, St>, ids: Vec<String>) {
    {
        let mut i = st.inner.lock().unwrap();
        let mut next = Vec::with_capacity(ids.len());
        let mut seen = HashSet::new();
        for id in ids {
            if i.settings.prefs.get(&id).map_or(false, |p| p.auto) && seen.insert(id.clone()) {
                next.push(id);
            }
        }
        for id in std::mem::take(&mut i.settings.streak_order) {
            if i.settings.prefs.get(&id).map_or(false, |p| p.auto) && seen.insert(id.clone()) {
                next.push(id);
            }
        }
        i.settings.streak_order = next;
        sync_streak_order(&mut i.settings);
    }
    st.save();
    wake_streak(&app);
    changed(&app);
}

#[tauri::command]
fn set_settings(app: AppHandle, st: tauri::State<'_, St>, sound: Option<String>, interval: Option<u64>, popup_secs: Option<u64>, chat_side: Option<String>, auto_bonus: Option<bool>, auto_drops: Option<bool>, emote_bttv: Option<bool>, emote_seventv: Option<bool>, lang: Option<String>, multi_audio_both: Option<bool>, multi_chat_both: Option<bool>, hw_accel: Option<bool>, adblock: Option<bool>, volume_boost: Option<u32>, mod_view: Option<bool>, unlimited_tabs: Option<bool>) {
    let emotes_changed = emote_bttv.is_some() || emote_seventv.is_some();
    {
        let mut i = st.inner.lock().unwrap();
        if let Some(s) = sound {
            i.settings.sound = s;
        }
        if let Some(v) = interval {
            i.settings.interval_secs = v.clamp(30, 900);
        }
        if let Some(v) = popup_secs {
            i.settings.popup_secs = v.min(600);
        }
        if let Some(v) = auto_bonus {
            i.settings.auto_bonus = v;
        }
        if let Some(v) = auto_drops {
            i.settings.auto_drops = v;
        }
        if let Some(v) = emote_bttv {
            i.settings.emote_bttv = v;
        }
        if let Some(v) = lang {
            i.settings.lang = if v == "en" { "en".into() } else { "fi".into() };
            i18n::set(&i.settings.lang);
        }
        if let Some(v) = emote_seventv {
            i.settings.emote_7tv = v;
        }
        if let Some(v) = multi_audio_both {
            i.settings.multi_audio_both = v;
        }
        if let Some(v) = multi_chat_both {
            i.settings.multi_chat_both = v;
        }
        if let Some(v) = hw_accel {
            i.settings.hw_accel = v;
        }
        if let Some(v) = adblock {
            i.settings.adblock = v;
            viewer::set_adblock(v);
        }
        if let Some(v) = volume_boost {
            i.settings.volume_boost = match v { 150 | 200 | 300 => v, _ => 100 };
        }
        if let Some(v) = mod_view {
            i.settings.mod_view = v;
        }
        if let Some(v) = unlimited_tabs {
            i.settings.unlimited_tabs = v;
            viewer::set_unlimited_tabs(v);
        }
        if let Some(v) = chat_side {
            i.settings.chat_side = match v.as_str() {
                "left" | "below" => v,
                _ => "right".into(),
            };
        }
    }
    st.save();
    if interval.is_some() {
        st.wake.notify_one();
    }
    if volume_boost.is_some() {
        let b = st.inner.lock().unwrap().settings.volume_boost;
        viewer::set_boost(&app, b);
    }
    if emotes_changed {
        let (b, t) = { let i = st.inner.lock().unwrap(); (i.settings.emote_bttv, i.settings.emote_7tv) };
        viewer::set_emotes(&app, b, t, true);
    }
    changed(&app);
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<(), String> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    changed(&app);
    Ok(())
}

#[tauri::command]
async fn add_channel(app: AppHandle, st: tauri::State<'_, St>, login: String) -> Result<(), String> {
    let login = login.trim().trim_end_matches('/').trim_start_matches('@').rsplit('/').next().unwrap_or("").to_lowercase();
    if login.is_empty() {
        return Err(i18n::tr("Anna kanavan nimi.", "Enter a channel name.").into());
    }
    let mut u = None;
    for attempt in 0..2 {
        let token = st.inner.lock().unwrap().settings.auth.as_ref().map(|a| a.access_token.clone()).ok_or(i18n::tr("Kirjaudu ensin.", "Log in first."))?;
        match twitch::user_by_login(&st.http, &token, &login).await {
            Err(ApiError::Unauthorized) if attempt == 0 => {
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
            }
            r => {
                u = r.map_err(|e| e.to_string())?;
                break;
            }
        }
    }
    let u = u.ok_or(if i18n::en() { format!("Channel \"{login}\" not found.") } else { format!("Kanavaa \"{login}\" ei löytynyt.") })?;
    {
        let mut i = st.inner.lock().unwrap();
        if !i.channels.iter().any(|c| c.id == u.id) {
            i.settings.manual.push(Manual { id: u.id.clone(), login: u.login, name: u.display_name });
            i.last_follow_refresh = None;
        }
        i.settings.prefs.entry(u.id).or_default().enabled = true;
    }
    st.save();
    st.wake.notify_one();
    changed(&app);
    Ok(())
}

#[tauri::command]
fn remove_channel(app: AppHandle, st: tauri::State<'_, St>, id: String) {
    {
        let mut i = st.inner.lock().unwrap();
        i.settings.manual.retain(|m| m.id != id);
        i.settings.prefs.remove(&id);
        sync_streak_order(&mut i.settings);
        i.last_follow_refresh = None;
    }
    st.save();
    st.wake.notify_one();
    changed(&app);
}

#[tauri::command]
fn test_notification(app: AppHandle, st: tauri::State<'_, St>, id: Option<String>, sound: Option<String>) {
    let (item, snd) = {
        let i = st.inner.lock().unwrap();
        let ch = id.as_ref().and_then(|id| i.channels.iter().find(|c| &c.id == id).cloned());
        let live = id.as_ref().and_then(|id| i.live.get(id).cloned());
        let ps = id.as_ref().and_then(|id| i.settings.prefs.get(id)).map(|p| p.sound.clone()).unwrap_or_default();
        let snd = sound.filter(|s| !s.is_empty()).unwrap_or(if ps.is_empty() { i.settings.sound.clone() } else { ps });
        let item = match ch {
            Some(c) => notify::PopupItem {
                name: c.name,
                login: c.login,
                title: live.as_ref().map(|l| l.title.clone()).unwrap_or(i18n::tr("Testi-ilmoitus – tältä se näyttää, kun kanava menee liveen.", "Test notification – this is how it looks when a channel goes live.").into()),
                game: live.as_ref().map(|l| l.game_name.clone()).unwrap_or_default(),
                viewers: live.map(|l| l.viewer_count),
                image: c.image,
                secs: 0,
                ..Default::default()
            },
            None => test_item(),
        };
        (item, snd)
    };
    notify_live(&st, &app, item, &snd);
}

/// Ilmoitus uudesta Chilly-versiosta (pysyy, kunnes käyttäjä valitsee Päivitä tai Myöhemmin).
pub fn notify_update(app: &AppHandle, version: &str, notes: &str) {
    let st = app.state::<St>();
    let sound = {
        let mut i = st.inner.lock().unwrap();
        i.popups.push(notify::PopupItem {
            name: version.to_string(),
            title: notes.to_string(),
            kind: "update".into(),
            secs: 0,
            ..Default::default()
        });
        i.settings.sound.clone()
    };
    notify::play_sound(&sound);
    notify::poke(app);
}

/// Chillyn kotisivu (Asetukset → Tietoja)
#[tauri::command]
fn open_website() {
    let _ = tauri_plugin_opener::open_url("https://samppalandia.wtf/chilly/", None::<&str>);
}

/// Avaa Chilly:n loki- ja asetuskansion (Asetukset → Tietoja)
#[tauri::command]
fn open_log_dir(st: tauri::State<'_, St>) -> Result<(), String> {
    let _ = std::fs::create_dir_all(&st.data_dir);
    tauri_plugin_opener::open_path(&st.data_dir, None::<&str>).map_err(|e| e.to_string())
}

/// Avaa Windowsin Käynnistyssovellukset-asetukset (MSIX / Microsoft Store -versiolle)
#[tauri::command]
fn open_startup_settings() -> Result<(), String> {
    tauri_plugin_opener::open_url("ms-settings:startupapps", None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn check_update(app: AppHandle) -> Result<update::UpdInfo, String> {
    update::check(&app).await
}

/// Tallentaa asetukset ja sulkee kaikki striimit/chatit puhtaasti ennen päivitysasentimen käynnistystä.
pub fn prepare_for_update(app: &AppHandle) {
    log::info("prepare_for_update: saving settings and closing all webviews");
    app.state::<St>().save();
    viewer::close_all(app);
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    update::install(&app).await
}

fn test_item() -> notify::PopupItem {
    notify::PopupItem {
        name: i18n::tr("Testikanava", "Test channel").into(),
        login: String::new(),
        title: i18n::tr("Tältä ilmoitus näyttää, kun kanava menee liveen.", "This is how a notification looks when a channel goes live.").into(),
        game: "Just Chatting".into(),
        viewers: Some(1234),
        image: String::new(),
        secs: 0,
        ..Default::default()
    }
}

#[tauri::command]
fn popup_take(st: tauri::State<'_, St>) -> Vec<notify::PopupItem> {
    std::mem::take(&mut st.inner.lock().unwrap().popups)
}

#[tauri::command]
fn popup_layout(app: AppHandle, height: f64) {
    notify::layout(&app, height);
}

#[tauri::command]
async fn pick_sound_file(app: AppHandle) -> Result<Option<String>, String> {
    let r = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title(i18n::tr("Valitse ilmoitusääni", "Choose notification sound"))
            .add_filter(i18n::tr("Äänitiedostot", "Sound files"), &["wav", "mp3", "ogg", "flac"])
            .blocking_pick_file()
            .map(|f| format!("file:{f}"))
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(r)
}

#[tauri::command]
async fn open_channel(app: AppHandle, login: String, name: Option<String>, image: Option<String>) -> Result<(), String> {
    open_stream_with(&app, &login, name, image)
}

/// Avaa kanavan Chillyyn.
fn open_stream(app: &AppHandle, login: &str, _unused: bool) -> Result<(), String> {
    open_stream_with(app, login, None, None)
}

fn open_stream_with(app: &AppHandle, login: &str, name: Option<String>, image: Option<String>) -> Result<(), String> {
    let (name, image) = {
        let st = app.state::<St>();
        let i = st.inner.lock().unwrap();
        let c = i.channels.iter().find(|c| c.login == login);
        (
            c.map(|c| c.name.clone()).or(name).unwrap_or_else(|| login.to_string()),
            c.map(|c| c.image.clone()).or(image).unwrap_or_default(),
        )
    };
    let r = viewer::open(app, login, &name, &image);
    if let Err(e) = &r {
        let _ = app.emit("viewer-error", e.clone());
    }
    r
}

#[tauri::command]
fn viewer_state(app: AppHandle, st: tauri::State<'_, St>) -> Value {
    let mut v = viewer::state_json(&app);
    // Täydennä kanavakuvat, jos välilehti avattiin ennen kuin kanavalista oli ladattu
    let i = st.inner.lock().unwrap();
    if let Some(tabs) = v["tabs"].as_array_mut() {
        for t in tabs {
            if t["image"].as_str().map_or(true, str::is_empty) {
                if let Some(c) = i.channels.iter().find(|c| Some(c.login.as_str()) == t["login"].as_str()) {
                    t["image"] = json!(c.image);
                    t["name"] = json!(c.name);
                }
            }
        }
    }
    v
}

#[tauri::command]
async fn viewer_activate(app: AppHandle, id: u32) {
    viewer::activate(&app, id);
}

#[tauri::command]
async fn viewer_close(app: AppHandle, id: u32) {
    viewer::close_tab_user(&app, id);
}

#[tauri::command]
async fn viewer_mute(app: AppHandle, id: u32) {
    viewer::toggle_mute(&app, id);
}

#[tauri::command]
async fn viewer_layout(app: AppHandle, video: [f64; 4], chat: Option<[f64; 4]>) {
    let t = |r: [f64; 4]| (r[0], r[1], r[2], r[3]);
    viewer::set_layout(&app, t(video), chat.map(t));
}

#[tauri::command]
async fn viewer_popout(app: AppHandle, id: u32) -> Result<(), String> {
    viewer::pop_out(&app, id)
}

#[tauri::command]
async fn viewer_popin(app: AppHandle, id: u32) {
    viewer::pop_in(&app, id);
}

#[tauri::command]
async fn viewer_cover(app: AppHandle, on: bool) {
    viewer::set_covered(&app, on);
}

#[tauri::command]
async fn viewer_twitch_login(app: AppHandle) -> Result<(), String> {
    viewer::open_twitch_login(&app, "https://www.twitch.tv/login")
}

/// Kanavan näyttönimi ja kuva (raidin jälkeen välilehdelle). Seuratuista ensin, muuten Twitchistä.
pub async fn lookup_channel(app: &AppHandle, login: &str) -> Option<(String, String)> {
    let st = app.state::<St>();
    if let Some(c) = st.inner.lock().unwrap().channels.iter().find(|c| c.login == login) {
        return Some((c.name.clone(), c.image.clone()));
    }
    for attempt in 0..2 {
        let (token, _) = with_token(&st).await.ok()?;
        match twitch::user_by_login(&st.http, &token, login).await {
            Ok(Some(u)) => return Some((u.display_name, u.profile_image_url)),
            Ok(None) => return None,
            Err(ApiError::Unauthorized) if attempt == 0 => {
                refresh_tokens(&st).await;
            }
            Err(_) => return None,
        }
    }
    None
}

/// Token komentoja varten; Unauthorized → uusitaan kerran.
async fn with_token(st: &App) -> Result<(String, String), String> {
    let i = st.inner.lock().unwrap();
    let a = i.settings.auth.as_ref().ok_or(i18n::tr("Kirjaudu ensin.", "Log in first."))?;
    Ok((a.access_token.clone(), a.user_id.clone()))
}

async fn resolve_user_api(st: &App, token: &str, login: &str) -> Result<twitch::UserInfo, ApiError> {
    match twitch::user_by_login(&st.http, token, login).await? {
        Some(u) => Ok(u),
        None => Err(ApiError::Other(if i18n::en() { format!("Channel {login} not found.") } else { format!("Kanavaa {login} ei löytynyt.") })),
    }
}

/// Chillyn oman kanavasivun tiedot.
#[tauri::command]
async fn channel_info(st: tauri::State<'_, St>, login: String) -> Result<Value, String> {
    for attempt in 0..2 {
        let (token, me) = with_token(&st).await?;
        let r: Result<Value, ApiError> = async {
            let u = twitch::user_by_login(&st.http, &token, &login).await?.ok_or(ApiError::Other(i18n::tr("Kanavaa ei löytynyt.", "Channel not found.").into()))?;
            let ids = [u.id.clone()];
            let (followers, clips, last, following, live) = tokio::join!(
                twitch::followers_total(&st.http, &token, &u.id),
                twitch::top_clips(&st.http, &token, &u.id),
                twitch::last_broadcast(&st.http, &token, &u.id),
                twitch::is_following(&st.http, &token, &me, &u.id),
                twitch::streams_by_ids(&st.http, &token, &ids),
            );
            Ok(json!({
                "id": u.id, "login": u.login, "name": u.display_name, "image": u.profile_image_url,
                "banner": u.offline_image_url, "description": u.description,
                "followers": followers.ok(), "clips": clips.unwrap_or_default(), "lastLive": last.ok().flatten(),
                "following": following.ok(), "live": live.ok().and_then(|v| v.into_iter().next()),
            }))
        }
        .await;
        match r {
            Err(ApiError::Unauthorized) if attempt == 0 => {
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
            }
            r => return r.map_err(|e| e.to_string()),
        }
    }
    Err(i18n::tr("Tietojen haku epäonnistui.", "Could not load channel info.").into())
}

#[tauri::command]
async fn viewer_add_partner(app: AppHandle, st: tauri::State<'_, St>, id: u32, login: String, name: Option<String>, image: Option<String>) -> Result<(), String> {
    let (name, image, ab, cb) = {
        let i = st.inner.lock().unwrap();
        let c = i.channels.iter().find(|c| c.login == login);
        (
            c.map(|c| c.name.clone()).or(name).unwrap_or_else(|| login.clone()),
            c.map(|c| c.image.clone()).or(image).unwrap_or_default(),
            i.settings.multi_audio_both,
            i.settings.multi_chat_both,
        )
    };
    viewer::add_partner(&app, id, &login, &name, &image, ab, cb)
}

#[tauri::command]
async fn viewer_remove_partner(app: AppHandle, id: u32) {
    viewer::remove_partner(&app, id);
}

#[tauri::command]
async fn viewer_multi(app: AppHandle, id: u32, layout: Option<String>, audio_partner: Option<bool>, chat_partner: Option<bool>, audio_both: Option<bool>, chat_both: Option<bool>) {
    viewer::set_multi(&app, id, layout, audio_partner, chat_partner, audio_both, chat_both);
}

#[tauri::command]
async fn viewer_self_mute(app: AppHandle, id: u32) {
    viewer::toggle_self_mute(&app, id);
}

#[tauri::command]
async fn viewer_swap(app: AppHandle, id: u32) -> Result<(), String> {
    viewer::swap(&app, id)
}

#[tauri::command]
async fn viewer_offline(app: AppHandle, id: u32, offline: bool) {
    viewer::set_offline(&app, id, offline);
}

#[tauri::command]
async fn viewer_play(app: AppHandle, id: u32, url: Option<String>) -> Result<(), String> {
    viewer::play_url(&app, id, url.as_deref())
}

#[tauri::command]
async fn viewer_reload_tab(app: AppHandle, id: u32) {
    viewer::reload_tab(&app, id);
}

/// Käynnistää Chillyn uudelleen (esim. laitekiihdytyksen muutoksen jälkeen).
#[tauri::command]
fn restart_app(app: AppHandle, st: tauri::State<'_, St>) {
    st.save();
    app.restart();
}

/// Seuraa / lopeta seuraaminen. Palauttaa uuden tilan Twitchin mukaan.
#[tauri::command]
async fn follow_channel(app: AppHandle, st: tauri::State<'_, St>, id: u32, login: String, follow: bool) -> Result<bool, String> {
    viewer::press_follow(&app, id, follow)?;
    let (mut token, me, u) = {
        let mut got = None;
        for attempt in 0..2 {
            let (tok, me) = with_token(&st).await?;
            match resolve_user_api(&st, &tok, &login).await {
                Ok(u) => { got = Some((tok, me, u)); break; }
                Err(ApiError::Unauthorized) if attempt == 0 => {
                    if !refresh_tokens(&st).await {
                        return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        got.ok_or_else(|| i18n::tr("Kirjautuminen vanheni.", "Login expired.").to_string())?
    };
    // Odotetaan, että Twitch rekisteröi muutoksen
    let mut now = !follow;
    for _ in 0..12 {
        tokio::time::sleep(Duration::from_millis(700)).await;
        match twitch::is_following(&st.http, &token, &me, &u.id).await {
            Ok(f) => {
                now = f;
                if f == follow {
                    break;
                }
            }
            Err(ApiError::Unauthorized) => {
                if refresh_tokens(&st).await {
                    if let Ok((tok, _)) = with_token(&st).await {
                        token = tok;
                    }
                }
            }
            Err(_) => {}
        }
    }
    st.inner.lock().unwrap().last_follow_refresh = None;
    st.wake.notify_one();
    Ok(now)
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
}

/// Avaa valmiin klipin katsottavaksi järjestelmän selaimeen (vain Twitchin klippiosoitteet).
#[tauri::command]
fn open_clip(url: String) -> Result<(), String> {
    if !(url.starts_with("https://clips.twitch.tv/") || url.starts_with("https://www.twitch.tv/")) {
        return Err("bad url".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
async fn viewer_close_slot(app: AppHandle, id: u32) {
    viewer::close_slot(&app, id);
}

#[tauri::command]
async fn viewer_reorder(app: AppHandle, ids: Vec<u32>) {
    viewer::reorder(&app, ids);
}

/// Mod View -napin kanavat: oma kanava + kanavat, joilla käyttäjä on moderaattori.
/// { needScope: true } = vanha kirjautuminen ilman lupaa → UI pyytää kirjautumaan uudelleen.
#[tauri::command]
async fn mod_channels(st: tauri::State<'_, St>) -> Result<Value, String> {
    for attempt in 0..2 {
        let (token, user_id) = with_token(&st).await?;
        match twitch::moderated_channels(&st.http, &token, &user_id).await {
            Ok(None) => return Ok(json!({ "needScope": true, "logins": [] })),
            Ok(Some(l)) => return Ok(json!({ "needScope": false, "logins": l })),
            Err(ApiError::Unauthorized) if attempt == 0 => {
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into())
}

#[tauri::command]
async fn open_mod_view(app: AppHandle, login: String) -> Result<(), String> {
    if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("bad login".into());
    }
    viewer::open_mod_view(&app, &login.to_lowercase())
}

#[tauri::command]
async fn open_chat_settings(app: AppHandle) -> Result<(), String> {
    viewer::open_chat_settings(&app)
}

#[tauri::command]
async fn open_subscribe(app: AppHandle, login: String) -> Result<(), String> {
    if login.is_empty() || !login.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("bad login".into());
    }
    viewer::open_aux_window(&app, "subs", i18n::tr("Tilaa – Chilly", "Subscribe – Chilly"), &format!("https://www.twitch.tv/subs/{}", login.to_lowercase()), 560.0, 800.0)
}

/// Klippi: "NEED_SCOPE" = tarvitaan uusi kirjautuminen klippioikeudella.
#[tauri::command]
async fn create_clip(app: AppHandle, st: tauri::State<'_, St>, login: String, title: Option<String>, duration: Option<f64>) -> Result<(), String> {
    let (mut title, mut duration) = (title, duration);
    let mut refreshed = false;
    for _ in 0..3 {
        let (token, _) = with_token(&st).await?;
        let u = match resolve_user_api(&st, &token, &login).await {
            Ok(u) => u,
            Err(ApiError::Unauthorized) if !refreshed => {
                refreshed = true;
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        match twitch::create_clip(&st.http, &token, &u.id, title.as_deref(), duration).await {
            Ok((id, edit_url)) => {
                // Klippi on valmis. Ei ikkunaa: pieni ilmoitus, jossa Kopioi linkki ja Avaa.
                {
                    let mut i = st.inner.lock().unwrap();
                    let secs = tool_popup_secs(i.settings.popup_secs);
                    i.popups.push(notify::PopupItem {
                        name: u.display_name.clone(),
                        login: u.login.clone(),
                        image: u.profile_image_url.clone(),
                        secs,
                        kind: "clip".into(),
                        clip_url: format!("https://clips.twitch.tv/{id}"),
                        edit_url,
                        ..Default::default()
                    });
                }
                notify::poke(&app);
                return Ok(());
            }
            Err(twitch::ClipError::NeedScope) => return Err("NEED_SCOPE".into()),
            Err(twitch::ClipError::NotLive) => return Err(i18n::tr("Klipin voi tehdä vain, kun kanava on livenä.", "You can only clip while the channel is live.").into()),
            Err(twitch::ClipError::Api(ApiError::Unauthorized)) if !refreshed => {
                refreshed = true;
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
            }
            // Jos Twitch ei hyväksy otsikkoa tai pituutta, tehdään tavallinen klippi ilman niitä
            Err(twitch::ClipError::Api(ApiError::Other(_))) if title.is_some() || duration.is_some() => {
                title = None;
                duration = None;
            }
            Err(twitch::ClipError::Api(e)) => return Err(e.to_string()),
        }
    }
    Err(i18n::tr("Klippi epäonnistui.", "Clip failed.").into())
}

/// Ajaa Twitch-kutsun ja uusii tokenin kerran, jos se on vanhentunut.
macro_rules! with_retry {
    ($st:expr, |$token:ident| $body:expr) => {{
        let mut result = Err(i18n::tr("Kirjaudu ensin.", "Log in first.").to_string());
        for attempt in 0..2 {
            let $token = $st.inner.lock().unwrap().settings.auth.as_ref().map(|a| a.access_token.clone())
                .ok_or(i18n::tr("Kirjaudu ensin.", "Log in first."))?;
            match $body.await {
                Err(ApiError::Unauthorized) if attempt == 0 => {
                    if !refresh_tokens(&$st).await {
                        return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                    }
                }
                r => { result = r.map_err(|e| e.to_string()); break; }
            }
        }
        result
    }};
}

#[tauri::command]
async fn discover_top_games(st: tauri::State<'_, St>) -> Result<Vec<twitch::Category>, String> {
    with_retry!(st, |token| twitch::top_games(&st.http, &token))
}

#[tauri::command]
async fn discover_categories(st: tauri::State<'_, St>, query: String) -> Result<Vec<twitch::Category>, String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(vec![]);
    }
    with_retry!(st, |token| twitch::search_categories(&st.http, &token, &q))
}

#[tauri::command]
async fn discover_streams(
    st: tauri::State<'_, St>,
    game_id: Option<String>,
    language: Option<String>,
    fewest: bool,
    cursor: Option<String>,
) -> Result<Value, String> {
    let (list, next) = with_retry!(st, |token| twitch::discover_streams(
        &st.http, &token, game_id.as_deref(), language.as_deref(), fewest, cursor.clone()
    ))?;
    Ok(json!({ "items": list, "cursor": next }))
}

#[tauri::command]
async fn discover_images(st: tauri::State<'_, St>, ids: Vec<String>) -> Result<std::collections::HashMap<String, String>, String> {
    let ids: Vec<String> = ids.into_iter().take(100).collect();
    let list = with_retry!(st, |token| twitch::profile_images(&st.http, &token, &ids))?;
    Ok(list.into_iter().collect())
}

#[tauri::command]
async fn discover_channels(
    st: tauri::State<'_, St>,
    query: String,
    game_id: Option<String>,
    language: Option<String>,
) -> Result<Vec<twitch::Found>, String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(vec![]);
    }
    with_retry!(st, |token| twitch::discover_channels(&st.http, &token, &q, game_id.as_deref(), language.as_deref()))
}

#[tauri::command]
async fn twitch_web_status(app: AppHandle) -> bool {
    viewer::web_logged_in(&app).await
}

#[tauri::command]
async fn search_channels(st: tauri::State<'_, St>, query: String) -> Result<Vec<twitch::SearchHit>, String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(vec![]);
    }
    for attempt in 0..2 {
        let token = st.inner.lock().unwrap().settings.auth.as_ref().map(|a| a.access_token.clone()).ok_or(i18n::tr("Kirjaudu ensin.", "Log in first."))?;
        match twitch::search_channels(&st.http, &token, &q).await {
            Err(ApiError::Unauthorized) if attempt == 0 => {
                if !refresh_tokens(&st).await {
                    return Err(i18n::tr("Kirjautuminen vanheni.", "Login expired.").into());
                }
            }
            r => return r.map_err(|e| e.to_string()),
        }
    }
    Ok(vec![])
}

#[tauri::command]
async fn viewer_reload(app: AppHandle) {
    viewer::reload_all(&app);
}

#[tauri::command]
fn ui_lang() -> &'static str {
    i18n::code()
}

/// Aloitusanimaatio: true vain kerran ja vain jos Chilly avattiin näkyvänä.
#[tauri::command]
fn take_splash(st: tauri::State<'_, St>) -> bool {
    std::mem::take(&mut st.inner.lock().unwrap().splash_pending)
}

#[tauri::command]
async fn open_login_page(app: AppHandle, st: tauri::State<'_, St>) -> Result<(), String> {
    let uri = st.inner.lock().unwrap().login_pending.clone().map(|(_, u)| u);
    if let Some(uri) = uri {
        viewer::open_twitch_login(&app, &uri)?;
    }
    Ok(())
}

#[tauri::command]
fn refresh_now(st: tauri::State<'_, St>) {
    st.inner.lock().unwrap().last_follow_refresh = None;
    st.wake.notify_one();
}

// ---------- Käynnistys ----------

/// `--open=<kanava>` avaa kanavan suoraan (myös jo käynnissä olevaan Chillyyn).
fn open_arg(args: &[String]) -> Option<String> {
    args.iter()
        .find_map(|a| a.strip_prefix("--open="))
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

fn main() {
    let hidden = std::env::args().any(|a| a == "--hidden");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            #[cfg(debug_assertions)]
            if args.iter().any(|a| a == "--dev-toggle-mute") {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { viewer::dev_toggle_active_mute(&app) });
                return;
            }
            #[cfg(debug_assertions)]
            if let Some(js) = args.iter().find_map(|a| a.strip_prefix("--dev-eval=")) {
                if let Some(ui) = app.get_webview("main-ui") {
                    let _ = ui.eval(js);
                }
                return;
            }
            #[cfg(debug_assertions)]
            if let Some(spec) = args.iter().find_map(|a| a.strip_prefix("--dev-click=")) {
                // label|x|y → aito hiiren klikkaus (DevTools Input.dispatchMouseEvent)
                let p: Vec<&str> = spec.split('|').collect();
                if p.len() == 3 {
                    viewer::dev_click(app, p[0], p[1].parse().unwrap_or(0.0), p[2].parse().unwrap_or(0.0));
                }
                return;
            }
            #[cfg(debug_assertions)]
            if let Some(spec) = args.iter().find_map(|a| a.strip_prefix("--dev-probe=")) {
                if let Some((label, js)) = spec.split_once('|') {
                    viewer::dev_probe(app, label, js.to_string());
                }
                return;
            }
            #[cfg(debug_assertions)]
            if let Some(login) = args.iter().find_map(|a| a.strip_prefix("--dev-open-dup=")) {
                // Sama komento voi tulla kahdesti peräkkäin → ohitetaan toistot 3 s sisällä
                static LAST: std::sync::Mutex<Option<(String, Instant)>> = std::sync::Mutex::new(None);
                {
                    let mut l = LAST.lock().unwrap();
                    let dupe = l.as_ref().map_or(false, |(s, t)| s.as_str() == login && t.elapsed() < Duration::from_secs(3));
                    *l = Some((login.to_string(), Instant::now()));
                    if dupe {
                        return;
                    }
                }
                let (app, login) = (app.clone(), login.to_string());
                tauri::async_runtime::spawn(async move { let _ = viewer::dev_open_dup(&app, &login); });
                return;
            }
            #[cfg(debug_assertions)]
            if args.iter().any(|a| a == "--dev-toggle-pop") {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { viewer::dev_toggle_active_pop(&app) });
                return;
            }
            if let Some(login) = open_arg(&args) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = open_stream(&app, &login, false);
                });
            } else {
                show_main(app);
            }
        }))
        .plugin(tauri_plugin_autostart::Builder::new().args(["--hidden"]).build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            log::init(data_dir.clone());
            let mut settings = App::load(&data_dir);
            sync_streak_order(&mut settings);
            let (lang, changed_lang) = i18n::detect(&settings.lang);
            settings.lang = lang;
            i18n::set(&settings.lang);
            log::info(format!(
                "Chilly {} started (lang={}, hw_accel={}, adblock={}, hidden={hidden}, msix={})",
                app.package_info().version, settings.lang, settings.hw_accel, settings.adblock, is_msix()
            ));
            viewer::init_browser_args(&settings.lang, settings.hw_accel);
            viewer::set_adblock(settings.adblock);
            viewer::set_unlimited_tabs(settings.unlimited_tabs);
            viewer::store_boost(settings.volume_boost);
            let saved_bounds = settings.window.clone();
            let st: St = Arc::new(App {
                inner: Mutex::new(Inner { settings, splash_pending: !hidden, ..Default::default() }),
                wake: tokio::sync::Notify::new(),
                http: twitch::client(),
                data_dir,
            });
            app.manage(st.clone());
            app.manage(update::UpdSt::default());
            update::start_background(app.handle().clone());
            if changed_lang {
                st.save();
            }
            app.manage(viewer::ViewerSt::default());
            {
                let i = st.inner.lock().unwrap();
                viewer::set_emotes(app.handle(), i.settings.emote_bttv, i.settings.emote_7tv, false);
            }

            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Chilly")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, ev| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                        show_main(tray.app_handle());
                    }
                })
                .on_menu_event(|app, ev| {
                    let id = ev.id().as_ref();
                    match id {
                        "open" => show_main(app),
                        "quit" => {
                            app.state::<St>().save();
                            app.exit(0);
                        }
                        "refresh" => app.state::<St>().wake.notify_one(),
                        _ => {
                            if let Some(login) = id.strip_prefix("live:") {
                                let (app, login) = (app.clone(), login.to_string());
                                tauri::async_runtime::spawn(async move {
                                    let _ = open_stream(&app, &login, false);
                                });
                            }
                        }
                    }
                })
                .build(app)?;
            viewer::create_main(app.handle(), !hidden, saved_bounds.as_ref())?;
            update_tray(app.handle())?;
            notify::create_popup(app.handle())?;

            // Automaattiset lunastukset (bonusarkut auki olevilta kanavilta, valmiit dropsit)
            claim::start(
                app.handle().clone(),
                |app| {
                    let st = app.state::<St>();
                    let i = st.inner.lock().unwrap();
                    claim::Settings { bonus: i.settings.auto_bonus, drops: i.settings.auto_drops && i.settings.auth.is_some() }
                },
                |app| viewer::tab_list(app),
            );
            // Lunastettu drop → ilmoitus nurkkaan
            {
                let h = app.handle().clone();
                app.listen("auto-claimed", move |ev| {
                    let v: Value = serde_json::from_str(ev.payload()).unwrap_or(Value::Null);
                    if v["kind"] == "drop" {
                        let st = h.state::<St>();
                        {
                            let mut i = st.inner.lock().unwrap();
                            let secs = tool_popup_secs(i.settings.popup_secs);
                            i.popups.push(notify::PopupItem {
                                name: v["name"].as_str().unwrap_or("Drop").to_string(),
                                secs,
                                kind: "drop".into(),
                                ..Default::default()
                            });
                        }
                        notify::poke(&h);
                    }
                });
            }

            if let Some(login) = open_arg(&std::env::args().collect::<Vec<_>>()) {
                let app = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    let _ = open_stream(&app, &login, false);
                });
            }
            if std::env::args().any(|a| a == "--test-popup") {
                let snd = st.inner.lock().unwrap().settings.sound.clone();
                notify_live(&st, app.handle(), test_item(), &snd);
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    run_cycle(&st, &handle).await;
                    let secs = st.inner.lock().unwrap().settings.interval_secs.max(30);
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(secs)) => {}
                        _ = st.wake.notified() => {}
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|w, e| match (w.label(), e) {
            // Ikkunan koon ja sijainnin muistaminen
            ("main", WindowEvent::Moved(_) | WindowEvent::Resized(_)) => {
                let st = w.app_handle().state::<St>();
                let should_save = {
                    let mut i = st.inner.lock().unwrap();
                    if let Some(b) = viewer::capture_main_bounds(w, i.settings.window.as_ref()) {
                        i.settings.window = Some(b);
                        let due = i.last_win_save.map_or(true, |t| t.elapsed() >= Duration::from_secs(3));
                        if due {
                            i.last_win_save = Some(Instant::now());
                        }
                        due
                    } else {
                        false
                    }
                };
                if should_save {
                    st.save();
                }
            }
            // Sulkeminen vie Chilly:n trayhin ja lopettaa omat streamit
            ("main", WindowEvent::CloseRequested { api, .. }) => {
                api.prevent_close();
                let st = w.app_handle().state::<St>();
                {
                    let mut i = st.inner.lock().unwrap();
                    if let Some(b) = viewer::capture_main_bounds(w, i.settings.window.as_ref()) {
                        i.settings.window = Some(b);
                    }
                }
                st.save();
                let _ = w.hide();
                viewer::close_all_but_auto(w.app_handle());
            }
            ("twlogin", WindowEvent::Destroyed) => {
                viewer::reload_all(w.app_handle());
                let _ = w.app_handle().emit("web-login-changed", ());
            }
            (l, WindowEvent::CloseRequested { api, .. }) if viewer::pop_id(l).is_some() => {
                // Irrotetun ikkunan sulkeminen palauttaa streamin välilehdelleen
                api.prevent_close();
                viewer::pop_in(w.app_handle(), viewer::pop_id(l).unwrap());
            }
            (l, WindowEvent::Resized(_)) if viewer::pop_id(l).is_some() => {
                viewer::fit_pop(w.app_handle(), viewer::pop_id(l).unwrap());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            start_login,
            cancel_login,
            logout,
            set_channel,
            set_streak_order,
            set_settings,
            set_autostart,
            add_channel,
            remove_channel,
            test_notification,
            pick_sound_file,
            open_channel,
            open_login_page,
            popup_take,
            check_update,
            open_website,
            open_log_dir,
            open_startup_settings,
            install_update,
            popup_layout,
            refresh_now,
            take_splash,
            ui_lang,
            viewer_state,
            viewer_activate,
            viewer_close,
            viewer_mute,
            viewer_layout,
            viewer_twitch_login,
            viewer_reload,
            viewer_popout,
            viewer_popin,
            viewer_cover,
            twitch_web_status,
            search_channels,
            channel_info,
            viewer_offline,
            viewer_play,
            viewer_reload_tab,
            follow_channel,
            open_subscribe,
            open_chat_settings,
            create_clip,
            copy_text,
            open_clip,
            viewer_close_slot,
            viewer_reorder,
            open_mod_view,
            mod_channels,
            viewer_add_partner,
            viewer_remove_partner,
            viewer_multi,
            viewer_self_mute,
            restart_app,
            viewer_swap,
            discover_top_games,
            discover_categories,
            discover_streams,
            discover_channels,
            discover_images
        ])
        .run(tauri::generate_context!())
        .expect("Chilly ei käynnistynyt");
}
