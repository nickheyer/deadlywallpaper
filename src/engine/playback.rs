//! Playback rules: decide per display whether wallpapers pause and how loud they play.

use crate::model::Display;
use crate::model::settings::{AudioOutput, PauseScope, Rules};
use crate::platform::{Snapshot, WindowInfo, WindowPlacement};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub pause: bool,
    pub volume: u8,
}

pub struct Inputs<'a> {
    pub rules: &'a Rules,
    pub volume: u8,
    pub audio_only_on_desktop: bool,
    pub audio_output: &'a AudioOutput,
    pub displays: &'a [Display],
    pub windows: Option<&'a Snapshot>,
    /// User pause, session lock, battery: everything stops.
    pub global_pause: bool,
}

fn on_display(w: &WindowInfo, d: &Display) -> bool {
    match &w.placement {
        WindowPlacement::Rect(r) => r.intersects(&d.rect),
        WindowPlacement::Outputs(origins) => origins.iter().any(|(x, y)| *x == d.rect.x && *y == d.rect.y),
    }
}

fn is_ours(w: &WindowInfo) -> bool {
    w.app.eq_ignore_ascii_case(crate::paths::APP_ID)
}

pub fn decide(input: &Inputs<'_>) -> HashMap<String, Decision> {
    let windows: Vec<&WindowInfo> = input.windows.map(|s| s.windows.iter().filter(|w| !is_ours(w)).collect()).unwrap_or_default();
    let app_pause = windows.iter().any(|w| {
        input.rules.app_pause.iter().any(|rule| !rule.is_empty() && w.app.to_ascii_lowercase().contains(&rule.to_ascii_lowercase()))
    });
    let desktop_focused = !windows.iter().any(|w| w.focused);
    let mut per: HashMap<String, bool> = HashMap::new();
    for d in input.displays {
        let here: Vec<&WindowInfo> = windows.iter().copied().filter(|w| on_display(w, d)).collect();
        let rects: Vec<_> = here.iter().filter_map(|w| match &w.placement {
            WindowPlacement::Rect(r) => Some(*r),
            WindowPlacement::Outputs(_) => None,
        }).collect();
        let covered = here.iter().any(|w| w.fullscreen)
            || (!rects.is_empty() && d.workarea.coverage(&rects) >= input.rules.coverage)
            || here.iter().any(|w| matches!(w.placement, WindowPlacement::Outputs(_)) && w.maximized);
        let focused_here = here.iter().any(|w| w.focused);
        let pause = input.rules.fullscreen_pause && (covered || (input.rules.focus_pause && focused_here));
        per.insert(d.id.clone(), pause);
    }
    let any_pause = per.values().any(|p| *p);
    let multi = input.displays.len() > 1;
    input
        .displays
        .iter()
        .map(|d| {
            let pause = input.global_pause
                || app_pause
                || match input.rules.scope {
                    PauseScope::All => any_pause,
                    PauseScope::Display => per.get(&d.id).copied().unwrap_or(false),
                };
            let mut volume = input.volume;
            if !desktop_focused && input.audio_only_on_desktop {
                volume = 0;
            }
            if multi {
                let routed = match input.audio_output {
                    AudioOutput::All => true,
                    AudioOutput::Primary => d.primary,
                    AudioOutput::Display(id) => &d.id == id,
                };
                if !routed {
                    volume = 0;
                }
            }
            (d.id.clone(), Decision { pause, volume })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;

    fn displays() -> Vec<Display> {
        vec![
            Display { id: "a".into(), name: "A".into(), rect: Rect::new(0, 0, 1000, 1000), workarea: Rect::new(0, 0, 1000, 950), scale: 1.0, primary: true },
            Display { id: "b".into(), name: "B".into(), rect: Rect::new(1000, 0, 1000, 1000), workarea: Rect::new(1000, 0, 1000, 1000), scale: 1.0, primary: false },
        ]
    }

    fn win(rect: Rect, fullscreen: bool, focused: bool, app: &str) -> WindowInfo {
        WindowInfo { placement: WindowPlacement::Rect(rect), fullscreen, maximized: false, focused, app: app.into(), pid: None }
    }

    fn run(rules: &Rules, snap: &Snapshot, audio_only: bool) -> HashMap<String, Decision> {
        decide(&Inputs { rules, volume: 80, audio_only_on_desktop: audio_only, audio_output: &AudioOutput::All, displays: &displays(), windows: Some(snap), global_pause: false })
    }

    #[test]
    fn fullscreen_pauses_only_its_display() {
        let snap = Snapshot { windows: vec![win(Rect::new(0, 0, 1000, 1000), true, true, "game")] };
        let d = run(&Rules::default(), &snap, true);
        assert!(d["a"].pause);
        assert!(!d["b"].pause);
        assert_eq!(d["a"].volume, 0, "focused app mutes audio when audio-only-on-desktop");
    }

    #[test]
    fn coverage_counts_as_covered() {
        let snap = Snapshot { windows: vec![win(Rect::new(0, 0, 1000, 930), false, false, "editor")] };
        let d = run(&Rules::default(), &snap, false);
        assert!(d["a"].pause, "930/950 of the work area is covered");
        assert_eq!(d["a"].volume, 80);
        let small = Snapshot { windows: vec![win(Rect::new(0, 0, 500, 500), false, false, "editor")] };
        assert!(!run(&Rules::default(), &small, false)["a"].pause);
    }

    #[test]
    fn scope_all_and_app_rules() {
        let snap = Snapshot { windows: vec![win(Rect::new(0, 0, 1000, 1000), true, false, "game")] };
        let rules = Rules { scope: PauseScope::All, ..Rules::default() };
        assert!(run(&rules, &snap, false)["b"].pause);
        let rules = Rules { fullscreen_pause: false, app_pause: vec!["Game".into()], ..Rules::default() };
        assert!(run(&rules, &snap, false)["b"].pause);
        let rules = Rules { fullscreen_pause: false, ..Rules::default() };
        assert!(!run(&rules, &snap, false)["a"].pause);
    }

    #[test]
    fn own_windows_and_global_pause() {
        let snap = Snapshot { windows: vec![win(Rect::new(0, 0, 1000, 1000), true, true, "deadlywp")] };
        let d = run(&Rules::default(), &snap, true);
        assert!(!d["a"].pause);
        assert_eq!(d["a"].volume, 80);
        let g = decide(&Inputs { rules: &Rules::default(), volume: 80, audio_only_on_desktop: true, audio_output: &AudioOutput::Primary, displays: &displays(), windows: None, global_pause: true });
        assert!(g["a"].pause && g["b"].pause);
        assert_eq!(g["b"].volume, 0, "audio routed to primary only");
    }
}
