//! The Steam Workshop for Wallpaper Engine, read without a Steam account: the public browse
//! pages (server-rendered, with the item list embedded as JSON), the public file-details API
//! and the preview image CDN.

use crate::error::{Error, Result};
use crate::we::project::ProjectType;
use crate::we::steam::APP_ID;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

const BROWSE_URL: &str = "https://steamcommunity.com/workshop/browse/";
const DETAILS_URL: &str =
    "https://api.steampowered.com/ISteamRemoteStorage/GetPublishedFileDetails/v1/";
const ITEM_URL: &str = "https://steamcommunity.com/sharedfiles/filedetails/?id=";
pub const PER_PAGE: u32 = 30;

/// Order of a browse listing; Steam's `browsesort` values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Trend,
    Recent,
    Updated,
    Subscribers,
    Rated,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Trend,
        Sort::Recent,
        Sort::Updated,
        Sort::Subscribers,
        Sort::Rated,
    ];

    pub fn param(self) -> &'static str {
        match self {
            Sort::Trend => "trend",
            Sort::Recent => "mostrecent",
            Sort::Updated => "lastupdated",
            Sort::Subscribers => "totaluniquesubscribers",
            Sort::Rated => "toprated",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::Trend => "Trending",
            Sort::Recent => "Newest",
            Sort::Updated => "Recently updated",
            Sort::Subscribers => "Most subscribed",
            Sort::Rated => "Top rated",
        }
    }

    pub fn parse(s: &str) -> Option<Sort> {
        let s = s.trim().to_ascii_lowercase();
        Sort::ALL.into_iter().find(|k| {
            k.param() == s
                || k.label().to_ascii_lowercase() == s
                || format!("{k:?}").to_ascii_lowercase() == s
        })
    }
}

/// Wallpaper Engine's age rating tags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rating {
    Everyone,
    Questionable,
    Mature,
}

impl Rating {
    pub fn tag(self) -> &'static str {
        match self {
            Rating::Everyone => "Everyone",
            Rating::Questionable => "Questionable",
            Rating::Mature => "Mature",
        }
    }

    fn from_tag(tag: &str) -> Option<Rating> {
        [Rating::Everyone, Rating::Questionable, Rating::Mature]
            .into_iter()
            .find(|r| r.tag().eq_ignore_ascii_case(tag))
    }
}

/// Trend periods Steam offers, in days.
pub const TREND_DAYS: [u32; 6] = [1, 7, 30, 90, 180, 365];

/// One browse request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Query {
    pub text: String,
    pub sort: Sort,
    /// Period the trend sort ranks over, in days.
    pub days: u32,
    /// Only items of this type.
    pub kind: Option<ProjectType>,
    /// Further tags every item must carry (genre, resolution, ...).
    pub tags: Vec<String>,
    /// Include items rated Questionable or Mature.
    pub mature: bool,
    /// 1-based page number.
    pub page: u32,
}

impl Default for Query {
    fn default() -> Query {
        Query {
            text: String::new(),
            sort: Sort::Trend,
            days: 7,
            kind: None,
            tags: Vec::new(),
            mature: false,
            page: 1,
        }
    }
}

/// A workshop item as the listing and the details API describe it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub id: u64,
    pub title: String,
    /// The uploader's Steam persona name.
    pub author: Option<String>,
    /// The uploader's 64-bit Steam id.
    pub creator: Option<u64>,
    /// Plain text; the listing carries a shortened version, the details the whole text.
    pub description: String,
    pub preview_url: Option<String>,
    pub tags: Vec<String>,
    /// Bytes of the download.
    pub size: u64,
    /// Seconds since the Unix epoch.
    pub created: u64,
    pub updated: u64,
    pub subscriptions: u64,
    pub favorites: u64,
    pub views: u64,
    /// Stars out of five, once enough people have voted.
    pub stars: Option<u8>,
    pub votes: u64,
}

impl Item {
    pub fn kind(&self) -> Option<ProjectType> {
        self.tags.iter().find_map(|t| ProjectType::parse(t))
    }

    pub fn rating(&self) -> Option<Rating> {
        self.tags.iter().find_map(|t| Rating::from_tag(t))
    }

    pub fn url(&self) -> String {
        crate::we::project::workshop_url(self.id)
    }

    /// The preview scaled by Steam's image service to fit a `size` pixel square.
    pub fn preview_sized(&self, size: u32) -> Option<String> {
        let base = self.preview_url.as_deref()?;
        let sep = if base.contains('?') { '&' } else { '?' };
        Some(format!(
            "{base}{sep}imw={size}&imh={size}&ima=fit&impolicy=Letterbox&imcolor=%23000000&letterbox=false"
        ))
    }
}

/// One page of a listing.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<Item>,
    pub page: u32,
    pub pages: u32,
    pub total: u64,
}

/// Recognise a workshop item reference: a bare id, the item page URL, the `steam://` page
/// URL, or `workshop:<id>`.
pub fn parse_ref(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Ok(id) = s.parse::<u64>() {
        return (id > 0).then_some(id);
    }
    let rest = s
        .strip_prefix("workshop:")
        .or_else(|| s.strip_prefix("steam://url/CommunityFilePage/"))
        .or_else(|| s.strip_prefix("steam://openurl/"));
    if let Some(rest) = rest {
        return parse_ref(rest);
    }
    let lower = s.to_ascii_lowercase();
    if lower.contains("steamcommunity.com/") || lower.contains("steamcommunity.com%2f") {
        let query = s.split_once('?').map(|(_, q)| q).unwrap_or("");
        return query
            .split('&')
            .find_map(|kv| kv.strip_prefix("id="))
            .and_then(|v| v.split(['&', '#']).next())
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|id| *id > 0);
    }
    None
}

/// The page Steam renders for `q`.
pub fn browse_url(q: &Query) -> String {
    let mut url = format!(
        "{BROWSE_URL}?appid={APP_ID}&section=readytouseitems&childpublishedfileid=0&l=english&numperpage={PER_PAGE}&browsesort={}&days={}&p={}&searchtext={}",
        q.sort.param(),
        if TREND_DAYS.contains(&q.days) { q.days } else { 7 },
        q.page.max(1),
        encode(&q.text)
    );
    if let Some(kind) = q.kind {
        url.push_str("&requiredtags%5B%5D=");
        url.push_str(&encode(kind.tag()));
    }
    for tag in &q.tags {
        url.push_str("&requiredtags%5B%5D=");
        url.push_str(&encode(tag));
    }
    if !q.mature {
        for r in [Rating::Questionable, Rating::Mature] {
            url.push_str("&excludedtags%5B%5D=");
            url.push_str(r.tag());
        }
    }
    url
}

fn encode(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            write!(out, "%{b:02X}").expect("writing to String");
        }
    }
    out
}

/// Parse a rendered browse page. Steam embeds the listing it rendered as a JSON document
/// inside `window.SSR.renderContext=JSON.parse("...")`; its `queryData` string holds the
/// page's query cache, and the `workshop_browse` query in it carries the results.
pub fn parse_browse(html: &str) -> Result<Page> {
    let context = render_context(html)?;
    let cache: Value = context
        .get("queryData")
        .and_then(Value::as_str)
        .map(serde_json::from_str)
        .transpose()?
        .ok_or_else(|| changed("no query cache in the workshop page"))?;
    let data = cache
        .get("queries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|q| {
            q.get("queryKey")
                .and_then(Value::as_array)
                .and_then(|k| k.first())
                .and_then(Value::as_str)
                == Some("workshop_browse")
        })
        .and_then(|q| q.pointer("/state/data"))
        .ok_or_else(|| changed("no workshop_browse query in the workshop page"))?;
    if data.get("eresult").and_then(Value::as_u64) != Some(1) {
        return Err(Error::Network(format!(
            "Steam answered the workshop listing with result {}",
            data.get("eresult").unwrap_or(&Value::Null)
        )));
    }
    let personas: Vec<(u64, String)> = data
        .get("creator_player_link_details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let public = p.get("public_data")?;
            Some((
                num(public.get("steamid"))?,
                public.get("persona_name")?.as_str()?.to_string(),
            ))
        })
        .collect();
    let mut items: Vec<Item> = data
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(item_from)
        .collect();
    for item in &mut items {
        if let Some(creator) = item.creator {
            item.author = personas
                .iter()
                .find(|(id, _)| *id == creator)
                .map(|(_, name)| name.clone());
        }
    }
    Ok(Page {
        items,
        page: num(data.get("current_page")).unwrap_or(1) as u32,
        pages: num(data.get("total_pages")).unwrap_or(1) as u32,
        total: num(data.get("total_count")).unwrap_or(0),
    })
}

fn render_context(html: &str) -> Result<Value> {
    const MARK: &str = "window.SSR.renderContext=JSON.parse(";
    let start = html
        .find(MARK)
        .ok_or_else(|| changed("no rendered listing in the workshop page"))?
        + MARK.len();
    let mut de = serde_json::Deserializer::from_str(&html[start..]);
    let literal = String::deserialize(&mut de)
        .map_err(|e| changed(&format!("unreadable listing in the workshop page: {e}")))?;
    serde_json::from_str(&literal)
        .map_err(|e| changed(&format!("unreadable listing in the workshop page: {e}")))
}

fn changed(what: &str) -> Error {
    Error::Network(format!(
        "{what}; Steam changed the Workshop site and Deadly Wallpaper needs an update"
    ))
}

/// Numbers Steam sends either as JSON numbers or as decimal strings.
fn num(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or("").to_string()
}

fn item_from(v: &Value) -> Option<Item> {
    let id = num(v.get("publishedfileid"))?;
    let tags = v
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| t.get("tag").and_then(Value::as_str).map(str::to_string))
        .collect();
    let description = match v.get("description") {
        Some(Value::String(s)) => s.clone(),
        _ => text(v.get("short_description")),
    };
    let stars = v
        .get("star_rating")
        .and_then(Value::as_i64)
        .filter(|s| (1..=5).contains(s))
        .map(|s| s as u8);
    Some(Item {
        id,
        title: text(v.get("title")),
        author: None,
        creator: num(v.get("creator")),
        description,
        preview_url: v
            .get("preview_url")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        tags,
        size: num(v.get("file_size")).unwrap_or(0),
        created: num(v.get("time_created")).unwrap_or(0),
        updated: num(v.get("time_updated")).unwrap_or(0),
        subscriptions: num(v.get("subscriptions")).unwrap_or(0),
        favorites: num(v.get("favorited")).unwrap_or(0),
        views: num(v.get("views")).unwrap_or(0),
        stars,
        votes: num(v.get("total_votes")).unwrap_or(0),
    })
}

/// Parse the details API's answer; items Steam does not know are left out.
pub fn parse_details(json: &str) -> Result<Vec<Item>> {
    let v: Value = serde_json::from_str(json)?;
    Ok(v
        .pointer("/response/publishedfiledetails")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|d| num(d.get("result")) == Some(1))
        .filter(|d| num(d.get("consumer_app_id")) == Some(APP_ID))
        .filter_map(item_from)
        .collect())
}

/// The uploader's persona name from an item page.
pub fn parse_author(html: &str) -> Option<String> {
    const MARK: &str = "class=\"friendBlockContent\">";
    let start = html.find(MARK)? + MARK.len();
    let end = html[start..].find("<br")? + start;
    let name = unescape(html[start..end].trim());
    (!name.is_empty()).then_some(name)
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| {
                    n.strip_prefix(['x', 'X'])
                        .map(|h| u32::from_str_radix(h, 16).ok())
                        .unwrap_or_else(|| n.parse().ok())
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// HTTP client for the Workshop with a preview image cache.
pub struct Client {
    agent: ureq::Agent,
    cache: PathBuf,
}

impl Client {
    pub fn new(cache_dir: &Path) -> Client {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(format!(
                "{}/{}",
                crate::paths::APP_ID,
                env!("CARGO_PKG_VERSION")
            ))
            .http_status_as_error(false)
            .build();
        Client {
            agent: config.into(),
            cache: cache_dir.join("workshop"),
        }
    }

    fn get(&self, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
        let response = self
            .agent
            .get(url)
            .call()
            .map_err(|e| Error::Network(format!("{url}: {e}")))?;
        if !response.status().is_success() {
            return Err(Error::Network(format!(
                "{url}: HTTP {}",
                response.status()
            )));
        }
        Ok(response)
    }

    pub fn browse(&self, q: &Query) -> Result<Page> {
        let url = browse_url(q);
        let html = self
            .get(&url)?
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(format!("{url}: {e}")))?;
        parse_browse(&html)
    }

    /// Details of up to a hundred items per call.
    pub fn details(&self, ids: &[u64]) -> Result<Vec<Item>> {
        let mut all = Vec::new();
        for chunk in ids.chunks(100) {
            let mut form = vec![("itemcount".to_string(), chunk.len().to_string())];
            for (i, id) in chunk.iter().enumerate() {
                form.push((format!("publishedfileids[{i}]"), id.to_string()));
            }
            let mut response = self
                .agent
                .post(DETAILS_URL)
                .send_form(form)
                .map_err(|e| Error::Network(format!("{DETAILS_URL}: {e}")))?;
            if !response.status().is_success() {
                return Err(Error::Network(format!(
                    "{DETAILS_URL}: HTTP {}",
                    response.status()
                )));
            }
            let body = response
                .body_mut()
                .read_to_string()
                .map_err(|e| Error::Network(format!("{DETAILS_URL}: {e}")))?;
            all.extend(parse_details(&body)?);
        }
        Ok(all)
    }

    /// Full details of one item, with the uploader's name from its page.
    pub fn item(&self, id: u64) -> Result<Item> {
        let mut item = self
            .details(&[id])?
            .into_iter()
            .next()
            .ok_or_else(|| {
                Error::NotFound(format!(
                    "workshop item {id} is not a Wallpaper Engine item, or it is private"
                ))
            })?;
        let url = format!("{ITEM_URL}{id}");
        let html = self
            .get(&url)?
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Network(format!("{url}: {e}")))?;
        item.author = parse_author(&html);
        Ok(item)
    }

    /// The preview image at `url`, downloaded once into the cache.
    pub fn preview(&self, url: &str) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.cache)?;
        let stem = format!("{:016x}", fnv(url.as_bytes()));
        if let Some(existing) = ["gif", "jpg", "png", "webp"]
            .iter()
            .map(|ext| self.cache.join(format!("{stem}.{ext}")))
            .find(|p| p.is_file())
        {
            return Ok(existing);
        }
        let mut response = self.get(url)?;
        let ext = match response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
        {
            "image/gif" => "gif",
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            other => {
                return Err(Error::Network(format!(
                    "{url}: preview is {other:?}, not an image"
                )));
            }
        };
        let bytes = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| Error::Network(format!("{url}: {e}")))?;
        let path = self.cache.join(format!("{stem}.{ext}"));
        crate::paths::write(&path, bytes)?;
        Ok(path)
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a page the way Steam does: the query cache serialised into a string field of
    /// the render context, which is itself a JSON string literal handed to `JSON.parse`.
    fn page(data: Value) -> String {
        let cache = json!({
            "mutations": [],
            "queries": [
                {"state": {"data": null}, "queryKey": ["AOWarningCookie"]},
                {"state": {"data": data}, "queryKey": ["workshop_browse", {"appid": 431960}]}
            ]
        });
        let context = json!({
            "localizationSettings": {},
            "queryData": serde_json::to_string(&cache).unwrap(),
        });
        let literal = serde_json::to_string(&serde_json::to_string(&context).unwrap()).unwrap();
        format!(
            "<html><script nonce=\"x\">window.SSR.loaderData = [\"{{}}\"];window.SSR.renderContext=JSON.parse({literal});window.SSR.clientAssets={{}}</script></html>"
        )
    }

    #[test]
    fn parses_the_rendered_listing() {
        let html = page(json!({
            "eresult": 1, "current_page": 2, "total_pages": 81, "total_count": 2410,
            "results": [{
                "publishedfileid": "3809541486", "creator": "76561199847076364",
                "consumer_appid": 431960, "file_type": 0,
                "preview_url": "https://images.steamusercontent.com/ugc/1/2/",
                "title": "Moon \"Princess\"", "short_description": "Hello everyone!",
                "time_created": 1790572395, "time_updated": 1790572396, "file_size": "17520572",
                "tags": [{"tag": "Scene"}, {"tag": "Fantasy"}, {"tag": "Everyone"}],
                "subscriptions": 1709, "favorited": 98, "views": 120, "star_rating": 4, "total_votes": 28
            }, {
                "publishedfileid": "3810405838", "creator": "76561198102900133",
                "title": "Unrated", "tags": [{"tag": "Video"}, {"tag": "Mature"}], "star_rating": -1
            }],
            "creator_player_link_details": [
                {"public_data": {"steamid": "76561199847076364", "persona_name": "ANWYSE"}}
            ]
        }));
        let p = parse_browse(&html).unwrap();
        assert_eq!((p.page, p.pages, p.total), (2, 81, 2410));
        assert_eq!(p.items.len(), 2);
        let a = &p.items[0];
        assert_eq!(a.id, 3809541486);
        assert_eq!(a.title, "Moon \"Princess\"");
        assert_eq!(a.author.as_deref(), Some("ANWYSE"));
        assert_eq!(a.creator, Some(76561199847076364));
        assert_eq!(a.description, "Hello everyone!");
        assert_eq!(a.kind(), Some(ProjectType::Scene));
        assert_eq!(a.rating(), Some(Rating::Everyone));
        assert_eq!(a.size, 17520572);
        assert_eq!(a.stars, Some(4));
        assert_eq!(a.subscriptions, 1709);
        assert_eq!(
            a.preview_sized(322).unwrap(),
            "https://images.steamusercontent.com/ugc/1/2/?imw=322&imh=322&ima=fit&impolicy=Letterbox&imcolor=%23000000&letterbox=false"
        );
        let b = &p.items[1];
        assert_eq!(b.author, None);
        assert_eq!(b.stars, None);
        assert_eq!(b.kind(), Some(ProjectType::Video));
        assert_eq!(b.rating(), Some(Rating::Mature));
    }

    #[test]
    fn rejects_pages_without_the_listing() {
        let e = parse_browse("<html><body>Sign in</body></html>").unwrap_err();
        assert!(e.to_string().contains("Steam changed"), "{e}");
        let html = page(json!({"eresult": 15, "results": []}));
        let e = parse_browse(&html).unwrap_err();
        assert!(e.to_string().contains("result 15"), "{e}");
    }

    #[test]
    fn parses_details_and_authors() {
        let json = r#"{"response":{"result":1,"resultcount":2,"publishedfiledetails":[
            {"publishedfileid":"1","result":9},
            {"publishedfileid":"3809541486","result":1,"creator":"76561199847076364","consumer_app_id":431960,
             "file_size":"17520572","preview_url":"https://images.steamusercontent.com/ugc/1/2/",
             "title":"Moon Princess","description":"Full text\n\nhere","time_created":1,"time_updated":2,
             "subscriptions":3,"favorited":4,"views":5,"tags":[{"tag":"Scene"}]},
            {"publishedfileid":"7","result":1,"consumer_app_id":578080,"title":"Other game","tags":[]}
        ]}}"#;
        let items = parse_details(json).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].description, "Full text\n\nhere");
        assert_eq!(items[0].tags, vec!["Scene"]);
        let html = "<div class=\"friendBlockContent\">\n\t\t\t\tR&amp;B &#x1F600; &#65;<br>\n<span>Offline</span>";
        assert_eq!(parse_author(html).as_deref(), Some("R&B 😀 A"));
        assert_eq!(parse_author("<html></html>"), None);
    }

    #[test]
    fn recognises_item_references() {
        for (s, id) in [
            ("3809541486", Some(3809541486)),
            (" 42 ", Some(42)),
            ("0", None),
            ("workshop:42", Some(42)),
            ("steam://url/CommunityFilePage/42", Some(42)),
            (
                "https://steamcommunity.com/sharedfiles/filedetails/?id=3809541486&searchtext=x",
                Some(3809541486),
            ),
            (
                "https://steamcommunity.com/workshop/filedetails/?l=english&id=42#c",
                Some(42),
            ),
            ("https://steamcommunity.com/app/431960/workshop/", None),
            ("https://example.org/?id=42", None),
            ("/home/me/wallpaper.mp4", None),
            ("nebula-40c894", None),
        ] {
            assert_eq!(parse_ref(s), id, "{s}");
        }
    }

    #[test]
    fn builds_browse_urls() {
        let url = browse_url(&Query {
            text: "ocean waves & sun".into(),
            sort: Sort::Subscribers,
            days: 30,
            kind: Some(ProjectType::Scene),
            tags: vec!["3840 x 2160".into()],
            mature: false,
            page: 2,
        });
        assert!(url.starts_with("https://steamcommunity.com/workshop/browse/?appid=431960&"));
        assert!(url.contains("browsesort=totaluniquesubscribers"));
        assert!(url.contains("&days=30&p=2&searchtext=ocean+waves+%26+sun"));
        assert!(url.contains("&requiredtags%5B%5D=Scene&requiredtags%5B%5D=3840+x+2160"));
        assert!(url.contains("&excludedtags%5B%5D=Questionable&excludedtags%5B%5D=Mature"));
        let all = browse_url(&Query {
            mature: true,
            days: 5,
            ..Query::default()
        });
        assert!(!all.contains("excludedtags"));
        assert!(all.contains("&days=7&"));
        assert_eq!(Sort::parse("most subscribed"), Some(Sort::Subscribers));
        assert_eq!(Sort::parse("mostrecent"), Some(Sort::Recent));
        assert_eq!(Sort::parse("updated"), Some(Sort::Updated));
        assert_eq!(Sort::parse("bogus"), None);
    }
}
