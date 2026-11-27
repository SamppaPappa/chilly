//! Kieli (fi / en) Rust-puolen teksteille: tray-valikko, ikkunoiden otsikot, virheilmoitukset.
//! Käyttöliittymän tekstit ovat ui/i18n.js:ssä.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set(lang: &str) {
    EN.store(lang == "en", Ordering::Relaxed);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

pub fn code() -> &'static str {
    if en() { "en" } else { "fi" }
}

/// Valitsee tekstin nykyisellä kielellä.
pub fn tr(fi: &'static str, en_text: &'static str) -> &'static str {
    if en() { en_text } else { fi }
}

/// Kieli ensimmäisellä käynnistyksellä: asennusohjelman valinta (rekisterissä, luetaan kerran
/// ja poistetaan), muuten Windowsin käyttöliittymän kieli (suomi → fi, muut → en).
pub fn detect(saved: &str) -> (String, bool) {
    // Asennusohjelman valinta voittaa aina (myös uudelleenasennuksessa)
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags("Software\\Chilly", winreg::enums::KEY_READ | winreg::enums::KEY_WRITE) {
        if let Ok(v) = key.get_value::<String, _>("Language") {
            let _ = key.delete_value("Language");
            if v == "fi" || v == "en" {
                return (v, true);
            }
        }
    }
    if saved == "fi" || saved == "en" {
        return (saved.to_string(), false);
    }
    // Windowsin kieli: ensisijainen kielitunnus 0x0B = suomi
    let lang_id = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    let primary = lang_id & 0x3ff;
    (if primary == 0x0b { "fi" } else { "en" }.to_string(), true)
}
