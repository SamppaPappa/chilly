//! Twitch-rajapinta: Device Code -kirjautuminen (julkinen client, ei salaisuutta) ja Helix-kutsut.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Twitch-kehittäjäkonsolissa rekisteröidyn julkisen sovelluksen Client ID.
/// Ei ole salaisuus – sama ID kaikille käyttäjille.
pub const CLIENT_ID: &str = "nocx9qjmfe6xei7h8vfzcqjbhto7re";
/// user:read:moderated_channels: Mod View -nappi näytetään vain kanavilla, joilla käyttäjä on moderaattori.
const SCOPES: &str = "user:read:follows clips:edit user:read:moderated_channels";
const HELIX: &str = "https://api.twitch.tv/helix";

#[derive(Debug)]
pub enum ApiError {
    Unauthorized,
    Other(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Unauthorized => write!(f, "{}", crate::i18n::tr("Kirjautuminen vanhentunut", "Login expired")),
            ApiError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Other(format!("{}: {e}", crate::i18n::tr("verkkovirhe", "network error")))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub enum DevicePoll {
    Pending,
    SlowDown,
    Done(Tokens),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub profile_image_url: String,
    pub offline_image_url: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct Followed {
    pub id: String,
    pub login: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stream {
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub game_name: String,
    pub title: String,
    pub viewer_count: u64,
    pub started_at: String,
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Chilly/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .expect("http client")
}

pub fn client_id_set() -> bool {
    !CLIENT_ID.starts_with("PUT_")
}

pub async fn device_start(http: &reqwest::Client) -> ApiResult<DeviceStart> {
    let r = http
        .post("https://id.twitch.tv/oauth2/device")
        .form(&[("client_id", CLIENT_ID), ("scopes", SCOPES)])
        .send()
        .await?;
    if !r.status().is_success() {
        let body = r.text().await.unwrap_or_default();
        return Err(ApiError::Other(format!("{}: {body}", crate::i18n::tr("Kirjautumisen aloitus epäonnistui", "Could not start login"))));
    }
    r.json().await.map_err(Into::into)
}

pub async fn device_poll(http: &reqwest::Client, device_code: &str) -> DevicePoll {
    let r = match http
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("scopes", SCOPES),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return DevicePoll::Pending, // verkkokatko – yritetään uudelleen
    };
    let ok = r.status().is_success();
    let v: Value = r.json().await.unwrap_or(Value::Null);
    if ok {
        if let (Some(a), Some(rf)) = (v["access_token"].as_str(), v["refresh_token"].as_str()) {
            return DevicePoll::Done(Tokens { access_token: a.into(), refresh_token: rf.into() });
        }
        return DevicePoll::Failed(crate::i18n::tr("Outo vastaus Twitchiltä", "Unexpected response from Twitch").into());
    }
    let msg = v["message"].as_str().unwrap_or("").to_string();
    match msg.as_str() {
        "authorization_pending" => DevicePoll::Pending,
        "slow_down" => DevicePoll::SlowDown,
        "access_denied" => DevicePoll::Failed(crate::i18n::tr("Kirjautuminen peruttiin", "Login was cancelled").into()),
        "expired_token" => DevicePoll::Failed(crate::i18n::tr("Koodi vanheni", "The code expired").into()),
        _ => DevicePoll::Failed(format!("{} ({msg})", crate::i18n::tr("Kirjautuminen epäonnistui", "Login failed"))),
    }
}

pub async fn refresh(http: &reqwest::Client, refresh_token: &str) -> ApiResult<Tokens> {
    let r = http
        .post("https://id.twitch.tv/oauth2/token")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?;
    if r.status().as_u16() == 400 || r.status().as_u16() == 401 {
        return Err(ApiError::Unauthorized);
    }
    if !r.status().is_success() {
        return Err(ApiError::Other(format!("{}: HTTP {}", crate::i18n::tr("tokenin uusinta", "token refresh"), r.status())));
    }
    let v: Value = r.json().await?;
    match (v["access_token"].as_str(), v["refresh_token"].as_str()) {
        (Some(a), Some(rf)) => Ok(Tokens { access_token: a.into(), refresh_token: rf.into() }),
        _ => Err(ApiError::Unauthorized),
    }
}

async fn helix_get(http: &reqwest::Client, token: &str, path: &str, query: &[(&str, String)]) -> ApiResult<Value> {
    let r = http
        .get(format!("{HELIX}{path}"))
        .bearer_auth(token)
        .header("Client-Id", CLIENT_ID)
        .query(query)
        .send()
        .await?;
    if r.status().as_u16() == 401 {
        return Err(ApiError::Unauthorized);
    }
    if !r.status().is_success() {
        let s = r.status();
        let body = r.text().await.unwrap_or_default();
        return Err(ApiError::Other(format!("Twitch {path}: HTTP {s} {body}")));
    }
    r.json().await.map_err(Into::into)
}

/// Kanavat, joilla käyttäjä on moderaattori (login-nimet). Ok(None) = kirjautumisesta puuttuu
/// lupa user:read:moderated_channels (vanha kirjautuminen) → käyttäjän pitää kirjautua uudelleen.
pub async fn moderated_channels(http: &reqwest::Client, token: &str, user_id: &str) -> ApiResult<Option<Vec<String>>> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..20 {
        let mut q = vec![("user_id", user_id.to_string()), ("first", "100".to_string())];
        if let Some(a) = &after {
            q.push(("after", a.clone()));
        }
        let r = http.get(format!("{HELIX}/moderation/channels")).bearer_auth(token).header("Client-Id", CLIENT_ID).query(&q).send().await?;
        let status = r.status().as_u16();
        let v: Value = r.json().await.unwrap_or(Value::Null);
        if status == 401 {
            if v["message"].as_str().unwrap_or("").to_lowercase().contains("scope") {
                return Ok(None);
            }
            return Err(ApiError::Unauthorized);
        }
        if !(200..300).contains(&status) {
            return Err(ApiError::Other(format!("Twitch /moderation/channels: HTTP {status}")));
        }
        if let Some(a) = v["data"].as_array() {
            out.extend(a.iter().map(|c| s(c, "broadcaster_login").to_lowercase()));
        }
        after = v["pagination"]["cursor"].as_str().filter(|c| !c.is_empty()).map(String::from);
        if after.is_none() {
            break;
        }
    }
    Ok(Some(out))
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

fn parse_user(u: &Value) -> UserInfo {
    UserInfo {
        id: s(u, "id"),
        login: s(u, "login"),
        display_name: s(u, "display_name"),
        profile_image_url: s(u, "profile_image_url"),
        offline_image_url: s(u, "offline_image_url"),
        description: s(u, "description"),
    }
}

/// Kirjautunut käyttäjä (ilman parametreja /users palauttaa tokenin omistajan).
pub async fn me(http: &reqwest::Client, token: &str) -> ApiResult<UserInfo> {
    let v = helix_get(http, token, "/users", &[]).await?;
    v["data"]
        .as_array()
        .and_then(|a| a.first())
        .map(parse_user)
        .ok_or_else(|| ApiError::Other(crate::i18n::tr("Käyttäjätietoja ei saatu", "Could not get user info").into()))
}

pub async fn users_by_ids(http: &reqwest::Client, token: &str, ids: &[String]) -> ApiResult<Vec<UserInfo>> {
    let mut out = Vec::new();
    for chunk in ids.chunks(100) {
        let q: Vec<(&str, String)> = chunk.iter().map(|i| ("id", i.clone())).collect();
        let v = helix_get(http, token, "/users", &q).await?;
        if let Some(a) = v["data"].as_array() {
            out.extend(a.iter().map(parse_user));
        }
    }
    Ok(out)
}

pub async fn user_by_login(http: &reqwest::Client, token: &str, login: &str) -> ApiResult<Option<UserInfo>> {
    let v = helix_get(http, token, "/users", &[("login", login.to_string())]).await?;
    Ok(v["data"].as_array().and_then(|a| a.first()).map(parse_user))
}

pub async fn followed_channels(http: &reqwest::Client, token: &str, user_id: &str) -> ApiResult<Vec<Followed>> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut q = vec![("user_id", user_id.to_string()), ("first", "100".to_string())];
        if let Some(a) = &after {
            q.push(("after", a.clone()));
        }
        let v = helix_get(http, token, "/channels/followed", &q).await?;
        if let Some(a) = v["data"].as_array() {
            out.extend(a.iter().map(|c| Followed {
                id: s(c, "broadcaster_id"),
                login: s(c, "broadcaster_login"),
                name: s(c, "broadcaster_name"),
            }));
        }
        after = v["pagination"]["cursor"].as_str().map(String::from);
        if after.is_none() || out.len() > 5000 {
            break;
        }
    }
    Ok(out)
}

fn parse_streams(v: &Value, out: &mut Vec<Stream>) {
    if let Some(a) = v["data"].as_array() {
        out.extend(a.iter().map(|x| Stream {
            user_id: s(x, "user_id"),
            user_login: s(x, "user_login"),
            user_name: s(x, "user_name"),
            game_name: s(x, "game_name"),
            title: s(x, "title"),
            viewer_count: x["viewer_count"].as_u64().unwrap_or(0),
            started_at: s(x, "started_at"),
        }));
    }
}

pub async fn live_followed(http: &reqwest::Client, token: &str, user_id: &str) -> ApiResult<Vec<Stream>> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let mut q = vec![("user_id", user_id.to_string()), ("first", "100".to_string())];
        if let Some(a) = &after {
            q.push(("after", a.clone()));
        }
        let v = helix_get(http, token, "/streams/followed", &q).await?;
        parse_streams(&v, &mut out);
        after = v["pagination"]["cursor"].as_str().map(String::from);
        if after.is_none() || out.len() > 2000 {
            break;
        }
    }
    Ok(out)
}

pub async fn streams_by_ids(http: &reqwest::Client, token: &str, ids: &[String]) -> ApiResult<Vec<Stream>> {
    let mut out = Vec::new();
    for chunk in ids.chunks(100) {
        let mut q: Vec<(&str, String)> = chunk.iter().map(|i| ("user_id", i.clone())).collect();
        q.push(("first", "100".into()));
        let v = helix_get(http, token, "/streams", &q).await?;
        parse_streams(&v, &mut out);
    }
    Ok(out)
}

pub async fn streams_by_logins(http: &reqwest::Client, token: &str, logins: &[String]) -> ApiResult<Vec<Stream>> {
    let mut out = Vec::new();
    for chunk in logins.chunks(100) {
        let mut q: Vec<(&str, String)> = chunk.iter().map(|l| ("user_login", l.clone())).collect();
        q.push(("first", "100".into()));
        let v = helix_get(http, token, "/streams", &q).await?;
        parse_streams(&v, &mut out);
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub login: String,
    pub name: String,
    pub image: String,
    pub live: bool,
    pub game: String,
    pub viewers: Option<u64>,
}

/// Twitchin kanavahaku (ehdotukset kirjoittaessa). Livenä olevat ensin katsojamäärän mukaan.
pub async fn search_channels(http: &reqwest::Client, token: &str, query: &str) -> ApiResult<Vec<SearchHit>> {
    let v = helix_get(http, token, "/search/channels", &[("query", query.to_string()), ("first", "12".into())]).await?;
    let mut hits: Vec<SearchHit> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| SearchHit {
                    id: s(c, "id"),
                    login: s(c, "broadcaster_login"),
                    name: s(c, "display_name"),
                    image: s(c, "thumbnail_url"),
                    live: c["is_live"].as_bool().unwrap_or(false),
                    game: s(c, "game_name"),
                    viewers: None,
                })
                .collect()
        })
        .unwrap_or_default();
    let live_ids: Vec<String> = hits.iter().filter(|h| h.live).map(|h| h.id.clone()).collect();
    if !live_ids.is_empty() {
        if let Ok(streams) = streams_by_ids(http, token, &live_ids).await {
            for h in &mut hits {
                if let Some(st) = streams.iter().find(|st| st.user_id == h.id) {
                    h.viewers = Some(st.viewer_count);
                    if !st.game_name.is_empty() {
                        h.game = st.game_name.clone();
                    }
                }
            }
        }
    }
    hits.sort_by(|a, b| b.live.cmp(&a.live).then(b.viewers.unwrap_or(0).cmp(&a.viewers.unwrap_or(0))));
    Ok(hits)
}

#[derive(Debug, Clone, Serialize)]
pub struct Clip {
    pub id: String,
    pub url: String,
    pub title: String,
    pub thumbnail: String,
    pub views: u64,
    pub duration: f64,
    pub created_at: String,
}

/// Kanavan seuraajamäärä (total toimii millä tahansa käyttäjätokenilla).
pub async fn followers_total(http: &reqwest::Client, token: &str, broadcaster_id: &str) -> ApiResult<u64> {
    let v = helix_get(http, token, "/channels/followers", &[("broadcaster_id", broadcaster_id.to_string()), ("first", "1".into())]).await?;
    Ok(v["total"].as_u64().unwrap_or(0))
}

/// Seuraako käyttäjä kanavaa.
pub async fn is_following(http: &reqwest::Client, token: &str, user_id: &str, broadcaster_id: &str) -> ApiResult<bool> {
    let v = helix_get(
        http,
        token,
        "/channels/followed",
        &[("user_id", user_id.to_string()), ("broadcaster_id", broadcaster_id.to_string())],
    )
    .await?;
    Ok(v["data"].as_array().map_or(false, |a| !a.is_empty()))
}

/// Katsotuimmat klipit (kaikki ajat).
pub async fn top_clips(http: &reqwest::Client, token: &str, broadcaster_id: &str) -> ApiResult<Vec<Clip>> {
    let v = helix_get(http, token, "/clips", &[("broadcaster_id", broadcaster_id.to_string()), ("first", "12".into())]).await?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Clip {
                    id: s(c, "id"),
                    url: s(c, "url"),
                    title: s(c, "title"),
                    thumbnail: s(c, "thumbnail_url"),
                    views: c["view_count"].as_u64().unwrap_or(0),
                    duration: c["duration"].as_f64().unwrap_or(0.0),
                    created_at: s(c, "created_at"),
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Viimeisimmän lähetyksen (arkisto-VOD) alkuaika, eli suunnilleen "viimeksi livenä".
pub async fn last_broadcast(http: &reqwest::Client, token: &str, user_id: &str) -> ApiResult<Option<String>> {
    let v = helix_get(
        http,
        token,
        "/videos",
        &[("user_id", user_id.to_string()), ("type", "archive".into()), ("first", "1".into())],
    )
    .await?;
    Ok(v["data"].as_array().and_then(|a| a.first()).map(|x| s(x, "created_at")))
}

pub enum ClipError {
    NeedScope,
    NotLive,
    Api(ApiError),
}

/// Tekee klipin streamista. Palauttaa (klipin id, editorin osoite).
/// `title` ja `duration` (5–60 s) ovat valinnaisia: ilman niitä Twitch tekee ~30 s klipin
/// striimin otsikolla.
pub async fn create_clip(http: &reqwest::Client, token: &str, broadcaster_id: &str, title: Option<&str>, duration: Option<f64>) -> Result<(String, String), ClipError> {
    let mut q: Vec<(&str, String)> = vec![("broadcaster_id", broadcaster_id.to_string())];
    if let Some(t) = title.map(str::trim).filter(|t| !t.is_empty()) {
        q.push(("title", t.chars().take(140).collect()));
    }
    if let Some(d) = duration {
        q.push(("duration", format!("{:.1}", d.clamp(5.0, 60.0))));
    }
    let r = http
        .post(format!("{HELIX}/clips"))
        .bearer_auth(token)
        .header("Client-Id", CLIENT_ID)
        .query(&q)
        .send()
        .await
        .map_err(|e| ClipError::Api(e.into()))?;
    let status = r.status().as_u16();
    let body: Value = r.json().await.unwrap_or(Value::Null);
    let msg = body["message"].as_str().unwrap_or("").to_lowercase();
    if status == 401 && msg.contains("scope") {
        return Err(ClipError::NeedScope);
    }
    if status == 401 {
        return Err(ClipError::Api(ApiError::Unauthorized));
    }
    if status == 404 || msg.contains("offline") || msg.contains("not live") {
        return Err(ClipError::NotLive);
    }
    if !(200..300).contains(&status) {
        return Err(ClipError::Api(ApiError::Other(format!("{}: {}", crate::i18n::tr("Klippi epäonnistui", "Clip failed"), body["message"].as_str().unwrap_or("?")))));
    }
    let d = &body["data"][0];
    Ok((s(d, "id"), s(d, "edit_url")))
}

// ---------- Löydä (uusien striimaajien etsiminen) ----------

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub art: String,
}

fn parse_categories(v: &Value, art_key: &str) -> Vec<Category> {
    v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Category {
                    id: s(c, "id"),
                    name: s(c, "name"),
                    art: s(c, art_key).replace("{width}", "144").replace("{height}", "192").replace("52x72", "144x192"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Suosituimmat kategoriat juuri nyt.
pub async fn top_games(http: &reqwest::Client, token: &str) -> ApiResult<Vec<Category>> {
    let v = helix_get(http, token, "/games/top", &[("first", "20".into())]).await?;
    Ok(parse_categories(&v, "box_art_url"))
}

/// Peli- / kategoriahaku (ehdotukset kirjoittaessa).
pub async fn search_categories(http: &reqwest::Client, token: &str, query: &str) -> ApiResult<Vec<Category>> {
    let v = helix_get(http, token, "/search/categories", &[("query", query.to_string()), ("first", "10".into())]).await?;
    Ok(parse_categories(&v, "box_art_url"))
}

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub id: String,
    pub login: String,
    pub name: String,
    pub title: String,
    pub game: String,
    pub language: String,
    pub viewers: Option<u64>,
    pub thumbnail: String,
    pub image: String,
    pub live: bool,
}

/// Livestreamit suodattimin. Twitch palauttaa ne katsojamäärän mukaan suurimmasta pienimpään.
/// `fewest`: haetaan useampi sivu ja käännetään järjestys (vähiten katsojia ensin).
pub async fn discover_streams(
    http: &reqwest::Client,
    token: &str,
    game_id: Option<&str>,
    language: Option<&str>,
    fewest: bool,
    cursor: Option<String>,
) -> ApiResult<(Vec<Found>, Option<String>)> {
    let mut out: Vec<Found> = Vec::new();
    let mut after = cursor;
    // Eniten katsojia: yksi sivu kerrallaan ("Näytä lisää"). Vähiten: käydään läpi enintään 40 sivua
    // (4 000 streamia), koska Twitch ei osaa lajitella nousevasti. Suodattimilla joukko on yleensä pienempi.
    let pages = if fewest { 40 } else { 1 };
    for _ in 0..pages {
        let mut q: Vec<(&str, String)> = vec![("first", "100".into())];
        if let Some(g) = game_id { q.push(("game_id", g.to_string())); }
        if let Some(l) = language { q.push(("language", l.to_string())); }
        if let Some(a) = &after { q.push(("after", a.clone())); }
        let v = helix_get(http, token, "/streams", &q).await?;
        if let Some(a) = v["data"].as_array() {
            out.extend(a.iter().map(|x| Found {
                id: s(x, "user_id"),
                login: s(x, "user_login"),
                name: s(x, "user_name"),
                title: s(x, "title"),
                game: s(x, "game_name"),
                language: s(x, "language"),
                viewers: x["viewer_count"].as_u64(),
                thumbnail: s(x, "thumbnail_url").replace("{width}", "440").replace("{height}", "248"),
                image: String::new(),
                live: true,
            }));
        }
        after = v["pagination"]["cursor"].as_str().map(String::from).filter(|c| !c.is_empty());
        if after.is_none() {
            break;
        }
    }
    if fewest {
        // Koko joukko palautetaan; käyttöliittymä näyttää sen erissä ja hakee kuvat näytettäville
        out.sort_by_key(|f| f.viewers.unwrap_or(0));
        after = None;
    } else {
        fill_images(http, token, &mut out).await;
    }
    Ok((out, after))
}

/// Kanavahaku (myös offline-kanavat) nimellä, suodattimin.
pub async fn discover_channels(
    http: &reqwest::Client,
    token: &str,
    query: &str,
    game_id: Option<&str>,
    language: Option<&str>,
) -> ApiResult<Vec<Found>> {
    let v = helix_get(http, token, "/search/channels", &[("query", query.to_string()), ("first", "100".into())]).await?;
    let mut out: Vec<Found> = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|c| game_id.map_or(true, |g| c["game_id"].as_str() == Some(g)))
                .filter(|c| language.map_or(true, |l| c["broadcaster_language"].as_str() == Some(l)))
                .map(|c| Found {
                    id: s(c, "id"),
                    login: s(c, "broadcaster_login"),
                    name: s(c, "display_name"),
                    title: s(c, "title"),
                    game: s(c, "game_name"),
                    language: s(c, "broadcaster_language"),
                    viewers: None,
                    thumbnail: String::new(),
                    image: s(c, "thumbnail_url"),
                    live: c["is_live"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.live.cmp(&a.live));
    Ok(out)
}

async fn fill_images(http: &reqwest::Client, token: &str, list: &mut [Found]) {
    let ids: Vec<String> = list.iter().filter(|f| f.image.is_empty()).map(|f| f.id.clone()).collect();
    if ids.is_empty() {
        return;
    }
    if let Ok(users) = users_by_ids(http, token, &ids).await {
        for f in list.iter_mut() {
            if let Some(u) = users.iter().find(|u| u.id == f.id) {
                f.image = u.profile_image_url.clone();
            }
        }
    }
}

/// Kanavakuvat tunnisteiden mukaan (Löydän erissä näytettäville).
pub async fn profile_images(http: &reqwest::Client, token: &str, ids: &[String]) -> ApiResult<Vec<(String, String)>> {
    Ok(users_by_ids(http, token, ids).await?.into_iter().map(|u| (u.id, u.profile_image_url)).collect())
}
