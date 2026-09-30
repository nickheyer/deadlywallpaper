use serde::{Deserialize, Serialize};

/// Wallpaper content kind. Serialized by name; LivelyInfo.json uses Lively's numeric codes
/// through [`lively`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Video,
    Gif,
    Picture,
    VideoStream,
    Web,
    WebAudio,
    Url,
    Program,
    /// A Wallpaper Engine scene, drawn by the built-in WebGL renderer from the scene's
    /// unpacked files and Wallpaper Engine's assets folder.
    Scene,
}

/// Lively wallpaper packages: a zip holding `LivelyInfo.json`.
pub const PACKAGE_EXTENSIONS: &[&str] = &["zip"];

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Video => "Video",
            Kind::Gif => "GIF",
            Kind::Picture => "Picture",
            Kind::VideoStream => "Stream",
            Kind::Web => "Web page",
            Kind::WebAudio => "Visualizer",
            Kind::Url => "Website",
            Kind::Program => "Program",
            Kind::Scene => "Scene",
        }
    }

    /// File extensions, lower case, that import as this kind. Empty for kinds that only come
    /// from a URL or a Lively package.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Kind::Video => &[
                "wmv", "avi", "flv", "m4v", "mkv", "mov", "mp4", "mp4v", "mpeg4", "mpg", "mpeg",
                "webm", "ogm", "ogv", "ogx", "ts", "m2ts",
            ],
            Kind::Gif => &["gif"],
            Kind::Picture => &[
                "jpg", "jpeg", "png", "bmp", "tif", "tiff", "webp", "jfif", "avif", "heic",
            ],
            Kind::Web => &["html", "htm"],
            Kind::Program => &["exe", "appimage", "sh", "run"],
            Kind::VideoStream | Kind::WebAudio | Kind::Url | Kind::Scene => &[],
        }
    }

    /// Every extension the file picker offers: each kind's files plus Lively packages.
    pub fn importable_extensions() -> Vec<&'static str> {
        let kinds = [
            Kind::Video,
            Kind::Gif,
            Kind::Picture,
            Kind::Web,
            Kind::Program,
        ];
        kinds
            .iter()
            .flat_map(|k| k.extensions().iter().copied())
            .chain(PACKAGE_EXTENSIONS.iter().copied())
            .collect()
    }

    /// Played through libmpv.
    pub fn is_media(self) -> bool {
        matches!(
            self,
            Kind::Video | Kind::Gif | Kind::Picture | Kind::VideoStream
        )
    }

    /// Can carry sound: everything but still and animated pictures.
    pub fn has_audio(self) -> bool {
        !matches!(self, Kind::Picture | Kind::Gif)
    }

    /// Plays along a timeline that can be restarted or sought.
    pub fn has_timeline(self) -> bool {
        matches!(self, Kind::Video | Kind::VideoStream | Kind::Gif)
    }

    /// A finite local clip that starts over when it ends.
    pub fn loops(self) -> bool {
        matches!(self, Kind::Video | Kind::Gif)
    }

    /// Played through the platform web view.
    pub fn is_web(self) -> bool {
        matches!(self, Kind::Web | Kind::WebAudio | Kind::Url | Kind::Scene)
    }

    /// Source is a URL rather than a local file.
    pub fn is_online(self) -> bool {
        matches!(self, Kind::Url | Kind::VideoStream)
    }

    /// Source lives in a directory of related files that must travel together.
    pub fn is_directory_project(self) -> bool {
        matches!(
            self,
            Kind::Web | Kind::WebAudio | Kind::Program | Kind::Scene
        )
    }

    pub fn accepts_pointer(self) -> bool {
        self.is_web() || self == Kind::Program
    }

    pub fn from_extension(ext: &str) -> Option<Kind> {
        let e = ext.trim_start_matches('.').to_ascii_lowercase();
        [
            Kind::Video,
            Kind::Gif,
            Kind::Picture,
            Kind::Web,
            Kind::Program,
        ]
        .into_iter()
        .find(|k| k.extensions().contains(&e.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_back_to_their_kind() {
        for kind in [
            Kind::Video,
            Kind::Gif,
            Kind::Picture,
            Kind::Web,
            Kind::Program,
        ] {
            for ext in kind.extensions() {
                assert_eq!(Kind::from_extension(ext), Some(kind), "{ext}");
                assert_eq!(
                    Kind::from_extension(&format!(".{}", ext.to_ascii_uppercase())),
                    Some(kind),
                    "{ext}"
                );
            }
        }
        assert_eq!(Kind::from_extension("txt"), None);
        let all = Kind::importable_extensions();
        assert!(
            all.contains(&"mp4")
                && all.contains(&"html")
                && all.contains(&"zip")
                && all.contains(&"appimage")
        );
    }
}

impl std::str::FromStr for Kind {
    type Err = String;

    fn from_str(s: &str) -> Result<Kind, String> {
        lively::from_name(s).ok_or_else(|| format!("unknown wallpaper kind '{s}'"))
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Video => "video",
            Kind::Gif => "gif",
            Kind::Picture => "picture",
            Kind::VideoStream => "videostream",
            Kind::Web => "web",
            Kind::WebAudio => "webaudio",
            Kind::Url => "url",
            Kind::Program => "program",
            Kind::Scene => "scene",
        })
    }
}

/// Lively's `WallpaperType` encoding: integers in the order
/// app, web, webaudio, url, bizhawk, unity, godot, video, gif, unityaudio, videostream, picture.
/// Scenes have no Lively code and are written by name.
pub mod lively {
    use super::Kind;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn to_code(k: Kind) -> Option<i64> {
        Some(match k {
            Kind::Program => 0,
            Kind::Web => 1,
            Kind::WebAudio => 2,
            Kind::Url => 3,
            Kind::Video => 7,
            Kind::Gif => 8,
            Kind::VideoStream => 10,
            Kind::Picture => 11,
            Kind::Scene => return None,
        })
    }

    pub fn from_code(c: i64) -> Option<Kind> {
        Some(match c {
            0 | 4 | 5 | 6 | 9 => Kind::Program,
            1 => Kind::Web,
            2 => Kind::WebAudio,
            3 => Kind::Url,
            7 => Kind::Video,
            8 => Kind::Gif,
            10 => Kind::VideoStream,
            11 => Kind::Picture,
            _ => return None,
        })
    }

    pub fn from_name(s: &str) -> Option<Kind> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "app" | "program" | "bizhawk" | "unity" | "godot" | "unityaudio" => Kind::Program,
            "web" => Kind::Web,
            "webaudio" => Kind::WebAudio,
            "url" => Kind::Url,
            "video" => Kind::Video,
            "gif" => Kind::Gif,
            "videostream" | "stream" => Kind::VideoStream,
            "picture" | "image" => Kind::Picture,
            "scene" => Kind::Scene,
            _ => return None,
        })
    }

    pub fn serialize<S: Serializer>(k: &Kind, s: S) -> Result<S::Ok, S::Error> {
        match to_code(*k) {
            Some(code) => code.serialize(s),
            None => k.to_string().serialize(s),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Kind, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Code(i64),
            Name(String),
        }
        match Option::<Raw>::deserialize(d)? {
            None => Ok(Kind::Web),
            Some(Raw::Code(c)) => from_code(c).ok_or_else(|| {
                serde::de::Error::custom(format!("unknown wallpaper type code {c}"))
            }),
            Some(Raw::Name(n)) => from_name(&n)
                .ok_or_else(|| serde::de::Error::custom(format!("unknown wallpaper type '{n}'"))),
        }
    }
}
