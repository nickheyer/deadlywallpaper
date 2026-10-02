mod audio;
mod autostart;
mod capture;
mod content;
mod daemon;
mod engine;
mod error;
mod geom;
mod http;
mod ipc;
mod logger;
mod media;
mod model;
mod msg;
mod nowplaying;
mod paths;
mod platform;
mod tray;
mod ui;
mod we;
mod web;

use clap::{Parser, Subcommand};
use error::{Error, Result};
use ipc::{Request, Response, client};
use model::{Arrangement, Pose};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "deadlywp",
    version,
    about = "Deadly Wallpaper: live wallpapers for every desktop"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the wallpaper daemon in the foreground
    Daemon,
    /// Open the control window, starting the daemon when needed
    Ui,
    /// Show daemon state
    Status,
    /// List library wallpapers
    List,
    /// List connected displays
    Displays,
    /// Apply a wallpaper: library id, file, folder, URL, `random`, or `reload`
    Set {
        target: String,
        /// Display id or 1-based index; defaults to the primary display
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Close the wallpaper on one display, or all wallpapers
    Close {
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Change how wallpapers map onto displays
    Layout { arrangement: Arrangement },
    /// Move, scale or rotate the spanning image or one display; --reset clears everything
    Align {
        /// `image`, a display id, or a 1-based display index
        target: Option<String>,
        /// Shift of the centre in pixels
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        x: f64,
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        y: f64,
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// Degrees clockwise
        #[arg(long, default_value_t = 0.0, allow_hyphen_values = true)]
        rotate: f64,
        #[arg(long)]
        reset: bool,
    },
    /// Set the volume 0-100, or adjust it with +n / -n
    Volume { value: String },
    /// Resume playback
    Play,
    /// Pause every wallpaper until `play`
    Pause,
    /// Seek to a percentage, or by +n / -n
    Seek {
        value: String,
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Change a running wallpaper's property: name=value (++n / --n for relative)
    Prop {
        assignment: String,
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Save a screenshot of a running wallpaper (.png or .jpg)
    Screenshot {
        file: PathBuf,
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Add a file, a folder of wallpapers, a Lively package, or a URL to the library
    Import { source: String },
    /// Export a wallpaper as a Lively package (.zip)
    Export { wallpaper: String, file: PathBuf },
    /// Remove a wallpaper from the library
    Delete { wallpaper: String },
    /// Browse the Steam Workshop for Wallpaper Engine and fetch wallpapers from it
    Workshop {
        #[command(subcommand)]
        command: WorkshopCommand,
    },
    /// Stop the daemon
    Quit,
}

#[derive(Subcommand)]
enum WorkshopCommand {
    /// Search the Workshop; no words lists what is trending
    Search {
        words: Vec<String>,
        /// trend, recent, updated, subscribers or rated
        #[arg(short, long, default_value = "trend")]
        sort: String,
        /// scene, video, web or application
        #[arg(short = 't', long = "type")]
        kind: Option<String>,
        /// Days the trend sort ranks over: 1, 7, 30, 90, 180 or 365
        #[arg(long, default_value_t = 7)]
        days: u32,
        /// Age ratings to show, any of everyone, questionable or mature; repeat for several
        #[arg(short, long, default_value = "everyone")]
        rating: Vec<String>,
        /// Screen sizes to show, as Steam names them ("1920 x 1080", "Ultrawide 3440 x 1440",
        /// "Portrait 1080 x 1920", ...) or 720p, 1080p, 1440p, 4k; repeat for several
        #[arg(long)]
        size: Vec<String>,
        #[arg(short, long, default_value_t = 1)]
        page: u32,
    },
    /// Show one item: an id or its Steam page URL
    Show { item: String },
    /// Have Steam download an item and add it to the library; --display applies it there
    Get {
        item: String,
        #[arg(short, long)]
        display: Option<String>,
    },
    /// Download every subscription Steam has not fetched, add every download and refresh the
    /// ones Steam updated
    Sync,
    /// Steam, Wallpaper Engine, downloads under way and every downloaded item
    Status,
    /// Stop an item's download
    Cancel { item: String },
}

fn main() {
    let cli = Cli::parse();
    let code = match run(cli.command) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };
    std::process::exit(code);
}

fn run(command: Option<Command>) -> Result<()> {
    match command {
        Some(Command::Daemon) => daemon::run(),
        None | Some(Command::Ui) => ui::run(),
        Some(cmd) => client_command(cmd),
    }
}

/// Start a detached daemon unless one already answers.
pub fn ensure_daemon() -> Result<()> {
    if client::is_running() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("daemon")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0008 | 0x0800_0000);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()?;
    client::Client::connect_within(Duration::from_secs(15)).map(|_| ())
}

fn client_command(cmd: Command) -> Result<()> {
    let req = match cmd {
        Command::Status => Request::Status,
        Command::List => Request::Library,
        Command::Displays => Request::Displays,
        Command::Set { target, display } => Request::Set {
            target: absolutize(target),
            display,
        },
        Command::Close { display } => Request::Close { display },
        Command::Layout { arrangement } => Request::SetArrangement {
            arrangement,
            display: None,
        },
        Command::Align {
            target,
            x,
            y,
            scale,
            rotate,
            reset,
        } => {
            let pose = Pose {
                x,
                y,
                scale,
                rotation: rotate,
            };
            match (reset, target.as_deref()) {
                (true, _) => Request::ResetAlignment,
                (false, Some("image")) => Request::AlignImage { pose },
                (false, Some(display)) => Request::AlignDisplay {
                    display: display.to_string(),
                    pose,
                },
                (false, None) => {
                    return Err(Error::Invalid(
                        "expected `image`, a display, or --reset".into(),
                    ));
                }
            }
        }
        Command::Volume { value } => Request::Volume { value },
        Command::Play => Request::Play { play: true },
        Command::Pause => Request::Play { play: false },
        Command::Seek { value, display } => Request::Seek { display, value },
        Command::Prop {
            assignment,
            display,
        } => {
            let (name, value) = assignment
                .split_once('=')
                .ok_or_else(|| Error::Invalid("expected name=value".into()))?;
            Request::SetProperty {
                wallpaper: String::new(),
                display,
                name: name.trim().into(),
                value: serde_json::Value::String(value.to_string()),
            }
        }
        Command::Screenshot { file, display } => Request::Screenshot {
            display,
            file: std::path::absolute(file)?,
        },
        Command::Import { source } => Request::Import {
            source: absolutize(source),
        },
        Command::Export { wallpaper, file } => Request::Export {
            wallpaper,
            file: std::path::absolute(file)?,
        },
        Command::Delete { wallpaper } => Request::Delete { wallpaper },
        Command::Workshop { command } => return workshop_command(command),
        Command::Quit => Request::Quit,
        Command::Daemon | Command::Ui => unreachable!("handled by run"),
    };
    print_response(client::call(&req)?);
    Ok(())
}

fn workshop_item_ref(item: &str) -> Result<u64> {
    we::workshop::parse_ref(item)
        .ok_or_else(|| Error::Invalid(format!("'{item}' is not a workshop item id or URL")))
}

fn workshop_command(cmd: WorkshopCommand) -> Result<()> {
    use we::workshop::{Client, Query, Sort};
    let req = match cmd {
        WorkshopCommand::Search {
            words,
            sort,
            kind,
            days,
            rating,
            size,
            page,
        } => {
            let query = Query {
                text: words.join(" "),
                sort: Sort::parse(&sort)
                    .ok_or_else(|| Error::Invalid(format!("'{sort}' is not a sort order")))?,
                days,
                kind: kind
                    .map(|k| {
                        we::project::ProjectType::parse(&k)
                            .ok_or_else(|| Error::Invalid(format!("'{k}' is not a wallpaper type")))
                    })
                    .transpose()?,
                tags: Vec::new(),
                ratings: rating
                    .iter()
                    .map(|r| {
                        we::workshop::Rating::parse(r)
                            .ok_or_else(|| Error::Invalid(format!("'{r}' is not an age rating")))
                    })
                    .collect::<Result<Vec<_>>>()?,
                sizes: size
                    .iter()
                    .map(|s| {
                        we::workshop::size_tag(s)
                            .map(str::to_string)
                            .ok_or_else(|| {
                                Error::Invalid(format!("'{s}' is not a Workshop screen size"))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?,
                page,
            };
            let client = Client::new(&paths::Paths::discover()?.cache_dir);
            let found = client.browse(&query)?;
            println!(
                "page {} of {} ({} items)",
                found.page, found.pages, found.total
            );
            for item in &found.items {
                print_workshop_item(item, false);
            }
            return Ok(());
        }
        WorkshopCommand::Show { item } => {
            let id = workshop_item_ref(&item)?;
            let client = Client::new(&paths::Paths::discover()?.cache_dir);
            print_workshop_item(&client.item(id)?, true);
            return Ok(());
        }
        WorkshopCommand::Get { item, display } => Request::WorkshopGet {
            id: workshop_item_ref(&item)?,
            title: None,
            author: None,
            display,
        },
        WorkshopCommand::Sync => Request::WorkshopSync,
        WorkshopCommand::Status => Request::WorkshopStatus,
        WorkshopCommand::Cancel { item } => Request::WorkshopCancel {
            id: workshop_item_ref(&item)?,
        },
    };
    print_response(client::call(&req)?);
    Ok(())
}

fn print_workshop_item(item: &we::workshop::Item, full: bool) {
    let kind = item.kind().map(|k| k.name()).unwrap_or("?");
    let rating = item.rating().map(|r| r.tag()).unwrap_or("-");
    println!(
        "{}\t{}\t{}\t{} subs\t{}\t{}",
        item.id,
        kind,
        rating,
        item.subscriptions,
        item.title,
        item.author.as_deref().unwrap_or("")
    );
    if full {
        println!("url: {}", item.url());
        if let Some(p) = &item.preview_url {
            println!("preview: {p}");
        }
        println!("tags: {}", item.tags.join(", "));
        println!(
            "size: {:.1} MB  updated: {}  favorites: {}  views: {}  stars: {}",
            item.size as f64 / 1_048_576.0,
            item.updated,
            item.favorites,
            item.views,
            item.stars
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into())
        );
        if !item.description.trim().is_empty() {
            println!("\n{}", item.description.trim());
        }
    }
}

/// Existing filesystem paths are sent absolute so the daemon resolves them correctly.
fn absolutize(target: String) -> String {
    let p = std::path::Path::new(&target);
    if p.exists() {
        std::path::absolute(p)
            .map(|a| a.to_string_lossy().into_owned())
            .unwrap_or(target)
    } else {
        target
    }
}

fn print_response(resp: Response) {
    match resp {
        Response::Ok => {}
        Response::Text(t) => println!("{t}"),
        Response::Status(s) => {
            println!(
                "deadlywp {} on {} ({}), presenter: {}, window monitor: {}",
                s.version, s.platform, s.session, s.capabilities.presenter, s.window_monitor
            );
            println!(
                "arrangement: {}  paused: {}  locked: {}  battery: {}",
                s.layout.arrangement.label(),
                s.paused,
                s.locked,
                s.on_battery
            );
            for (i, d) in s.displays.iter().enumerate() {
                let active = s.active.iter().find(|a| a.display == d.id);
                let state = match active {
                    Some(a) => format!(
                        "{} [{}]{}",
                        a.title,
                        a.wallpaper,
                        if a.paused { " paused" } else { "" }
                    ),
                    None => "-".into(),
                };
                println!(
                    "{}. {} {}x{}+{}+{}{}: {}",
                    i + 1,
                    d.name,
                    d.rect.w,
                    d.rect.h,
                    d.rect.x,
                    d.rect.y,
                    if d.primary { " primary" } else { "" },
                    state
                );
            }
        }
        Response::Displays(ds) => {
            for (i, d) in ds.iter().enumerate() {
                println!(
                    "{}\t{}\t{}\t{}x{}+{}+{}\tscale {:.2}{}",
                    i + 1,
                    d.id,
                    d.name,
                    d.rect.w,
                    d.rect.h,
                    d.rect.x,
                    d.rect.y,
                    d.scale,
                    if d.primary { "\tprimary" } else { "" }
                );
            }
        }
        Response::Library(items) | Response::Wallpapers(items) => {
            for w in items {
                println!("{}\t{}\t{}\t{}", w.id, w.kind, w.title, w.source);
            }
        }
        Response::Wallpaper(w) => println!("{}\t{}\t{}\t{}", w.id, w.kind, w.title, w.source),
        Response::Workshop(ws) => {
            let shown = |p: &Option<PathBuf>| {
                p.as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "not found".into())
            };
            println!("steam: {}", shown(&ws.steam.steam_dir));
            println!(
                "steam client: {}",
                match &ws.client {
                    ipc::SteamClient::Running => match &ws.account {
                        Some(a) => format!("running, signed in as {a}"),
                        None => "running".into(),
                    },
                    ipc::SteamClient::NotRunning => "not running".into(),
                    ipc::SteamClient::Unavailable { reason } => reason.clone(),
                }
            );
            println!("wallpaper engine: {}", shown(&ws.steam.install_dir));
            println!(
                "assets: {}{}",
                shown(&ws.steam.assets_dir),
                if ws.steam.assets_overridden {
                    " (from settings)"
                } else {
                    ""
                }
            );
            for d in &ws.downloads {
                let progress = match d.fraction() {
                    Some(f) => format!("{:.0}%", f * 100.0),
                    None => String::new(),
                };
                println!(
                    "{}\t{}\t{}\t{}",
                    d.id,
                    d.phase.label().to_lowercase(),
                    d.name(),
                    progress
                );
            }
            for item in &ws.items {
                let state = match (&item.wallpaper, item.stale) {
                    (Some(_), true) => "update available",
                    (Some(_), false) => "in library",
                    (None, _) => "downloaded",
                };
                println!(
                    "{}\t{}\t{}\t{}",
                    item.id,
                    state,
                    item.title,
                    item.wallpaper.as_deref().unwrap_or("")
                );
            }
        }
        other @ (Response::Settings(_)
        | Response::Layout(_)
        | Response::Controls { .. }
        | Response::Devices(_)) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&other).unwrap_or_default()
            );
        }
        Response::Error { message, .. } => eprintln!("error: {message}"),
    }
}
