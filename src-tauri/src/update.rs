//! Päivitysten tarkistus ja asennus (tauri-plugin-updater).
//!
//! Chilly lukee osoitteesta https://samppalandia.wtf/chilly/latest.json uusimman version tiedot.
//! Jos versio on uudempi, näytetään ilmoitus. Asennusohjelma ladataan samalta sivulta ja sen
//! allekirjoitus tarkistetaan Chillyyn rakennetulla julkisella avaimella ennen asennusta.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Viimeksi löydetty päivitys (asennetaan Päivitä-napista ilman uutta hakua)
#[derive(Default)]
pub struct UpdSt {
    pending: Mutex<Option<Update>>,
    notified: Mutex<Option<String>>,
}

#[derive(Serialize, Clone)]
pub struct UpdInfo {
    pub available: bool,
    pub current: String,
    pub version: String,
    pub notes: String,
}

fn updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    #[allow(unused_mut)]
    let mut b = app.updater_builder();
    // Kehitysversiossa päivityslähteen voi vaihtaa testiä varten
    #[cfg(debug_assertions)]
    if let Ok(u) = std::env::var("CHILLY_UPDATE_URL") {
        if let Ok(url) = u.parse() {
            b = b.endpoints(vec![url]).map_err(|e| e.to_string())?;
        }
    }
    b.build().map_err(|e| e.to_string())
}

/// Hakee uusimman version tiedot.
pub async fn check(app: &AppHandle) -> Result<UpdInfo, String> {
    let current = app.package_info().version.to_string();
    let upd = updater(app)?.check().await.map_err(|e| {
        crate::log::warn(format!("update check failed: {e}"));
        e.to_string()
    })?;
    let info = match &upd {
        Some(u) => {
            crate::log::info(format!("update available: {} (current {current})", u.version));
            UpdInfo { available: true, current, version: u.version.clone(), notes: u.body.clone().unwrap_or_default() }
        }
        None => UpdInfo { available: false, current: current.clone(), version: current, notes: String::new() },
    };
    *app.state::<UpdSt>().pending.lock().unwrap() = upd;
    Ok(info)
}

/// Lataa, tarkistaa allekirjoituksen ja asentaa päivityksen. Windowsissa asennusohjelma
/// käynnistyy ja Chilly sulkeutuu; asennuksen jälkeen Chilly käynnistyy uudelleen.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let pending = app.state::<UpdSt>().pending.lock().unwrap().take();
    let upd = match pending {
        Some(u) => u,
        None => updater(app)?
            .check()
            .await
            .map_err(|e| e.to_string())?
            .ok_or(crate::i18n::tr("Uutta versiota ei löytynyt.", "No new version found."))?,
    };
    // Kehitysversion kuivaharjoitus: ladataan ja tarkistetaan allekirjoitus, mutta ei asenneta
    #[cfg(debug_assertions)]
    if std::env::var("CHILLY_UPDATE_DRYRUN").is_ok() {
        let bytes = upd.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        let _ = std::fs::write(std::env::temp_dir().join("chilly-update-dryrun.txt"), format!("OK {} bytes, signature verified", bytes.len()));
        return Ok(());
    }
    // Ladataan ja tarkistetaan allekirjoitus ensin (striimit pysyvät auki, jos lataus epäonnistuu)
    let bytes = upd.download(|_, _| {}, || {}).await.map_err(|e| {
        crate::log::err(format!("update download failed: {e}"));
        e.to_string()
    })?;
    // Suljetaan kaikki striimit ja chatit puhtaasti ennen asennusohjelmaa, ettei yksikään
    // WebView2-prosessi jää taustalle soittamaan ääniä
    crate::prepare_for_update(app);
    tokio::time::sleep(Duration::from_millis(300)).await;
    upd.install(&bytes).map_err(|e| {
        crate::log::err(format!("update install failed: {e}"));
        e.to_string()
    })?;
    app.restart();
}

/// Taustatarkistus: käynnistyksen jälkeen ja kerran päivässä. Ilmoitus samasta versiosta
/// näytetään kerran per tarkistuskierros (eli "Myöhemmin" siirtää muistutusta päivällä).
pub fn start_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if let Ok(info) = check(&app).await {
                if info.available {
                    let st = app.state::<UpdSt>();
                    *st.notified.lock().unwrap() = Some(info.version.clone());
                    crate::notify_update(&app, &info.version, &info.notes);
                }
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });
}
