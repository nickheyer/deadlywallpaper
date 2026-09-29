mod audio;
mod autostart;
mod capture;
mod content;
mod daemon;
mod engine;
mod error;
mod geom;
mod ipc;
mod logger;
mod media;
mod model;
mod msg;
mod paths;
mod platform;
mod scheme;
mod tray;
mod ui;
mod web;

use clap::{Parser, Subcommand};
use error::{Error, Result};
use ipc::{Request, Response, client};
use model::{Arrangement, Pose};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "deadlywp", version, about = "Deadly Wallpaper: live wallpapers for every desktop")]
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
    /// Stop the daemon
    Quit,
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
    cmd.arg("daemon").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
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
        Command::Set { target, display } => Request::Set { target: absolutize(target), display },
        Command::Close { display } => Request::Close { display },
        Command::Layout { arrangement } => Request::SetArrangement { arrangement, display: None },
        Command::Align { target, x, y, scale, rotate, reset } => {
            let pose = Pose { x, y, scale, rotation: rotate };
            match (reset, target.as_deref()) {
                (true, _) => Request::ResetAlignment,
                (false, Some("image")) => Request::AlignImage { pose },
                (false, Some(display)) => Request::AlignDisplay { display: display.to_string(), pose },
                (false, None) => return Err(Error::Invalid("expected `image`, a display, or --reset".into())),
            }
        }
        Command::Volume { value } => Request::Volume { value },
        Command::Play => Request::Play { play: true },
        Command::Pause => Request::Play { play: false },
        Command::Seek { value, display } => Request::Seek { display, value },
        Command::Prop { assignment, display } => {
            let (name, value) = assignment
                .split_once('=')
                .ok_or_else(|| Error::Invalid("expected name=value".into()))?;
            Request::SetProperty { wallpaper: String::new(), display, name: name.trim().into(), value: serde_json::Value::String(value.to_string()) }
        }
        Command::Screenshot { file, display } => Request::Screenshot { display, file: std::path::absolute(file)? },
        Command::Import { source } => Request::Import { source: absolutize(source) },
        Command::Export { wallpaper, file } => Request::Export { wallpaper, file: std::path::absolute(file)? },
        Command::Delete { wallpaper } => Request::Delete { wallpaper },
        Command::Quit => Request::Quit,
        Command::Daemon | Command::Ui => unreachable!("handled by run"),
    };
    print_response(client::call(&req)?);
    Ok(())
}

/// Existing filesystem paths are sent absolute so the daemon resolves them correctly.
fn absolutize(target: String) -> String {
    let p = std::path::Path::new(&target);
    if p.exists() {
        std::path::absolute(p).map(|a| a.to_string_lossy().into_owned()).unwrap_or(target)
    } else {
        target
    }
}

fn print_response(resp: Response) {
    match resp {
        Response::Ok => {}
        Response::Text(t) => println!("{t}"),
        Response::Status(s) => {
            println!("deadlywp {} on {} ({}), presenter: {}, window monitor: {}", s.version, s.platform, s.session, s.capabilities.presenter, s.window_monitor);
            println!("arrangement: {}  paused: {}  locked: {}  battery: {}", s.layout.arrangement.label(), s.paused, s.locked, s.on_battery);
            for (i, d) in s.displays.iter().enumerate() {
                let active = s.active.iter().find(|a| a.display == d.id);
                let state = match active {
                    Some(a) => format!("{} [{}]{}", a.title, a.wallpaper, if a.paused { " paused" } else { "" }),
                    None => "-".into(),
                };
                println!("{}. {} {}x{}+{}+{}{}: {}", i + 1, d.name, d.rect.w, d.rect.h, d.rect.x, d.rect.y, if d.primary { " primary" } else { "" }, state);
            }
        }
        Response::Displays(ds) => {
            for (i, d) in ds.iter().enumerate() {
                println!("{}\t{}\t{}\t{}x{}+{}+{}\tscale {:.2}{}", i + 1, d.id, d.name, d.rect.w, d.rect.h, d.rect.x, d.rect.y, d.scale, if d.primary { "\tprimary" } else { "" });
            }
        }
        Response::Library(items) | Response::Wallpapers(items) => {
            for w in items {
                println!("{}\t{}\t{}\t{}", w.id, w.kind, w.title, w.source);
            }
        }
        Response::Wallpaper(w) => println!("{}\t{}\t{}\t{}", w.id, w.kind, w.title, w.source),
        other @ (Response::Settings(_) | Response::Layout(_) | Response::Controls { .. } | Response::Devices(_)) => {
            println!("{}", serde_json::to_string_pretty(&other).unwrap_or_default());
        }
        Response::Error { message, .. } => eprintln!("error: {message}"),
    }
}
