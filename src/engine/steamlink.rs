//! The Steam side of the Workshop: downloads through the running Steam client, what Steam
//! has fetched already, and library entries kept in step with it.

use super::Engine;
use super::library::ImportOptions;
use crate::error::{Error, Result};
use crate::ipc::server::Reply;
use crate::ipc::{
    DownloadPhase, Event, Response, SteamClient, WorkshopDownload, WorkshopItemStatus,
    WorkshopStatus,
};
use crate::model::wallpaper::WorkshopOrigin;
use crate::we::project::{self, FILE_NAME};
use crate::we::steam::{self, InstalledItem};
use crate::we::steamapi::{self, Call, Lib, Session};
use crate::we::workshop;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Ticks between looks at Steam's workshop folders while auto-import is on.
const SCAN_TICKS: u64 = 3;
/// Ticks between checks whether the Steam client came or went.
const CLIENT_CHECK_TICKS: u64 = 10;
/// How long the Steam connection stays open after the last download, so a run of requests
/// does not sign Wallpaper Engine in and out for each one.
const SESSION_IDLE: Duration = Duration::from_secs(60);
/// How long Steam gets to answer a subscribe call.
const SUBSCRIBE_TIMEOUT: Duration = Duration::from_secs(30);
/// How long Steam may leave a download untouched before it is given up on.
const START_TIMEOUT: Duration = Duration::from_secs(120);
/// How often an untouched download is asked for again.
const KICK_EVERY: Duration = Duration::from_secs(15);
/// Least time between progress broadcasts of one download.
const PROGRESS_EVERY: Duration = Duration::from_millis(900);

/// A Workshop item the Steam client is fetching for the daemon.
pub(super) struct Download {
    id: u64,
    title: Option<String>,
    author: Option<String>,
    /// Apply the wallpaper here once it is in the library.
    display: Option<String>,
    /// The subscribe call Steam has not answered yet.
    subscribing: Option<Call>,
    /// This request subscribed the account, so cancelling unsubscribes it again.
    subscribed_here: bool,
    phase: DownloadPhase,
    done: u64,
    total: u64,
    started: Instant,
    /// Last time Steam was asked to fetch it.
    kicked: Instant,
    /// Last time its progress was broadcast.
    announced: Instant,
}

impl Download {
    fn new(
        id: u64,
        title: Option<String>,
        author: Option<String>,
        display: Option<String>,
    ) -> Download {
        let now = Instant::now();
        Download {
            id,
            title,
            author,
            display,
            subscribing: None,
            subscribed_here: false,
            phase: DownloadPhase::Queued,
            done: 0,
            total: 0,
            started: now,
            kicked: now,
            announced: now,
        }
    }

    fn status(&self) -> WorkshopDownload {
        WorkshopDownload {
            id: self.id,
            title: self.title.clone(),
            phase: self.phase,
            done: self.done,
            total: self.total,
        }
    }

    fn name(&self) -> String {
        self.status().name()
    }
}

/// The Steam client as the engine holds on to it.
#[derive(Default)]
pub(super) struct SteamLink {
    /// The client library, loaded on first use.
    lib: Option<Arc<Lib>>,
    /// Why the last load failed, so the reason is logged once rather than on every try.
    lib_error: Option<String>,
    session: Option<Session>,
    /// Since when the session has had nothing to do.
    idle_since: Option<Instant>,
    /// The account the last session was signed in as.
    account: Option<String>,
    /// Whether the client was running at the last check.
    running: Option<bool>,
    downloads: Vec<Download>,
}

impl Engine {
    pub(super) fn refresh_steam(&mut self) {
        let we = &self.settings.wallpaper_engine;
        self.steam = steam::locate(we.steam_dir.as_deref(), we.assets_dir.as_deref());
        match (&self.steam.steam_dir, &self.steam.assets_dir) {
            (Some(s), Some(a)) => log::info!(
                "Steam at {}; Wallpaper Engine assets at {}",
                s.display(),
                a.display()
            ),
            (Some(s), None) => log::info!(
                "Steam at {}; Wallpaper Engine is not installed there",
                s.display()
            ),
            (None, _) => log::info!("Steam was not found"),
        }
    }

    /// The Steam client library, loaded once it can be; a failed load is tried again next
    /// time, so Steam or Wallpaper Engine installed after the daemon started are picked up.
    fn steam_lib(&mut self) -> Result<Arc<Lib>> {
        if let Some(lib) = &self.steam_link.lib {
            return Ok(lib.clone());
        }
        match Lib::find(&self.steam) {
            Ok(lib) => {
                let lib = Arc::new(lib);
                self.steam_link.lib = Some(lib.clone());
                self.steam_link.lib_error = None;
                Ok(lib)
            }
            Err(e) => {
                let text = e.to_string();
                if self.steam_link.lib_error.as_ref() != Some(&text) {
                    log::warn!("Steam client library: {text}");
                    self.steam_link.lib_error = Some(text);
                }
                Err(e)
            }
        }
    }

    fn steam_client(&mut self) -> SteamClient {
        match self.steam_lib() {
            Ok(lib) if lib.running() => SteamClient::Running,
            Ok(_) => SteamClient::NotRunning,
            Err(e) => SteamClient::Unavailable {
                reason: e.to_string(),
            },
        }
    }

    /// Connect to the Steam client as Wallpaper Engine, unless already connected.
    fn open_steam_session(&mut self) -> Result<()> {
        if self.steam_link.session.is_none() {
            let lib = self.steam_lib()?;
            let session = Session::open(&lib)?;
            log::info!(
                "connected to Steam as Wallpaper Engine; account {}",
                session.account
            );
            self.steam_link.account = Some(session.account.clone());
            self.steam_link.running = Some(true);
            self.steam_link.session = Some(session);
        }
        self.steam_link.idle_since = None;
        Ok(())
    }

    pub(super) fn workshop_status(&mut self) -> WorkshopStatus {
        let client = self.steam_client();
        let entries = self.library.workshop_entries();
        let items = steam::installed(&self.steam)
            .into_iter()
            .map(|it| {
                let entry = entries.iter().find(|(_, o)| o.id == it.id);
                let title = entry
                    .map(|(w, _)| w.title())
                    .or_else(|| {
                        project::Project::load(&it.dir.join(FILE_NAME))
                            .ok()
                            .map(|p| p.title)
                    })
                    .unwrap_or_else(|| it.id.to_string());
                WorkshopItemStatus {
                    id: it.id,
                    stale: entry.is_some_and(|(_, o)| is_stale(o, &it)),
                    wallpaper: entry.map(|(w, _)| w.id.clone()),
                    title,
                    dir: it.dir,
                    updated: it.updated,
                }
            })
            .collect();
        WorkshopStatus {
            steam: self.steam.clone(),
            client,
            account: self.steam_link.account.clone(),
            items,
            downloads: self
                .steam_link
                .downloads
                .iter()
                .map(Download::status)
                .collect(),
        }
    }

    /// Add a downloaded item, or have Steam fetch it and add it when it lands. Returns a
    /// sentence saying which happened.
    pub(super) fn workshop_get(
        &mut self,
        id: u64,
        title: Option<String>,
        author: Option<String>,
        display: Option<String>,
    ) -> Result<String> {
        if let Some(item) = steam::installed_item(&self.steam, id) {
            if let Some(existing) = self.library.find_workshop(id) {
                let stale =
                    WorkshopOrigin::load(&existing.dir).is_some_and(|o| is_stale(&o, &item));
                if stale {
                    self.failed_items.retain(|(i, _)| *i != id);
                    let author = author.or_else(|| existing.info.author.clone());
                    self.workshop_import(item, author, display, Some(existing.id.clone()));
                    return Ok(format!(
                        "Refreshing '{}' from Steam's newer download",
                        existing.title()
                    ));
                }
                if let Some(d) = display {
                    self.layout.assign(&d, &existing.id);
                    self.reconcile();
                }
                return Ok(format!("'{}' is already in the library", existing.title()));
            }
            self.failed_items.retain(|(i, _)| *i != id);
            self.workshop_import(item, author, display, None);
            return Ok(format!(
                "Adding {} from Steam's download",
                title.unwrap_or_else(|| format!("item {id}"))
            ));
        }
        if let Some(d) = self.steam_link.downloads.iter().find(|d| d.id == id) {
            return Ok(format!("{} is already on its way", d.name()));
        }
        self.open_steam_session()?;
        let link = &mut self.steam_link;
        let session = link.session.as_ref().expect("opened just above");
        let mut download = Download::new(id, title, author, display);
        if session.item_state(id).subscribed() {
            if !session.download(id) {
                return Err(Error::Platform(format!(
                    "Steam declined to download item {id}"
                )));
            }
        } else {
            download.subscribing = Some(session.subscribe(id));
            download.subscribed_here = true;
            download.phase = DownloadPhase::Subscribing;
        }
        let name = download.name();
        link.downloads.push(download);
        self.broadcast(Event::Workshop);
        Ok(format!("Steam is fetching {name}"))
    }

    /// Stop a download; the account is unsubscribed again when this daemon subscribed it.
    pub(super) fn workshop_cancel(&mut self, id: u64) -> Result<String> {
        let link = &mut self.steam_link;
        let Some(pos) = link.downloads.iter().position(|d| d.id == id) else {
            return Err(Error::NotFound(format!("nothing is downloading item {id}")));
        };
        if link.downloads[pos].phase == DownloadPhase::Importing {
            return Err(Error::Invalid(format!(
                "{} has finished downloading and is being added to the library",
                link.downloads[pos].name()
            )));
        }
        let download = link.downloads.remove(pos);
        let text = if download.subscribed_here {
            if let Some(session) = &link.session {
                session.unsubscribe(id);
            }
            format!("Unsubscribed from {}", download.name())
        } else {
            format!("Stopped waiting for {}", download.name())
        };
        self.broadcast(Event::Workshop);
        Ok(text)
    }

    /// Add everything Steam has that the library lacks, refresh what Steam updated, and have
    /// Steam fetch every subscription it has not downloaded. The first two need only Steam's
    /// folders; when the Steam client cannot be reached for the third, that is the error.
    pub(super) fn workshop_sync(&mut self) -> Result<String> {
        self.failed_items.clear();
        self.last_installed.clear();
        if self.steam.steam_dir.is_none() {
            return Err(Error::Unsupported(steamapi::STEAM_NOT_FOUND.into()));
        }
        let installed = steam::installed(&self.steam);
        let entries = self.library.workshop_entries();
        let mut added = 0;
        let mut refreshed = 0;
        for item in installed.iter().cloned() {
            if self.importing.contains(&item.id) {
                continue;
            }
            match entries.iter().find(|(_, o)| o.id == item.id) {
                None => {
                    added += 1;
                    self.workshop_import(item, None, None, None);
                }
                Some((w, o)) if is_stale(o, &item) => {
                    refreshed += 1;
                    self.workshop_import(item, w.info.author.clone(), None, Some(w.id.clone()));
                }
                Some(_) => {}
            }
        }
        let mut fetching = 0;
        let mut declined = Vec::new();
        let steam = self.open_steam_session();
        if steam.is_ok() {
            let link = &mut self.steam_link;
            let session = link.session.as_ref().expect("opened just above");
            for id in session.subscribed() {
                if installed.iter().any(|i| i.id == id) || link.downloads.iter().any(|d| d.id == id)
                {
                    continue;
                }
                if !session.download(id) {
                    declined.push(id);
                    continue;
                }
                link.downloads.push(Download::new(id, None, None, None));
                fetching += 1;
            }
        }
        self.broadcast(Event::Workshop);
        let count = |n: usize, what: &str| {
            if n == 1 {
                format!("1 {what}")
            } else {
                format!("{n} {what}s")
            }
        };
        let mut parts = Vec::new();
        if fetching > 0 {
            parts.push(format!(
                "fetching {} Steam had not downloaded",
                count(fetching, "subscription")
            ));
        }
        if added > 0 {
            parts.push(format!("adding {}", count(added, "downloaded item")));
        }
        if refreshed > 0 {
            parts.push(format!("refreshing {}", count(refreshed, "updated item")));
        }
        if !declined.is_empty() {
            log::warn!(
                "Steam declined to download subscribed items {}",
                declined
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            parts.push(format!(
                "Steam declined to fetch {} (the log names them)",
                count(declined.len(), "subscription")
            ));
        }
        let mut done = parts.join(", ");
        if let Some(first) = done.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        match steam {
            Ok(()) if done.is_empty() => {
                Ok("Every subscription is downloaded and in the library".into())
            }
            Ok(()) => Ok(done),
            Err(e) if done.is_empty() => Err(Error::Unsupported(format!(
                "Subscriptions Steam has not downloaded were not fetched: {e}"
            ))),
            Err(e) => Err(Error::Unsupported(format!(
                "{done}; subscriptions Steam has not downloaded were not fetched: {e}"
            ))),
        }
    }

    /// Every tick: move downloads along, let an idle Steam connection go, and notice the
    /// client coming or going.
    pub(super) fn workshop_tick(&mut self) {
        if self.ticks % CLIENT_CHECK_TICKS == 0 {
            self.check_steam_client();
        }
        self.poll_downloads();
        if self.ticks % SCAN_TICKS == 0 {
            self.workshop_scan();
        }
    }

    fn check_steam_client(&mut self) {
        let Some(lib) = &self.steam_link.lib else {
            return;
        };
        let running = lib.running();
        if !running && self.steam_link.session.is_some() {
            log::warn!("Steam closed while connected to it");
            self.steam_link.session = None;
            self.steam_link.idle_since = None;
        }
        if self.steam_link.running != Some(running) {
            self.steam_link.running = Some(running);
            self.broadcast(Event::Workshop);
        }
    }

    fn poll_downloads(&mut self) {
        let link = &mut self.steam_link;
        if let Some(session) = &link.session {
            session.pump();
        }
        if link.downloads.is_empty() {
            if link.session.is_some()
                && link.idle_since.get_or_insert_with(Instant::now).elapsed() >= SESSION_IDLE
            {
                log::info!("disconnected from Steam after an idle minute");
                link.session = None;
                link.idle_since = None;
            }
            return;
        }
        let Some(session) = link.session.as_ref() else {
            let downloads = std::mem::take(&mut link.downloads);
            for d in downloads {
                self.report(&Error::Unsupported(format!(
                    "Steam closed before {} finished downloading",
                    d.name()
                )));
            }
            self.broadcast(Event::Workshop);
            return;
        };
        let now = Instant::now();
        let mut finished = Vec::new();
        let mut failed = Vec::new();
        let mut changed = false;
        let mut kept = Vec::new();
        for mut d in std::mem::take(&mut link.downloads) {
            if d.phase == DownloadPhase::Importing {
                kept.push(d);
                continue;
            }
            if let Some(call) = d.subscribing {
                match session.call_result(call) {
                    None if d.started.elapsed() >= SUBSCRIBE_TIMEOUT => {
                        failed.push((
                            d,
                            Error::Network(
                                "Steam did not answer the subscription request in 30 seconds"
                                    .into(),
                            ),
                        ));
                        continue;
                    }
                    None => {
                        kept.push(d);
                        continue;
                    }
                    Some(Err(e)) => {
                        failed.push((d, e));
                        continue;
                    }
                    Some(Ok(())) => {
                        d.subscribing = None;
                        if !session.download(d.id) {
                            failed
                                .push((d, Error::Platform("Steam declined to download it".into())));
                            continue;
                        }
                        d.kicked = now;
                        d.phase = DownloadPhase::Queued;
                        changed = true;
                    }
                }
            }
            let state = session.item_state(d.id);
            if state.ready() {
                match session.install_info(d.id) {
                    Some(install) if install.dir.join(FILE_NAME).is_file() => {
                        d.phase = DownloadPhase::Importing;
                        finished.push((d, install));
                    }
                    Some(install) => failed.push((
                        d,
                        Error::Media(format!(
                            "Steam's download at {} holds no {FILE_NAME}, so it is not a Wallpaper Engine wallpaper",
                            install.dir.display()
                        )),
                    )),
                    None => failed.push((
                        d,
                        Error::Platform(
                            "Steam reports it installed but does not say where".into(),
                        ),
                    )),
                }
                changed = true;
                continue;
            }
            if state.downloading() {
                if d.phase != DownloadPhase::Downloading {
                    d.phase = DownloadPhase::Downloading;
                    changed = true;
                }
                if let Some((done, total)) = session.download_info(d.id) {
                    if (done, total) != (d.done, d.total) {
                        d.done = done;
                        d.total = total;
                        if d.announced.elapsed() >= PROGRESS_EVERY {
                            d.announced = now;
                            changed = true;
                        }
                    }
                }
            } else if state.fetching() {
                if d.phase != DownloadPhase::Queued {
                    d.phase = DownloadPhase::Queued;
                    changed = true;
                }
            } else {
                // Steam has not taken it on: ask again now and then, give up eventually.
                if d.kicked.elapsed() >= KICK_EVERY {
                    session.download(d.id);
                    d.kicked = now;
                }
                if d.started.elapsed() >= START_TIMEOUT {
                    let reason = if session.app_installed() {
                        "Steam did not start downloading it in two minutes: check that downloads are not paused in Steam".to_string()
                    } else {
                        "Steam did not start downloading it in two minutes. Wallpaper Engine is not installed in this Steam client, which may be why; check too that downloads are not paused in Steam".to_string()
                    };
                    failed.push((d, Error::Platform(reason)));
                    continue;
                }
            }
            kept.push(d);
        }
        link.downloads = kept;
        for (d, _) in failed.iter().filter(|(d, _)| d.subscribed_here) {
            session.unsubscribe(d.id);
        }
        for (d, e) in failed {
            log::warn!("workshop item {}: {e}", d.id);
            self.broadcast(Event::Error {
                message: format!("{}: {e}", d.name()),
            });
            changed = true;
        }
        for (d, install) in finished {
            let item = InstalledItem {
                id: d.id,
                dir: install.dir,
                updated: Some(install.updated),
            };
            let replace = self.library.find_workshop(d.id).map(|w| w.id);
            let (author, display) = (d.author.clone(), d.display.clone());
            self.steam_link.downloads.push(d);
            self.workshop_import(item, author, display, replace);
        }
        if changed {
            self.broadcast(Event::Workshop);
        }
    }

    /// With auto-import on, add whatever new or updated items Steam's folders hold.
    fn workshop_scan(&mut self) {
        if !self.settings.wallpaper_engine.auto_import || self.steam.libraries.is_empty() {
            return;
        }
        let installed = steam::installed(&self.steam);
        let seen: Vec<(u64, Option<u64>)> = installed.iter().map(|i| (i.id, i.updated)).collect();
        if seen == self.last_installed {
            return;
        }
        self.last_installed = seen;
        let entries = self.library.workshop_entries();
        for item in installed {
            if self.importing.contains(&item.id)
                || self.failed_items.contains(&(item.id, item.updated))
                || self.steam_link.downloads.iter().any(|d| d.id == item.id)
            {
                continue;
            }
            match entries.iter().find(|(_, o)| o.id == item.id) {
                None => self.workshop_import(item, None, None, None),
                Some((w, o)) if is_stale(o, &item) => {
                    let (author, id) = (w.info.author.clone(), w.id.clone());
                    self.workshop_import(item, author, None, Some(id));
                }
                Some(_) => {}
            }
        }
    }

    /// Import `item` off the main thread. `replace` names the library entry it refreshes.
    fn workshop_import(
        &mut self,
        item: InstalledItem,
        author: Option<String>,
        display: Option<String>,
        replace: Option<String>,
    ) {
        if self.importing.contains(&item.id) {
            return;
        }
        self.importing.push(item.id);
        let lib = self.library.clone();
        let (copy, thumbnails, temp) = self.import_options();
        let cache = self.paths.cache_dir.clone();
        let id = item.id;
        let origin = WorkshopOrigin {
            id,
            updated: item.updated,
            source: Some(item.dir.clone()),
        };
        let refreshing = replace.is_some();
        let updated = item.updated;
        self.job(
            move || {
                let author = author.or_else(|| match workshop::Client::new(&cache).author(id) {
                    Ok(name) => name,
                    Err(e) => {
                        log::warn!("workshop item {id} author: {e}");
                        None
                    }
                });
                let fresh = lib.import_project(
                    &item.dir,
                    &ImportOptions {
                        copy,
                        thumbnails,
                        temp_dir: &temp,
                    },
                    Some(origin),
                    author,
                )?;
                match &replace {
                    Some(old) => lib.replace(old, &fresh),
                    None => Ok(fresh),
                }
            },
            move |e, r| {
                e.importing.retain(|i| *i != id);
                e.steam_link.downloads.retain(|d| d.id != id);
                match r {
                    Ok(w) => {
                        log::info!("workshop item {id} imported as '{}'", w.title());
                        if refreshing {
                            e.active.retain(|a| a.wallpaper.id != w.id);
                        }
                        e.broadcast(Event::Info {
                            message: format!(
                                "{} '{}' from the Steam Workshop",
                                if refreshing { "Refreshed" } else { "Added" },
                                w.title()
                            ),
                        });
                        if let Some(d) = display {
                            e.layout.assign(&d, &w.id);
                        }
                        e.reconcile();
                    }
                    Err(err) => {
                        e.failed_items.push((id, updated));
                        e.report(&Error::Media(format!("workshop item {id}: {err}")));
                    }
                }
                e.broadcast(Event::Library);
                e.broadcast(Event::Workshop);
            },
        );
    }

    /// `import` or `set` with a workshop reference: fetch the item, then reply once it is in
    /// the library (or as soon as Steam has been asked for it).
    pub(super) fn import_workshop_ref(
        &mut self,
        id: u64,
        display: Option<String>,
        reply: Reply,
    ) -> bool {
        if let Some(existing) = self.library.find_workshop(id) {
            if let Some(d) = display {
                self.layout.assign(&d, &existing.id);
                self.reconcile();
            }
            reply(Response::Wallpaper(existing.summary()));
            return true;
        }
        match self.workshop_get(id, None, None, display) {
            Ok(text) => reply(Response::Text(text)),
            Err(e) => reply(Response::error(&e)),
        }
        true
    }
}

/// Whether Steam's download is newer than what the entry was made from.
fn is_stale(origin: &WorkshopOrigin, item: &InstalledItem) -> bool {
    matches!((origin.updated, item.updated), (Some(o), Some(i)) if i > o)
}
