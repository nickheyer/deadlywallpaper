//! The Steamworks API of the running Steam client, spoken as Wallpaper Engine so that Steam
//! subscribes to and downloads Workshop items for the signed-in account. Valve's `steam_api`
//! library is loaded at run time from the Steam install, Wallpaper Engine's files or an installed
//! game that bundles it, so nothing links against it and the daemon runs the same without Steam.

use crate::error::{Error, Result};
use crate::we::steam::{APP_ID, SteamInfo};
use libloading::Library;
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Callback ids of the answers to subscribe and unsubscribe calls
/// (`RemoteStorageSubscribePublishedFileResult_t` and its unsubscribe twin).
const SUBSCRIBE_RESULT: c_int = 1300 + 13;
const UNSUBSCRIBE_RESULT: c_int = 1300 + 15;

pub const STEAM_NOT_FOUND: &str = "Steam was not found on this machine: install it, or point Settings → Wallpaper Engine at its folder";

#[cfg(target_os = "linux")]
const LIBRARY_NAME: &str = "libsteam_api.so";
#[cfg(target_os = "macos")]
const LIBRARY_NAME: &str = "libsteam_api.dylib";
#[cfg(all(windows, target_pointer_width = "64"))]
const LIBRARY_NAME: &str = "steam_api64.dll";
#[cfg(all(windows, not(target_pointer_width = "64")))]
const LIBRARY_NAME: &str = "steam_api.dll";
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
const LIBRARY_NAME: &str = "libsteam_api.so";

/// Folders inside an installed game's folder where games keep their copy of the library.
#[cfg(target_os = "linux")]
const GAME_DIRS: &[&str] = &["", "bin", "bin/linux64", "linux64"];
#[cfg(windows)]
const GAME_DIRS: &[&str] = &["", "bin", "bin/win64", "bin/x64"];
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
const GAME_DIRS: &[&str] = &[""];
/// Folders inside a game's `.app` bundle where Mac games keep their copy.
#[cfg(target_os = "macos")]
const BUNDLE_DIRS: &[&str] = &["Contents/MacOS", "Contents/Frameworks", "Contents/Plugins"];

/// Every copy of Valve's `steam_api` library on this machine, likeliest first. Steam's Linux
/// client ships one in its runtime folder, Wallpaper Engine's Windows install carries the one
/// it uses, and every native Steam game bundles the one it was built with.
pub fn library_candidates(info: &SteamInfo) -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "linux")]
    if let Some(steam_dir) = &info.steam_dir {
        push_if_file(&mut out, steam_dir.join("steamrt64").join(LIBRARY_NAME));
    }
    #[cfg(windows)]
    if let Some(install_dir) = &info.install_dir {
        push_if_file(
            &mut out,
            install_dir
                .join("distribution")
                .join("bin")
                .join(LIBRARY_NAME),
        );
    }
    for library in &info.libraries {
        let Ok(games) = std::fs::read_dir(library.join("steamapps").join("common")) else {
            continue;
        };
        for game in games.flatten().map(|e| e.path()) {
            bundled(&game, &mut out);
        }
    }
    out
}

/// The copies an installed game keeps where games usually keep them.
fn bundled(game: &Path, out: &mut Vec<PathBuf>) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir(game) {
            for app in entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "app"))
            {
                for dir in BUNDLE_DIRS {
                    push_if_file(out, app.join(dir).join(LIBRARY_NAME));
                }
            }
        }
        push_if_file(out, game.join(LIBRARY_NAME));
    }
    #[cfg(not(target_os = "macos"))]
    for dir in GAME_DIRS {
        let folder = if dir.is_empty() {
            game.to_path_buf()
        } else {
            game.join(dir)
        };
        push_if_file(out, folder.join(LIBRARY_NAME));
    }
}

fn push_if_file(out: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() {
        out.push(path);
    }
}

/// Tell Steam which application this process is. Steam reads it when the API starts.
///
/// Call it from the daemon's entry point before the runtime, the IPC server or any worker
/// thread exists: changing the environment while other threads may read it is unsound.
pub fn announce_app() {
    let id = APP_ID.to_string();
    // SAFETY: the caller guarantees this runs before any other thread of the process starts,
    // so nothing can be reading the environment concurrently.
    unsafe {
        std::env::set_var("SteamAppId", &id);
        std::env::set_var("SteamGameId", &id);
    }
}

/// Drop the announcement from a child's environment, so programs the daemon starts are not
/// taken for Wallpaper Engine by Steam.
pub fn scrub(cmd: &mut std::process::Command) -> &mut std::process::Command {
    cmd.env_remove("SteamAppId").env_remove("SteamGameId")
}

type Iface = *mut c_void;
type Accessor = unsafe extern "C" fn() -> Iface;

/// The exported functions the daemon uses, resolved once from the loaded library.
pub struct Lib {
    _library: Library,
    is_running: unsafe extern "C" fn() -> bool,
    init: unsafe extern "C" fn() -> bool,
    shutdown: unsafe extern "C" fn(),
    run_callbacks: unsafe extern "C" fn(),
    user: Accessor,
    logged_on: unsafe extern "C" fn(Iface) -> bool,
    apps: Accessor,
    owns_app: unsafe extern "C" fn(Iface, u32) -> bool,
    app_installed: unsafe extern "C" fn(Iface, u32) -> bool,
    friends: Accessor,
    persona_name: unsafe extern "C" fn(Iface) -> *const c_char,
    utils: Accessor,
    call_completed: unsafe extern "C" fn(Iface, u64, *mut bool) -> bool,
    call_result: unsafe extern "C" fn(Iface, u64, *mut c_void, c_int, c_int, *mut bool) -> bool,
    ugc: Accessor,
    subscribe: unsafe extern "C" fn(Iface, u64) -> u64,
    unsubscribe: unsafe extern "C" fn(Iface, u64) -> u64,
    download: unsafe extern "C" fn(Iface, u64, bool) -> bool,
    item_state: unsafe extern "C" fn(Iface, u64) -> u32,
    download_info: unsafe extern "C" fn(Iface, u64, *mut u64, *mut u64) -> bool,
    install_info: unsafe extern "C" fn(Iface, u64, *mut u64, *mut c_char, u32, *mut u32) -> bool,
    subscribed_count: unsafe extern "C" fn(Iface) -> u32,
    subscribed_items: unsafe extern "C" fn(Iface, *mut u64, u32) -> u32,
}

impl Lib {
    /// Load the library at `path`. Loading connects to nothing; [`Session::open`] does.
    pub fn load(path: &Path) -> Result<Lib> {
        // SAFETY: the file is Valve's steam_api redistributable from the Steam or Wallpaper
        // Engine install; its initialisers only set up internal state.
        let library = unsafe { Library::new(path) }
            .map_err(|e| Error::Platform(format!("{}: {e}", path.display())))?;
        macro_rules! sym {
            ($name:literal) => {{
                // SAFETY: the signature is the one steam_api_flat.h declares for this symbol,
                // and the library stays loaded for as long as the pointer (it lives in the
                // same struct).
                let symbol = unsafe { library.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|e| Error::Platform(format!("{}: {e}", $name)))?;
                *symbol
            }};
        }
        Ok(Lib {
            is_running: sym!("SteamAPI_IsSteamRunning"),
            init: sym!("SteamAPI_InitSafe"),
            shutdown: sym!("SteamAPI_Shutdown"),
            run_callbacks: sym!("SteamAPI_RunCallbacks"),
            user: accessor(&library, "User")?,
            logged_on: sym!("SteamAPI_ISteamUser_BLoggedOn"),
            apps: accessor(&library, "Apps")?,
            owns_app: sym!("SteamAPI_ISteamApps_BIsSubscribedApp"),
            app_installed: sym!("SteamAPI_ISteamApps_BIsAppInstalled"),
            friends: accessor(&library, "Friends")?,
            persona_name: sym!("SteamAPI_ISteamFriends_GetPersonaName"),
            utils: accessor(&library, "Utils")?,
            call_completed: sym!("SteamAPI_ISteamUtils_IsAPICallCompleted"),
            call_result: sym!("SteamAPI_ISteamUtils_GetAPICallResult"),
            ugc: accessor(&library, "UGC")?,
            subscribe: sym!("SteamAPI_ISteamUGC_SubscribeItem"),
            unsubscribe: sym!("SteamAPI_ISteamUGC_UnsubscribeItem"),
            download: sym!("SteamAPI_ISteamUGC_DownloadItem"),
            item_state: sym!("SteamAPI_ISteamUGC_GetItemState"),
            download_info: sym!("SteamAPI_ISteamUGC_GetItemDownloadInfo"),
            install_info: sym!("SteamAPI_ISteamUGC_GetItemInstallInfo"),
            subscribed_count: sym!("SteamAPI_ISteamUGC_GetNumSubscribedItems"),
            subscribed_items: sym!("SteamAPI_ISteamUGC_GetSubscribedItems"),
            _library: library,
        })
    }

    /// Load the first copy of the library on this machine that works.
    pub fn find(info: &SteamInfo) -> Result<Lib> {
        if info.steam_dir.is_none() {
            return Err(Error::Unsupported(STEAM_NOT_FOUND.into()));
        }
        let candidates = library_candidates(info);
        let mut first_error = None;
        for path in &candidates {
            match Lib::load(path) {
                Ok(lib) => {
                    log::info!("Steam client library: {}", path.display());
                    return Ok(lib);
                }
                Err(e) => {
                    log::debug!("Steam client library: {e}");
                    first_error.get_or_insert(e);
                }
            }
        }
        Err(match first_error {
            Some(e) if candidates.len() == 1 => e,
            Some(e) => Error::Platform(format!(
                "none of the {} copies of {LIBRARY_NAME} found could be loaded; the first: {e}",
                candidates.len()
            )),
            None => Error::Unsupported(format!(
                "No {LIBRARY_NAME} was found: Steam itself ships none here and no installed Steam game bundles one; installing any native Steam game provides it"
            )),
        })
    }

    /// Whether a Steam client is running for this user; answered without connecting to it.
    pub fn running(&self) -> bool {
        // SAFETY: a plain call into the loaded library.
        unsafe { (self.is_running)() }
    }
}

/// The accessor of the library's own version of an interface, `SteamAPI_SteamUGC_v021` and
/// the like. Each SDK exports one version per interface, and its `SteamAPI_ISteam*` functions
/// expect that version's layout, so the version asked for has to be the library's own.
fn accessor(library: &Library, interface: &str) -> Result<Accessor> {
    for version in (1..=99).rev() {
        let name = format!("SteamAPI_Steam{interface}_v{version:03}\0");
        // SAFETY: every versioned accessor in steam_api_flat.h takes nothing and returns the
        // interface pointer, and the library stays loaded for as long as the pointer.
        if let Ok(symbol) = unsafe { library.get::<Accessor>(name.as_bytes()) } {
            return Ok(*symbol);
        }
    }
    Err(Error::Platform(format!(
        "the steam_api library exports no SteamAPI_Steam{interface} accessor"
    )))
}

/// What Steam's client knows about one item; the `EItemState` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemState(u32);

impl ItemState {
    pub fn subscribed(self) -> bool {
        self.0 & 1 != 0
    }

    pub fn installed(self) -> bool {
        self.0 & 4 != 0
    }

    pub fn needs_update(self) -> bool {
        self.0 & 8 != 0
    }

    pub fn downloading(self) -> bool {
        self.0 & 16 != 0
    }

    pub fn download_pending(self) -> bool {
        self.0 & 32 != 0
    }

    /// Installed with nothing left to fetch.
    pub fn ready(self) -> bool {
        self.installed() && !self.needs_update() && !self.downloading() && !self.download_pending()
    }

    /// Steam has taken the item on: it is queued, downloading or needs an update.
    pub fn fetching(self) -> bool {
        self.needs_update() || self.downloading() || self.download_pending()
    }
}

/// A subscribe or unsubscribe request Steam is still answering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    handle: u64,
    callback: c_int,
}

/// `RemoteStorage(Un)SubscribePublishedFileResult_t`: an `EResult` then the item id. Valve
/// packs callback structs to 4 bytes on Linux and macOS and to 8 bytes on Windows, and Steam
/// hands the result over only to a buffer of exactly that size.
#[cfg_attr(any(target_os = "linux", target_os = "macos"), repr(C, packed(4)))]
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), repr(C))]
struct FileResult {
    result: i32,
    id: u64,
}

/// Where Steam put a finished download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    pub dir: PathBuf,
    pub size: u64,
    /// The item's update time, as Steam's workshop manifest records it.
    pub updated: u64,
}

/// A connection to the running Steam client as Wallpaper Engine. Steam shows the account as
/// playing Wallpaper Engine while it is open, so it is kept only while downloads run.
pub struct Session {
    lib: Arc<Lib>,
    user: Iface,
    apps: Iface,
    friends: Iface,
    utils: Iface,
    ugc: Iface,
    /// The signed-in account's persona name.
    pub account: String,
}

impl Session {
    pub fn open(lib: &Arc<Lib>) -> Result<Session> {
        if !lib.running() {
            return Err(Error::Unsupported(
                "Steam is not running: start it and try again".into(),
            ));
        }
        // SAFETY: plain calls into the loaded library; the session is built right after
        // `init` succeeds so that every exit path shuts the API down again.
        if !unsafe { (lib.init)() } {
            return Err(Error::Platform(
                "Steam did not accept the connection: make sure the Steam client is signed in and up to date".into(),
            ));
        }
        let mut session = Session {
            lib: lib.clone(),
            user: std::ptr::null_mut(),
            apps: std::ptr::null_mut(),
            friends: std::ptr::null_mut(),
            utils: std::ptr::null_mut(),
            ugc: std::ptr::null_mut(),
            account: String::new(),
        };
        // SAFETY: the API is initialised; accessors return null only when the client is older
        // than the interfaces asked for, which is checked before any is used.
        unsafe {
            session.user = (lib.user)();
            session.apps = (lib.apps)();
            session.friends = (lib.friends)();
            session.utils = (lib.utils)();
            session.ugc = (lib.ugc)();
        }
        if [
            session.user,
            session.apps,
            session.friends,
            session.utils,
            session.ugc,
        ]
        .iter()
        .any(|p| p.is_null())
        {
            return Err(Error::Platform(
                "This Steam client is too old for the Steamworks interfaces Deadly Wallpaper uses: update Steam".into(),
            ));
        }
        // SAFETY: the interfaces are live for the session's lifetime.
        unsafe {
            if !(lib.logged_on)(session.user) {
                return Err(Error::Unsupported(
                    "Steam is running but not signed in: sign in and try again".into(),
                ));
            }
            let name = (lib.persona_name)(session.friends);
            if !name.is_null() {
                session.account = CStr::from_ptr(name).to_string_lossy().into_owned();
            }
            if !(lib.owns_app)(session.apps, APP_ID as u32) {
                return Err(Error::Unsupported(format!(
                    "The Steam account {} does not own Wallpaper Engine, and the Workshop hands its items only to accounts that do",
                    if session.account.is_empty() {
                        "signed in"
                    } else {
                        session.account.as_str()
                    }
                )));
            }
        }
        Ok(session)
    }

    /// Let the client deliver what it has for this process; call it regularly.
    pub fn pump(&self) {
        // SAFETY: the API is initialised for as long as the session lives.
        unsafe { (self.lib.run_callbacks)() }
    }

    /// Whether Wallpaper Engine itself is installed in this Steam client.
    pub fn app_installed(&self) -> bool {
        // SAFETY: `apps` is live for the session's lifetime.
        unsafe { (self.lib.app_installed)(self.apps, APP_ID as u32) }
    }

    pub fn item_state(&self, id: u64) -> ItemState {
        // SAFETY: `ugc` is live for the session's lifetime.
        ItemState(unsafe { (self.lib.item_state)(self.ugc, id) })
    }

    /// Subscribe the account to the item; Steam then keeps it downloaded and up to date.
    pub fn subscribe(&self, id: u64) -> Call {
        Call {
            // SAFETY: `ugc` is live for the session's lifetime.
            handle: unsafe { (self.lib.subscribe)(self.ugc, id) },
            callback: SUBSCRIBE_RESULT,
        }
    }

    pub fn unsubscribe(&self, id: u64) -> Call {
        Call {
            // SAFETY: `ugc` is live for the session's lifetime.
            handle: unsafe { (self.lib.unsubscribe)(self.ugc, id) },
            callback: UNSUBSCRIBE_RESULT,
        }
    }

    /// Ask Steam to fetch the item now, ahead of its other downloads. `false` means Steam
    /// declined outright.
    pub fn download(&self, id: u64) -> bool {
        // SAFETY: `ugc` is live for the session's lifetime.
        unsafe { (self.lib.download)(self.ugc, id, true) }
    }

    /// `(bytes downloaded, bytes in total)` of a download under way; the total is zero until
    /// Steam has started it.
    pub fn download_info(&self, id: u64) -> Option<(u64, u64)> {
        let (mut done, mut total) = (0u64, 0u64);
        // SAFETY: `ugc` is live and both out-pointers point at locals.
        let known = unsafe { (self.lib.download_info)(self.ugc, id, &mut done, &mut total) };
        known.then_some((done, total))
    }

    /// Where an installed item's files are.
    pub fn install_info(&self, id: u64) -> Option<Install> {
        let mut size = 0u64;
        let mut updated = 0u32;
        let mut folder = vec![0u8; 4096];
        // SAFETY: `ugc` is live; the buffer's length is passed along with it and the
        // out-pointers point at locals.
        let known = unsafe {
            (self.lib.install_info)(
                self.ugc,
                id,
                &mut size,
                folder.as_mut_ptr().cast(),
                folder.len() as u32,
                &mut updated,
            )
        };
        if !known {
            return None;
        }
        let dir = CStr::from_bytes_until_nul(&folder)
            .ok()?
            .to_string_lossy()
            .into_owned();
        (!dir.is_empty()).then(|| Install {
            dir: PathBuf::from(dir),
            size,
            updated: updated as u64,
        })
    }

    /// Every item the account is subscribed to.
    pub fn subscribed(&self) -> Vec<u64> {
        // SAFETY: `ugc` is live; the vector's capacity is passed as the buffer size.
        unsafe {
            let count = (self.lib.subscribed_count)(self.ugc);
            let mut ids = vec![0u64; count as usize];
            let got = (self.lib.subscribed_items)(self.ugc, ids.as_mut_ptr(), count);
            ids.truncate(got as usize);
            ids
        }
    }

    /// Steam's answer to a call: `None` while it is still working on it.
    pub fn call_result(&self, call: Call) -> Option<Result<()>> {
        let mut failed = false;
        // SAFETY: `utils` is live and `failed` is a local.
        let completed = unsafe { (self.lib.call_completed)(self.utils, call.handle, &mut failed) };
        if !completed {
            return None;
        }
        if failed {
            return Some(Err(Error::Network(
                "Steam could not reach its servers for the request".into(),
            )));
        }
        let mut out = FileResult { result: 0, id: 0 };
        // SAFETY: `utils` is live; the buffer is the struct Steam fills for this callback id,
        // and its size is passed along.
        let copied = unsafe {
            (self.lib.call_result)(
                self.utils,
                call.handle,
                (&mut out as *mut FileResult).cast(),
                std::mem::size_of::<FileResult>() as c_int,
                call.callback,
                &mut failed,
            )
        };
        if !copied || failed {
            return Some(Err(Error::Network(
                "Steam did not deliver the answer to the request".into(),
            )));
        }
        let (result, id) = (out.result, out.id);
        log::debug!(
            "Steam answered call {} for item {id}: {result}",
            call.handle
        );
        Some(match result {
            1 | 29 => Ok(()),
            code => Err(Error::Network(format!(
                "Steam answered: {}",
                eresult_text(code)
            ))),
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // SAFETY: the API was initialised when the session was made and is shut down once.
        unsafe { (self.lib.shutdown)() }
    }
}

/// A Steam `EResult` in words.
pub fn eresult_text(code: i32) -> String {
    match code {
        1 => "ok".into(),
        2 => "a generic failure".into(),
        8 => "an invalid parameter".into(),
        9 => "no such Workshop item".into(),
        10 => "the client is busy".into(),
        15 => "access denied; the item may be hidden or the account may not own Wallpaper Engine"
            .into(),
        16 => "the request timed out".into(),
        20 => "the Workshop service is unavailable right now".into(),
        21 => "the account is not signed in".into(),
        25 => "a limit was exceeded".into(),
        29 => "already done".into(),
        84 => "too many requests; try again in a little while".into(),
        n => format!("result code {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_state_flags_read_as_steam_sets_them() {
        assert!(!ItemState(0).ready());
        assert!(!ItemState(0).fetching());
        let queued = ItemState(40);
        assert!(queued.needs_update() && queued.download_pending() && queued.fetching());
        let downloading = ItemState(56);
        assert!(downloading.downloading() && !downloading.ready());
        let done = ItemState(4);
        assert!(done.installed() && done.ready() && !done.subscribed());
        let kept = ItemState(5);
        assert!(kept.subscribed() && kept.ready());
        let outdated = ItemState(13);
        assert!(outdated.installed() && !outdated.ready() && outdated.fetching());
    }

    #[test]
    fn result_codes_have_words() {
        assert_eq!(eresult_text(9), "no such Workshop item");
        assert_eq!(eresult_text(1234), "result code 1234");
    }

    #[test]
    fn call_results_are_packed_the_way_valve_packs_callbacks() {
        let expected = if cfg!(any(target_os = "linux", target_os = "macos")) {
            12
        } else {
            16
        };
        assert_eq!(std::mem::size_of::<FileResult>(), expected);
    }

    #[test]
    fn scrub_removes_the_announcement() {
        let mut cmd = std::process::Command::new("true");
        cmd.env("SteamAppId", "431960").env("SteamGameId", "431960");
        scrub(&mut cmd);
        let removed: Vec<_> = cmd
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        assert_eq!(removed, ["SteamAppId", "SteamGameId"]);
    }

    #[test]
    fn without_steam_there_is_no_library() {
        assert!(matches!(
            Lib::find(&SteamInfo::default()),
            Err(Error::Unsupported(m)) if m == STEAM_NOT_FOUND
        ));
        assert!(library_candidates(&SteamInfo::default()).is_empty());
    }

    /// A Steam folder holding one library with one installed game that bundles the library
    /// at `game_relative`, plus whatever `extra` files; returns the folder.
    fn steam_with_game(game_relative: &str, extra: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        let game = root.path().join("steamapps").join("common").join("Game");
        for rel in std::iter::once(game_relative).chain(extra.iter().copied()) {
            let path = if rel.starts_with("steamapps") || rel.starts_with("steamrt64") {
                root.path().join(rel)
            } else {
                game.join(rel)
            };
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"not a real library").unwrap();
        }
        root
    }

    fn info_for(root: &Path) -> SteamInfo {
        SteamInfo {
            steam_dir: Some(root.to_path_buf()),
            libraries: vec![root.to_path_buf()],
            ..SteamInfo::default()
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn steams_own_copy_comes_before_the_games() {
        let root = steam_with_game("bin/libsteam_api.so", &["steamrt64/libsteam_api.so"]);
        let found = library_candidates(&info_for(root.path()));
        assert_eq!(
            found,
            [
                root.path().join("steamrt64/libsteam_api.so"),
                root.path()
                    .join("steamapps/common/Game/bin/libsteam_api.so"),
            ]
        );
        let err = Lib::find(&info_for(root.path()))
            .err()
            .expect("no library loads")
            .to_string();
        assert!(err.contains("none of the 2 copies"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn wallpaper_engines_copy_comes_before_the_games() {
        let root = steam_with_game(
            "steam_api64.dll",
            &["steamapps/common/wallpaper_engine/distribution/bin/steam_api64.dll"],
        );
        let mut info = info_for(root.path());
        info.install_dir = Some(root.path().join("steamapps/common/wallpaper_engine"));
        let found = library_candidates(&info);
        assert_eq!(
            found,
            [
                root.path()
                    .join("steamapps/common/wallpaper_engine/distribution/bin/steam_api64.dll"),
                root.path().join("steamapps/common/Game/steam_api64.dll"),
            ]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_games_bundle_the_library_in_their_app() {
        let root = steam_with_game("Game.app/Contents/MacOS/libsteam_api.dylib", &[]);
        let found = library_candidates(&info_for(root.path()));
        assert_eq!(
            found,
            [root
                .path()
                .join("steamapps/common/Game/Game.app/Contents/MacOS/libsteam_api.dylib")]
        );
    }

    #[test]
    fn a_steam_without_libraries_says_so() {
        let root = tempfile::tempdir().unwrap();
        let err = Lib::find(&info_for(root.path()))
            .err()
            .expect("no library loads")
            .to_string();
        assert!(err.starts_with("No "), "{err}");
    }
}
