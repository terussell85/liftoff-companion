//! Pure parser for Liftoff's Unity `Player.log`.
//!
//! Both *Liftoff: Micro Drones* and *Liftoff: FPV Drone Racing* write an
//! identical, plain-text `Level setup:` block on every track load, plus a few
//! flight-lifecycle lines. This module turns a stream of log lines into
//! [`LogEvent`]s. It performs **no I/O** so it is fully unit-testable; the
//! tailer in `gamelog/` owns file reading, timestamps, and side effects.
//!
//! Example block (Micro Drones):
//! ```text
//! Level setup:
//! Flags: Race
//! Environment: SilverScreen
//! Type: DRONE   Name: [Copy] [Copy] Air75   Status: Player-created   Local ID: 1c3e...
//! Type: TRACK   Name: 01 - Garage Galore     Status: Internal         Local ID: b783...
//! Type: RACE    Name: 01 - Garage Galore      Status: Internal         Local ID: 75f6...
//! ```
//! The log has **no per-line timestamps** — callers stamp events themselves.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DetectedContext {
    /// Raw `Environment:` value from the log, e.g. `SilverScreen`.
    pub environment_raw: String,
    /// Display level name, e.g. `Azure District`.
    pub level: String,
    /// Display game mode, e.g. `Race`, `Free Flight`, `Stunt`.
    pub game_mode: Option<String>,
    pub drone: Option<String>,
    pub track: Option<String>,
    pub race: Option<String>,
    /// GUID of the RACE (matches `raceTimes.xml`); falls back to the TRACK id.
    pub race_guid: Option<String>,
    /// Game title that produced this context (set by the tailer per source log).
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LogEvent {
    LevelSetup(DetectedContext),
    SceneLoad { scene: String, is_menu: bool },
    FlightActive,
    Paused,
    ResetLocked,
    ResetUnlocked,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    Drone,
    Track,
    Race,
    Other,
}

#[derive(Debug, Default, Clone)]
struct BlockAccumulator {
    flags: Option<String>,
    environment: Option<String>,
    drone: Option<String>,
    track: Option<String>,
    race: Option<String>,
    track_guid: Option<String>,
    race_guid: Option<String>,
    current: Option<Section>,
}

impl BlockAccumulator {
    fn finalize(self, title: Option<String>) -> Option<DetectedContext> {
        let environment_raw = self.environment?;
        let level = environment_display(&environment_raw);
        Some(DetectedContext {
            level,
            game_mode: self.flags.as_deref().map(mode_display),
            drone: self.drone,
            track: self.track.clone(),
            race: self.race.clone().or(self.track),
            race_guid: self.race_guid.or(self.track_guid),
            title,
            environment_raw,
        })
    }
}

/// Incremental, line-at-a-time parser. One per source file (carries its title).
pub struct LineParser {
    title: Option<String>,
    block: Option<BlockAccumulator>,
    /// Last seen flight-mapping state, to debounce repeated `Flight.`/`Menu.` lines.
    in_flight: bool,
}

impl LineParser {
    pub fn new(title: Option<String>) -> Self {
        Self {
            title,
            block: None,
            in_flight: false,
        }
    }

    /// Feed one line; returns any events it produced (usually 0 or 1).
    pub fn push_line(&mut self, line: &str) -> Vec<LogEvent> {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        let t = trimmed.trim();

        // `Level setup:` opens a new block (finalizing any pending one first).
        if t == "Level setup:" {
            let mut out = Vec::new();
            if let Some(ev) = self.flush_block() {
                out.push(ev);
            }
            self.block = Some(BlockAccumulator::default());
            return out;
        }

        // While a block is open, consume its grammar lines.
        if self.block.is_some() {
            if let Some((key, val)) = split_field(t) {
                if is_block_key(key) {
                    self.apply_block_field(key, val);
                    return Vec::new();
                }
            }
            // Non-grammar line ends the block; fall through to handle this line.
            let mut out = Vec::new();
            if let Some(ev) = self.flush_block() {
                out.push(ev);
            }
            out.extend(self.parse_loose_line(t));
            return out;
        }

        self.parse_loose_line(t)
    }

    /// Flush a trailing block (e.g. at EOF). Call after the last line.
    pub fn finish(&mut self) -> Vec<LogEvent> {
        self.flush_block().into_iter().collect()
    }

    fn flush_block(&mut self) -> Option<LogEvent> {
        let block = self.block.take()?;
        block.finalize(self.title.clone()).map(LogEvent::LevelSetup)
    }

    fn apply_block_field(&mut self, key: &str, val: &str) {
        match key.to_ascii_lowercase().as_str() {
            "flags" => self.block_mut().flags = nonempty(val),
            "environment" => self.block_mut().environment = nonempty(val),
            "type" => {
                let upper = val.to_ascii_uppercase();
                let section = if upper.contains("DRONE") {
                    Section::Drone
                } else if upper.contains("TRACK") {
                    Section::Track
                } else if upper.contains("RACE") {
                    Section::Race
                } else {
                    Section::Other
                };
                self.block_mut().current = Some(section);
            }
            "name" => {
                let v = nonempty(val);
                match self.block_mut().current {
                    Some(Section::Drone) => self.block_mut().drone = v,
                    Some(Section::Track) => self.block_mut().track = v,
                    Some(Section::Race) => self.block_mut().race = v,
                    _ => {}
                }
            }
            "local id" => {
                let v = nonempty(val);
                match self.block_mut().current {
                    Some(Section::Track) => self.block_mut().track_guid = v,
                    Some(Section::Race) => self.block_mut().race_guid = v,
                    _ => {}
                }
            }
            _ => {} // "status" and anything else ignored
        }
    }

    fn block_mut(&mut self) -> &mut BlockAccumulator {
        self.block.get_or_insert_with(BlockAccumulator::default)
    }

    fn parse_loose_line(&mut self, t: &str) -> Vec<LogEvent> {
        // Scene-load lines are wrapped in `===` rules, so search rather than prefix-match.
        if let Some(pos) = t.find("SCENE LOAD START:") {
            let rest = &t[pos + "SCENE LOAD START:".len()..];
            let scene = rest.trim().trim_matches('=').trim().to_string();
            let is_menu = is_menu_scene(&scene);
            return vec![LogEvent::SceneLoad { scene, is_menu }];
        }
        if t.contains("Enabling controller mapping: Flight.") {
            if !self.in_flight {
                self.in_flight = true;
                return vec![LogEvent::FlightActive];
            }
            return Vec::new();
        }
        if t.contains("Enabling controller mapping: Menu.")
            || t.contains("Restoring controller mappings for ID InGameMenu")
        {
            if self.in_flight {
                self.in_flight = false;
                return vec![LogEvent::Paused];
            }
            return Vec::new();
        }
        if t.starts_with("Drone reset locked") {
            return vec![LogEvent::ResetLocked];
        }
        if t.starts_with("Drone reset unlocked") {
            return vec![LogEvent::ResetUnlocked];
        }
        Vec::new()
    }
}

/// Scan a whole file's text for the current track context to backfill when a
/// capture starts mid-flight: the most recent `Level setup:` block, unless a
/// menu scene-load follows it (the player returned to a menu, so there is no
/// active track and the block is stale).
pub fn find_last_context(text: &str, title: Option<String>) -> Option<DetectedContext> {
    let mut parser = LineParser::new(title);
    let mut last: Option<DetectedContext> = None;
    let apply = |ev: LogEvent, last: &mut Option<DetectedContext>| match ev {
        LogEvent::LevelSetup(ctx) => *last = Some(ctx),
        LogEvent::SceneLoad { is_menu: true, .. } => *last = None,
        _ => {}
    };
    for line in text.lines() {
        for ev in parser.push_line(line) {
            apply(ev, &mut last);
        }
    }
    for ev in parser.finish() {
        apply(ev, &mut last);
    }
    last
}

fn split_field(line: &str) -> Option<(&str, &str)> {
    let idx = line.find(':')?;
    Some((line[..idx].trim(), line[idx + 1..].trim()))
}

fn is_block_key(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "flags" | "environment" | "type" | "name" | "status" | "local id"
    )
}

fn nonempty(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn is_menu_scene(scene: &str) -> bool {
    // Liftoff menu/system scenes are prefixed `XS` (XSMainMenu, XSSplashScreen).
    scene.starts_with("XS") || scene.eq_ignore_ascii_case("MainMenu")
}

pub fn mode_display(flag: &str) -> String {
    match flag.trim() {
        "Race" => "Race",
        "FreeFlight" => "Free Flight",
        "StuntMode" => "Stunt",
        "TimeTrial" => "Time Trial",
        other => other,
    }
    .to_string()
}

/// Map a raw `Environment:` internal name to a display name. Known internals are
/// mapped explicitly (notably `SilverScreen → Azure District`); unknowns fall
/// back to splitting CamelCase/number runs (`StrawBale → Straw Bale`).
pub fn environment_display(raw: &str) -> String {
    match raw {
        // Liftoff: Micro Drones
        "SilverScreen" => "Azure District",
        "InTransit" => "In Transit",
        "HovertonHigh" => "Hoverton High",
        "MelonpanPark" | "MelonPanPark" => "Melon-pan Park",
        "SawdustInc" => "Sawdust Inc.",
        "Sealand" => "Sealand",
        "SanLipoDrive" => "San Lipo Drive",
        // Liftoff: FPV Drone Racing
        "StrawBale" => "Straw Bale",
        "PineValley" => "Pine Valley",
        "MinusTwo" => "Minus Two",
        "AutumnFields" => "Autumn Fields",
        "LiftoffArena" => "Liftoff Arena",
        "DubaiLegends" => "Dubai Legends",
        "ParisDroneFestival" => "Paris Drone Festival",
        "ThePit" => "The Pit",
        "TheGreen" => "The Green",
        "BardwellsYard" => "Bardwell's Yard",
        "BandoCity" => "Bando City",
        "TheWoodpecker" => "The Woodpecker",
        "ShortCircuit" => "Short Circuit",
        _ => return split_camel(raw),
    }
    .to_string()
}

/// Insert spaces between CamelCase words and before digit runs:
/// `HangarC03 -> Hangar C03`, `Hall26 -> Hall 26`, `StrawBale -> Straw Bale`.
fn split_camel(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 {
            let prev = chars[i - 1];
            let boundary = (c.is_uppercase() && (prev.is_lowercase() || prev.is_ascii_digit()))
                // digit run starts a new word only after a lowercase letter
                // (Hall26 -> "Hall 26") but not after a capital (HangarC03 -> "Hangar C03")
                || (c.is_ascii_digit() && prev.is_lowercase())
                || (c.is_uppercase()
                    && prev.is_uppercase()
                    && chars.get(i + 1).is_some_and(|n| n.is_lowercase()));
            if boundary {
                out.push(' ');
            }
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_of(events: Vec<LogEvent>) -> Option<DetectedContext> {
        events.into_iter().find_map(|e| match e {
            LogEvent::LevelSetup(c) => Some(c),
            _ => None,
        })
    }

    // Real Micro Drones block (SilverScreen / Garage Galore), as captured.
    const MD_BLOCK: &str = "\
Level setup:
Flags: Race
Environment: SilverScreen
Type: DRONE
Name: [Copy] [Copy] Air75
Status: Player-created
Local ID: 1c3e3d0d-515d-46a3-a2fa-23bb97e7e744
Type: TRACK
Name: 01 - Garage Galore
Status: Internal
Local ID: b7830037-571b-4f04-a3b1-3eb5b3850ad9
Type: RACE
Name: 01 - Garage Galore
Status: Internal
Local ID: 75f61b19-504d-49c0-8f88-3a791b6e8441
Disabling all controller mappings.
Enabling controller mapping: Flight.";

    #[test]
    fn parses_micro_drones_block() {
        let mut p = LineParser::new(Some("Liftoff Micro Drones".into()));
        let mut events = Vec::new();
        for line in MD_BLOCK.lines() {
            events.extend(p.push_line(line));
        }
        events.extend(p.finish());

        let ctx = ctx_of(events.clone()).expect("level setup");
        assert_eq!(ctx.environment_raw, "SilverScreen");
        assert_eq!(ctx.level, "Azure District");
        assert_eq!(ctx.game_mode.as_deref(), Some("Race"));
        assert_eq!(ctx.drone.as_deref(), Some("[Copy] [Copy] Air75"));
        assert_eq!(ctx.race.as_deref(), Some("01 - Garage Galore"));
        assert_eq!(
            ctx.race_guid.as_deref(),
            Some("75f61b19-504d-49c0-8f88-3a791b6e8441")
        );
        assert_eq!(ctx.title.as_deref(), Some("Liftoff Micro Drones"));

        // The trailing "Enabling controller mapping: Flight." should fire once.
        assert!(events.contains(&LogEvent::FlightActive));
    }

    #[test]
    fn parses_fpv_freeflight_block_without_track_or_race() {
        // FPV FreeFlight blocks have only a Drone Configuration, no TRACK/RACE.
        let block = "\
Level setup:
Flags: FreeFlight
Environment: StrawBale
Type: Drone Configuration
Name: Skyeliner
Status: Internal
Local ID: aaaa-bbbb
Calculated Power 635";
        let mut p = LineParser::new(Some("Liftoff".into()));
        let mut events = Vec::new();
        for line in block.lines() {
            events.extend(p.push_line(line));
        }
        events.extend(p.finish());
        let ctx = ctx_of(events).expect("level setup");
        assert_eq!(ctx.level, "Straw Bale");
        assert_eq!(ctx.game_mode.as_deref(), Some("Free Flight"));
        assert_eq!(ctx.drone.as_deref(), Some("Skyeliner"));
        assert_eq!(ctx.race, None);
        assert_eq!(ctx.race_guid, None);
    }

    #[test]
    fn find_last_context_picks_latest_block() {
        let text = format!(
            "{}\nsome noise\nLevel setup:\nFlags: StuntMode\nEnvironment: PineValley\nType: Track\nName: 02 - Cinema Premier\nLocal ID: zzz\nDisabling all controller mappings.",
            MD_BLOCK
        );
        let ctx = find_last_context(&text, None).expect("ctx");
        assert_eq!(ctx.level, "Pine Valley");
        assert_eq!(ctx.game_mode.as_deref(), Some("Stunt"));
        assert_eq!(ctx.race.as_deref(), Some("02 - Cinema Premier"));
    }

    #[test]
    fn scene_loads_and_menu_detection() {
        let mut p = LineParser::new(None);
        let menu = p.push_line(
            "================================= SCENE LOAD START: XSMainMenu ===================",
        );
        assert_eq!(
            menu,
            vec![LogEvent::SceneLoad {
                scene: "XSMainMenu".into(),
                is_menu: true
            }]
        );
        let env = p.push_line("SCENE LOAD START: InTransit");
        assert_eq!(
            env,
            vec![LogEvent::SceneLoad {
                scene: "InTransit".into(),
                is_menu: false
            }]
        );
    }

    #[test]
    fn flight_state_is_debounced() {
        let mut p = LineParser::new(None);
        assert_eq!(
            p.push_line("Enabling controller mapping: Flight."),
            vec![LogEvent::FlightActive]
        );
        // Repeated Flight lines do not re-fire.
        assert!(p
            .push_line("Enabling controller mapping: Flight.")
            .is_empty());
        assert_eq!(
            p.push_line("Enabling controller mapping: Menu."),
            vec![LogEvent::Paused]
        );
        assert_eq!(
            p.push_line("Enabling controller mapping: Flight."),
            vec![LogEvent::FlightActive]
        );
    }

    #[test]
    fn camel_split_fallback() {
        assert_eq!(split_camel("StrawBale"), "Straw Bale");
        assert_eq!(split_camel("Hall26"), "Hall 26");
        assert_eq!(split_camel("HangarC03"), "Hangar C03");
        assert_eq!(environment_display("Permafrost"), "Permafrost");
        assert_eq!(environment_display("Hannover"), "Hannover");
    }
}
