//! Sorting and grouping of library wallpapers: every key orders the flat list and cuts it
//! into labelled groups the same way, so the grouped and flat views always agree.

use crate::model::{Kind, Summary};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use std::path::Path;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Title,
    Kind,
    Format,
    Folder,
    Author,
    Size,
    Added,
    Modified,
}

impl Key {
    pub const ALL: [Key; 8] = [
        Key::Title,
        Key::Kind,
        Key::Format,
        Key::Folder,
        Key::Author,
        Key::Size,
        Key::Added,
        Key::Modified,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Key::Title => "Title",
            Key::Kind => "Type",
            Key::Format => "Format",
            Key::Folder => "Source folder",
            Key::Author => "Author",
            Key::Size => "Size",
            Key::Added => "Date added",
            Key::Modified => "Date modified",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Key::Title => "title",
            Key::Kind => "type",
            Key::Format => "format",
            Key::Folder => "folder",
            Key::Author => "author",
            Key::Size => "size",
            Key::Added => "added",
            Key::Modified => "modified",
        }
    }

    pub fn from_name(name: &str) -> Option<Key> {
        Key::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Direction the key reads best in: biggest and newest first.
    pub fn descends_by_default(self) -> bool {
        matches!(self, Key::Size | Key::Added | Key::Modified)
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Layout {
    Grid,
    List,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Grid => "grid",
            Layout::List => "list",
        }
    }

    pub fn from_name(name: &str) -> Option<Layout> {
        [Layout::Grid, Layout::List]
            .into_iter()
            .find(|l| l.name() == name)
    }
}

/// How the library page arranges its wallpapers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub key: Key,
    pub descending: bool,
    pub grouped: bool,
    pub layout: Layout,
}

impl Default for Order {
    fn default() -> Self {
        Order {
            key: Key::Title,
            descending: false,
            grouped: false,
            layout: Layout::Grid,
        }
    }
}

impl Order {
    /// Switch keys, taking the new key's natural direction.
    pub fn set_key(&mut self, key: Key) {
        if self.key != key {
            self.key = key;
            self.descending = key.descends_by_default();
        }
    }
}

pub struct Group<'a> {
    /// Empty for the single group of a flat arrangement.
    pub label: String,
    pub items: Vec<&'a Summary>,
}

/// Today's local date, fixed once per frame so every date bucket agrees.
#[derive(Clone, Copy, Debug)]
pub struct Today(NaiveDate);

impl Today {
    pub fn now() -> Today {
        Today(Local::now().date_naive())
    }
}

struct Slot<'a> {
    item: &'a Summary,
    /// Items without a value for the key stay last in either direction.
    present: bool,
    /// Equal buckets share a group; the bucket orders groups among themselves.
    bucket: (i64, String),
    label: String,
    /// Order inside a bucket, before the title.
    number: i64,
    /// Lower-case title, the final tie-breaker before the id.
    title: String,
}

impl Slot<'_> {
    /// Mark the item as having no value for the key. Absent buckets sit in their own rank
    /// range so they never merge with a present group; online content lists before content
    /// that could not be read.
    fn absent(&mut self, online: bool, label: &str) {
        self.present = false;
        self.bucket = (i64::MIN + if online { 1 } else { 2 }, String::new());
        self.label = label.into();
    }

    fn rank(&self) -> (&(i64, String), i64, &str, &str) {
        (&self.bucket, self.number, &self.title, &self.item.id)
    }
}

/// Sort `items` by `order`, cutting them into groups when grouped; a flat arrangement is a
/// single unlabelled group.
pub fn arrange<'a>(items: &[&'a Summary], order: &Order, today: Today) -> Vec<Group<'a>> {
    let (mut present, mut absent): (Vec<Slot<'a>>, Vec<Slot<'a>>) = items
        .iter()
        .map(|w| slot(w, order.key, today))
        .partition(|s| s.present);
    present.sort_by(|a, b| a.rank().cmp(&b.rank()));
    absent.sort_by(|a, b| a.rank().cmp(&b.rank()));
    if order.descending {
        present.reverse();
    }
    present.append(&mut absent);
    if !order.grouped {
        return vec![Group {
            label: String::new(),
            items: present.into_iter().map(|s| s.item).collect(),
        }];
    }
    let mut groups: Vec<(&(i64, String), Group<'a>)> = Vec::new();
    for s in &present {
        match groups.last_mut() {
            Some((bucket, group)) if *bucket == &s.bucket => group.items.push(s.item),
            _ => groups.push((
                &s.bucket,
                Group {
                    label: s.label.clone(),
                    items: vec![s.item],
                },
            )),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

fn slot<'a>(w: &'a Summary, key: Key, today: Today) -> Slot<'a> {
    let mut s = Slot {
        item: w,
        present: true,
        bucket: (0, String::new()),
        label: String::new(),
        number: 0,
        title: w.title.to_lowercase(),
    };
    match key {
        Key::Title => {
            let letter = w
                .title
                .trim()
                .chars()
                .next()
                .filter(|c| c.is_alphabetic())
                .map(|c| c.to_uppercase().collect::<String>());
            s.bucket = match &letter {
                Some(l) => (1, l.clone()),
                None => (0, String::new()),
            };
            s.label = letter.unwrap_or_else(|| "#".into());
        }
        Key::Kind => {
            s.bucket = (kind_rank(w.kind), String::new());
            s.label = w.kind.label().to_string();
        }
        Key::Format => {
            let f = format_label(w);
            s.bucket = (i64::from(w.kind.is_online()), f.clone());
            s.label = f;
        }
        Key::Folder => match folder_label(w) {
            Folder::Path(p) => {
                s.bucket = (0, p.clone());
                s.label = p;
            }
            Folder::Library => {
                s.bucket = (1, String::new());
                s.label = "Library".into();
            }
            Folder::Online => s.absent(true, "Online"),
        },
        Key::Author => match w.author.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            Some(a) => {
                s.bucket = (0, a.to_lowercase());
                s.label = a.to_string();
            }
            None => s.absent(false, "Unknown author"),
        },
        Key::Size => match w.size {
            Some(bytes) => {
                let (rank, label) = size_bucket(bytes);
                s.bucket = (rank, String::new());
                s.label = label.into();
                s.number = i64::try_from(bytes).unwrap_or(i64::MAX);
            }
            None => s.absent(w.kind.is_online(), unavailable(w)),
        },
        Key::Added | Key::Modified => {
            let stamp = if key == Key::Added {
                w.added
            } else {
                w.modified
            };
            match stamp {
                Some(secs) => {
                    let (rank, label) = date_bucket(today, secs);
                    s.bucket = (rank, String::new());
                    s.label = label;
                    s.number = i64::try_from(secs).unwrap_or(i64::MAX);
                }
                None if key == Key::Added => s.absent(false, "Unknown date"),
                None => s.absent(w.kind.is_online(), unavailable(w)),
            }
        }
    }
    s
}

fn kind_rank(kind: Kind) -> i64 {
    match kind {
        Kind::Video => 0,
        Kind::Gif => 1,
        Kind::Picture => 2,
        Kind::VideoStream => 3,
        Kind::Web => 4,
        Kind::WebAudio => 5,
        Kind::Url => 6,
        Kind::Program => 7,
        Kind::Scene => 8,
    }
}

/// Why a wallpaper has no size or modification time.
fn unavailable(w: &Summary) -> &'static str {
    if w.kind.is_online() {
        "Online"
    } else {
        "Unavailable"
    }
}

/// The content's file extension in capitals; online kinds and files without an extension are
/// named by kind.
pub fn format_label(w: &Summary) -> String {
    if w.kind.is_online() {
        return w.kind.label().to_string();
    }
    match Path::new(&w.source)
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
    {
        Some(ext) => ext.to_ascii_uppercase(),
        None => format!("{} without extension", w.kind.label()),
    }
}

pub enum Folder {
    Path(String),
    Library,
    Online,
}

pub fn folder_label(w: &Summary) -> Folder {
    if w.kind.is_online() {
        Folder::Online
    } else {
        match &w.folder {
            Some(p) => Folder::Path(p.to_string_lossy().into_owned()),
            None => Folder::Library,
        }
    }
}

const KB: u64 = 1024;
const MB: u64 = KB * 1024;
const GB: u64 = MB * 1024;

fn size_bucket(bytes: u64) -> (i64, &'static str) {
    match bytes {
        b if b < MB => (0, "Under 1 MB"),
        b if b < 10 * MB => (1, "1 – 10 MB"),
        b if b < 100 * MB => (2, "10 – 100 MB"),
        b if b < GB => (3, "100 MB – 1 GB"),
        _ => (4, "Over 1 GB"),
    }
}

pub fn size_text(bytes: u64) -> String {
    if bytes < KB {
        format!("{bytes} B")
    } else if bytes < MB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    }
}

fn local_date(secs: u64) -> Option<NaiveDate> {
    Local
        .timestamp_opt(i64::try_from(secs).ok()?, 0)
        .single()
        .map(|dt| dt.date_naive())
}

/// Recent days get their own buckets; everything older is bucketed by calendar month. Ranks
/// grow toward the present so ascending order runs oldest to newest.
fn date_bucket(today: Today, secs: u64) -> (i64, String) {
    let Some(date) = local_date(secs) else {
        return (i64::MIN, "Unknown date".into());
    };
    match (today.0 - date).num_days() {
        d if d <= 0 => (i64::MAX, "Today".into()),
        1 => (i64::MAX - 1, "Yesterday".into()),
        2..=6 => (i64::MAX - 2, "Last 7 days".into()),
        7..=29 => (i64::MAX - 3, "Last 30 days".into()),
        _ => (
            i64::from(date.year()) * 12 + i64::from(date.month0()),
            date.format("%B %Y").to_string(),
        ),
    }
}

pub fn date_text(secs: u64) -> String {
    match local_date(secs) {
        Some(date) => date.format("%-d %b %Y").to_string(),
        None => "Unknown date".into(),
    }
}

/// The value a card or row shows for the key it is sorted by; the author for keys whose value
/// is already visible.
pub fn detail(w: &Summary, key: Key) -> Option<String> {
    match key {
        Key::Title | Key::Kind | Key::Author => w.author.clone(),
        Key::Format => Some(format_label(w)),
        Key::Folder => Some(match folder_label(w) {
            Folder::Path(p) => p,
            Folder::Library => "Library".into(),
            Folder::Online => "Online".into(),
        }),
        Key::Size => Some(w.size.map_or_else(|| unavailable(w).into(), size_text)),
        Key::Added => Some(match w.added {
            Some(t) => format!("Added {}", date_text(t)),
            None => "Unknown date".into(),
        }),
        Key::Modified => Some(match w.modified {
            Some(t) => format!("Modified {}", date_text(t)),
            None => unavailable(w).into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn wp(id: &str, title: &str, kind: Kind) -> Summary {
        Summary {
            id: id.into(),
            title: title.into(),
            kind,
            author: None,
            desc: None,
            contact: None,
            license: None,
            arguments: None,
            thumbnail: None,
            customizable: false,
            source: format!("/media/{id}.mp4"),
            dir: PathBuf::from(format!("/lib/{id}")),
            absolute: true,
            size: None,
            added: None,
            modified: None,
            folder: Some(PathBuf::from("/media")),
            workshop: None,
            tags: Vec::new(),
        }
    }

    fn today() -> Today {
        Today(NaiveDate::from_ymd_opt(2026, 9, 29).unwrap())
    }

    fn secs_of(date: NaiveDate) -> u64 {
        Local
            .from_local_datetime(&date.and_hms_opt(12, 0, 0).unwrap())
            .single()
            .unwrap()
            .timestamp() as u64
    }

    fn labels(groups: &[Group<'_>]) -> Vec<(String, Vec<String>)> {
        groups
            .iter()
            .map(|g| {
                (
                    g.label.clone(),
                    g.items.iter().map(|w| w.id.clone()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn titles_group_by_letter_with_symbols_first() {
        let items = [
            wp("b", "beta", Kind::Video),
            wp("n", "9 lives", Kind::Video),
            wp("a", "Alpha", Kind::Video),
            wp("a2", "apex", Kind::Video),
            wp("t", "~tilde", Kind::Video),
        ];
        let refs: Vec<&Summary> = items.iter().collect();
        let order = Order {
            grouped: true,
            ..Order::default()
        };
        let groups = arrange(&refs, &order, today());
        assert_eq!(
            labels(&groups),
            vec![
                ("#".to_string(), vec!["n".to_string(), "t".to_string()]),
                ("A".to_string(), vec!["a".to_string(), "a2".to_string()]),
                ("B".to_string(), vec!["b".to_string()]),
            ]
        );
        let flat = arrange(
            &refs,
            &Order {
                descending: true,
                ..Order::default()
            },
            today(),
        );
        assert_eq!(flat.len(), 1);
        assert_eq!(flat[0].label, "");
        let ids: Vec<&str> = flat[0].items.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["b", "a2", "a", "t", "n"]);
    }

    #[test]
    fn sizes_bucket_and_keep_unavailable_last_in_both_directions() {
        let mut small = wp("s", "small", Kind::Picture);
        small.size = Some(300 * KB);
        let mut mid = wp("m", "mid", Kind::Video);
        mid.size = Some(42 * MB);
        let mut big = wp("b", "big", Kind::Video);
        big.size = Some(3 * GB);
        let online = wp("o", "online", Kind::Url);
        let mut gone = wp("g", "gone", Kind::Video);
        gone.size = None;
        let items = [online, mid, gone, big, small];
        let refs: Vec<&Summary> = items.iter().collect();
        let mut order = Order::default();
        order.set_key(Key::Size);
        assert!(order.descending);
        order.grouped = true;
        let groups = arrange(&refs, &order, today());
        assert_eq!(
            labels(&groups),
            vec![
                ("Over 1 GB".to_string(), vec!["b".to_string()]),
                ("10 – 100 MB".to_string(), vec!["m".to_string()]),
                ("Under 1 MB".to_string(), vec!["s".to_string()]),
                ("Online".to_string(), vec!["o".to_string()]),
                ("Unavailable".to_string(), vec!["g".to_string()]),
            ]
        );
        order.descending = false;
        let groups = arrange(&refs, &order, today());
        let heads: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(
            heads,
            [
                "Under 1 MB",
                "10 – 100 MB",
                "Over 1 GB",
                "Online",
                "Unavailable"
            ]
        );
        assert_eq!(size_text(999), "999 B");
        assert_eq!(size_text(300 * KB), "300 KB");
        assert_eq!(size_text(42 * MB + MB / 2), "42.5 MB");
        assert_eq!(size_text(3 * GB), "3.00 GB");
    }

    #[test]
    fn dates_bucket_relative_to_today_then_by_month() {
        let t = today();
        let day = |y, m, d| secs_of(NaiveDate::from_ymd_opt(y, m, d).unwrap());
        let mut items = [
            wp("today", "a", Kind::Video),
            wp("yesterday", "b", Kind::Video),
            wp("week", "c", Kind::Video),
            wp("month", "d", Kind::Video),
            wp("aug", "e", Kind::Video),
            wp("jan", "f", Kind::Video),
            wp("none", "g", Kind::Video),
        ];
        items[0].added = Some(day(2026, 9, 29));
        items[1].added = Some(day(2026, 9, 28));
        items[2].added = Some(day(2026, 9, 24));
        items[3].added = Some(day(2026, 9, 5));
        items[4].added = Some(day(2026, 8, 20));
        items[5].added = Some(day(2026, 1, 3));
        let refs: Vec<&Summary> = items.iter().collect();
        let mut order = Order {
            grouped: true,
            ..Order::default()
        };
        order.set_key(Key::Added);
        let groups = arrange(&refs, &order, t);
        let heads: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(
            heads,
            [
                "Today",
                "Yesterday",
                "Last 7 days",
                "Last 30 days",
                "August 2026",
                "January 2026",
                "Unknown date"
            ]
        );
        order.descending = false;
        let groups = arrange(&refs, &order, t);
        let heads: Vec<&str> = groups.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(heads[0], "January 2026");
        assert_eq!(heads[5], "Today");
        assert_eq!(heads[6], "Unknown date");
        assert_eq!(date_text(day(2026, 8, 5)), "5 Aug 2026");
    }

    #[test]
    fn folders_order_paths_then_library_then_online() {
        let mut copied = wp("c", "copied", Kind::Video);
        copied.absolute = false;
        copied.folder = None;
        let online = wp("o", "online", Kind::VideoStream);
        let mut zoo = wp("z", "zoo", Kind::Picture);
        zoo.folder = Some(PathBuf::from("/pictures/zoo"));
        let media = wp("m", "media", Kind::Video);
        let items = [online, copied, zoo, media];
        let refs: Vec<&Summary> = items.iter().collect();
        let order = Order {
            key: Key::Folder,
            grouped: true,
            ..Order::default()
        };
        let groups = arrange(&refs, &order, today());
        assert_eq!(
            labels(&groups),
            vec![
                ("/media".to_string(), vec!["m".to_string()]),
                ("/pictures/zoo".to_string(), vec!["z".to_string()]),
                ("Library".to_string(), vec!["c".to_string()]),
                ("Online".to_string(), vec!["o".to_string()]),
            ]
        );
    }

    #[test]
    fn formats_and_kinds_and_authors() {
        let mut png = wp("p", "pic", Kind::Picture);
        png.source = "/x/a.PNG".into();
        let mut site = wp("s", "site", Kind::Url);
        site.source = "https://example.org".into();
        let mut bin = wp("b", "tool", Kind::Program);
        bin.source = "/x/tool".into();
        bin.author = Some(" Zed ".into());
        let mut mp4 = wp("v", "vid", Kind::Video);
        mp4.author = Some("amy".into());
        assert_eq!(format_label(&png), "PNG");
        assert_eq!(format_label(&site), "Website");
        assert_eq!(format_label(&bin), "Program without extension");
        let items = [png, site, bin, mp4];
        let refs: Vec<&Summary> = items.iter().collect();
        let by_kind = arrange(
            &refs,
            &Order {
                key: Key::Kind,
                grouped: true,
                ..Order::default()
            },
            today(),
        );
        let heads: Vec<&str> = by_kind.iter().map(|g| g.label.as_str()).collect();
        assert_eq!(heads, ["Video", "Picture", "Website", "Program"]);
        let by_author = arrange(
            &refs,
            &Order {
                key: Key::Author,
                grouped: true,
                ..Order::default()
            },
            today(),
        );
        assert_eq!(
            labels(&by_author),
            vec![
                ("amy".to_string(), vec!["v".to_string()]),
                ("Zed".to_string(), vec!["b".to_string()]),
                (
                    "Unknown author".to_string(),
                    vec!["p".to_string(), "s".to_string()]
                ),
            ]
        );
        assert_eq!(detail(&items[3], Key::Title).as_deref(), Some("amy"));
        assert_eq!(detail(&items[1], Key::Size).as_deref(), Some("Online"));
        assert_eq!(detail(&items[0], Key::Folder).as_deref(), Some("/media"));
        assert_eq!(Key::from_name("added"), Some(Key::Added));
        assert_eq!(Layout::from_name("list"), Some(Layout::List));
        for k in Key::ALL {
            assert_eq!(Key::from_name(k.name()), Some(k));
        }
    }
}
