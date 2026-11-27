//! Automaattiset lunastukset: kanavapisteiden bonusarkut ja valmiit dropsit.
//! Toimii vain Chillyssä auki olevien kanavien kanssa (bonusarkku) ja käyttäjän omassa
//! dropsivarastossa – ei taustakeräystä kanaville, joita ei katsota.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl};

/// Painaa "Lunasta bonus" -nappia, jos se on näkyvissä (kielestä riippumaton luokkanimi).
const BONUS_JS: &str = r#"(() => {
  const i = document.querySelector('.claimable-bonus__icon');
  const b = i && i.closest('button');
  if (!b || b.disabled) return 'none';
  b.click();
  return 'claimed';
})()"#;

/// Lunastaa dropsivarastosta valmiit dropit. Palauttaa lunastettujen nimet.
/// HUOM: vain keskeneräisten droppien "Lunasta nyt" / "Claim Now" -nappi. Varaston koodipalkinnoissa
/// on pelkkä "Lunasta"-nappi (avaa koodin) – niihin ei saa koskea.
const DROPS_JS: &str = r#"(() => {
  const out = [];
  const byTarget = [...document.querySelectorAll('[data-test-selector$="claim-button"]')];
  const byText = [...document.querySelectorAll('button')].filter((b) => /^(claim now|lunasta nyt)$/i.test((b.innerText || '').trim()));
  const seen = new Set();
  for (const b of [...byTarget, ...byText]) {
    if (seen.has(b) || b.disabled || b.closest('[role="dialog"]')) continue;
    seen.add(b);
    let name = '';
    for (let el = b.parentElement, i = 0; el && i < 6 && !name; el = el.parentElement, i++) {
      const p = el.querySelector('p');
      const t = p && p.innerText.trim();
      if (t && !/^(claim now|lunasta nyt)$/i.test(t)) name = t;
    }
    b.click();
    out.push(name || 'Drop');
  }
  return JSON.stringify(out);
})()"#;

const DROPS_URL: &str = "https://www.twitch.tv/drops/inventory";
const DROPS_LABEL: &str = "drops";

/// Ajaa JS:n webviewissä ja palauttaa tuloksen (JSON-merkkijono WebView2:lta).
pub async fn exec(app: &AppHandle, label: &str, js: &str) -> Option<String> {
    let wv = app.get_webview(label)?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Arc::new(std::sync::Mutex::new(Some(tx)));
    let js = js.to_string();
    let ok = wv
        .with_webview(move |pw| unsafe {
            use webview2_com::ExecuteScriptCompletedHandler;
            if let Ok(core) = pw.controller().CoreWebView2() {
                let js = windows_core::HSTRING::from(js);
                let tx2 = tx.clone();
                let _ = core.ExecuteScript(
                    &js,
                    &ExecuteScriptCompletedHandler::create(Box::new(move |_e, res| {
                        if let Some(t) = tx2.lock().unwrap().take() {
                            let _ = t.send(res);
                        }
                        Ok(())
                    })),
                );
            }
        })
        .is_ok();
    if !ok {
        return None;
    }
    tokio::time::timeout(Duration::from_secs(5), rx).await.ok()?.ok()
}

/// WebView2 palauttaa merkkijonon JSON-koodattuna ("\"claimed\"").
fn unquote(s: &str) -> String {
    serde_json::from_str::<String>(s).unwrap_or_default()
}

pub struct Settings {
    pub bonus: bool,
    pub drops: bool,
}

/// Taustasilmukka (10 s välein). `settings` lukee asetukset, `tabs` palauttaa (id, login) -parit.
pub fn start<FS, FT>(app: AppHandle, settings: FS, tabs: FT)
where
    FS: Fn(&AppHandle) -> Settings + Send + 'static,
    FT: Fn(&AppHandle) -> Vec<(u32, String)> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        let mut last_bonus: HashMap<String, Instant> = HashMap::new();
        let mut next_drops = Instant::now() + Duration::from_secs(60);
        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;
            let s = settings(&app);

            if s.bonus {
                for (id, login) in tabs(&app) {
                    // Sama kanava voi näkyä sekä chatissa että kanavasivulla → lunastus vain kerran minuutissa
                    if last_bonus.get(&login).map_or(false, |t| t.elapsed() < Duration::from_secs(60)) {
                        continue;
                    }
                    // Kanavasivun oma chat-palsta suljetaan (strip.js), joten pisteet ovat chat-ikkunassa
                    for label in [format!("c{id}")] {
                        if let Some(r) = exec(&app, &label, BONUS_JS).await {
                            if unquote(&r) == "claimed" {
                                last_bonus.insert(login.clone(), Instant::now());
                                crate::log::info(format!("bonus claimed: {login}"));
                                let _ = app.emit("auto-claimed", serde_json::json!({ "kind": "bonus", "login": login }));
                                break;
                            }
                        }
                    }
                }
            }

            if s.drops && Instant::now() >= next_drops {
                next_drops = Instant::now() + Duration::from_secs(15 * 60);
                check_drops(&app).await;
            } else if !s.drops {
                close_drops(&app);
            }
        }
    });
}

/// Tarkistaa onko dropsivaraston sisältö renderöitynyt.
const DROPS_READY_JS: &str = r#"(() => {
  if (document.readyState !== 'complete') return 'wait';
  const main = document.querySelector('main');
  return main && (main.innerText || '').trim().length > 40 ? 'ready' : 'wait';
})()"#;

/// Avaa näkymättömän dropsivaraston, lunastaa valmiit dropit ja sulkee näkymän heti
/// (säästää ~150–250 Mt muistia tarkistusten välillä).
async fn check_drops(app: &AppHandle) {
    if !crate::viewer::web_logged_in(app).await {
        return;
    }
    close_drops(app);
    let url = Url::parse(DROPS_URL).unwrap();
    let Some(win) = app.get_window("main") else { return };
    let b = tauri::webview::WebviewBuilder::new(DROPS_LABEL, WebviewUrl::External(url))
        .additional_browser_args(crate::viewer::browser_args())
        .on_navigation(|u| u.host_str().map_or(false, |h| h.ends_with("twitch.tv")));
    match win.add_child(b, LogicalPosition::new(0.0, 0.0), LogicalSize::new(1.0, 1.0)) {
        Ok(wv) => {
            let _ = wv.hide();
            crate::viewer::set_muted(&wv, true);
        }
        Err(e) => {
            crate::log::warn(format!("drops webview create failed: {e}"));
            return;
        }
    }
    // Odotetaan React-sivun latautumista (6–18 s), ettei hitaampi yhteys jää tyhjin käsin
    tokio::time::sleep(Duration::from_secs(6)).await;
    for _ in 0..3 {
        if let Some(st) = exec(app, DROPS_LABEL, DROPS_READY_JS).await {
            if unquote(&st) == "ready" {
                break;
            }
        }
        tokio::time::sleep(Duration::from_secs(4)).await;
    }
    let names: Vec<String> = match exec(app, DROPS_LABEL, DROPS_JS).await {
        Some(r) => serde_json::from_str(&unquote(&r)).unwrap_or_default(),
        None => Vec::new(),
    };
    if !names.is_empty() {
        // Annetaan Twitchin lunastuspyynnön valmistua ennen näkymän sulkemista
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    close_drops(app);
    static NOTIFIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    for n in names {
        crate::log::info(format!("drop claimed: {n}"));
        let mut done = NOTIFIED.lock().unwrap();
        if done.contains(&n) {
            continue;
        }
        done.push(n.clone());
        drop(done);
        let _ = app.emit("auto-claimed", serde_json::json!({ "kind": "drop", "name": n }));
    }
}

fn close_drops(app: &AppHandle) {
    if let Some(wv) = app.get_webview(DROPS_LABEL) {
        let _ = wv.close();
    }
}
