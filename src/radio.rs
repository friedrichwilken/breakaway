//! Team radio: the DS (in the team car) and the partner in the break talk to
//! the rider. Lines come from `assets/radio/lines.toml` (one entry per key,
//! a speaker, at least two text variants with `{placeholders}`). The one
//! rule applies to words too: no line may tell the rider to exceed the band.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::race::{Band, BandState, Ride, RideEvent};
use crate::scenario::{Break, Scenario, ScenarioEvent, StepRole};
use crate::workout::{BlockKind, Step, WorkoutRunner, WorkoutSession, step_band};

/// Every placeholder a line may use.
pub const PLACEHOLDERS: [&str; 7] = ["secs", "low", "high", "rep", "reps", "advantage", "matches"];

/// The only placeholders a spoken text (the voice clip) may use; the clip
/// generator renders every value: `{secs}` 10 and 5, `{matches}` 1–4.
pub const SPOKEN_PLACEHOLDERS: [&str; 2] = ["secs", "matches"];

/// The partner's name in the dialogue box.
pub const PARTNER_NAME: &str = "Marco";

/// The keys the game speaks, in the lines file's order (the tests check
/// the file and the triggers against it).
#[cfg(test)]
pub const KEYS: [&str; 22] = [
    "step_warning_pull",
    "step_warning_draft",
    "step_warning_harder",
    "step_warning_easier",
    "workout_start",
    "break_start",
    "pull_start",
    "draft_start",
    "last_rep",
    "block_done",
    "bunch_closing",
    "caught",
    "break_survived",
    "under_band",
    "over_band",
    "match_burned",
    "out_of_matches",
    "back_on_wheel",
    "dropped",
    "in_band_streak",
    "halfway",
    "workout_done",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speaker {
    /// The directeur sportif, on the radio from the team car.
    Ds,
    /// The partner in the break.
    Partner,
}

impl Speaker {
    /// `ds` or `partner`.
    pub fn label(self) -> &'static str {
        match self {
            Speaker::Ds => "ds",
            Speaker::Partner => "partner",
        }
    }

    /// The name in the dialogue box: `DS` or the partner's name.
    pub fn name(self) -> &'static str {
        match self {
            Speaker::Ds => "DS",
            Speaker::Partner => PARTNER_NAME,
        }
    }
}

/// One key's entry in the lines file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Line {
    pub key: String,
    pub speaker: Speaker,
    /// The variants.
    pub text: Vec<String>,
    /// What the voice says, parallel to `text`; `None`: the same as `text`.
    #[serde(default)]
    pub spoken: Option<Vec<String>>,
}

impl Line {
    /// The spoken template of variant `v` (0-based): `spoken[v]`, else
    /// `text[v]`.
    pub fn spoken_text(&self, v: usize) -> &str {
        self.spoken.as_ref().map_or(&self.text[v], |s| &s[v])
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinesFile {
    line: Vec<Line>,
}

/// The `{name}` placeholders in `text`, in order; `Err` on an unclosed or
/// stray brace.
pub fn placeholders(text: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            return Err(format!("stray '}}' in {text:?}"));
        }
        let after = &rest[open + 1..];
        let close = after.find('}').ok_or_else(|| format!("unclosed '{{' in {text:?}"))?;
        out.push(&after[..close]);
        rest = &after[close + 1..];
    }
    Ok(out)
}

/// All lines, by key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lines(Vec<Line>);

impl Lines {
    /// Parses and checks a lines file: keys unique, at least two variants
    /// each, only known placeholders; `spoken` as long as `text`, and every
    /// spoken text (`spoken`, else `text`) only with `{secs}` / `{matches}`.
    pub fn parse(text: &str) -> Result<Lines, String> {
        let file: LinesFile = toml::from_str(text).map_err(|e| e.to_string())?;
        for (i, line) in file.line.iter().enumerate() {
            if file.line[..i].iter().any(|l| l.key == line.key) {
                return Err(format!("{}: duplicate key", line.key));
            }
            if line.text.len() < 2 {
                return Err(format!("{}: needs at least two variants", line.key));
            }
            for variant in &line.text {
                for name in placeholders(variant).map_err(|e| format!("{}: {e}", line.key))? {
                    if !PLACEHOLDERS.contains(&name) {
                        return Err(format!("{}: unknown placeholder {{{name}}}", line.key));
                    }
                }
            }
            if let Some(spoken) = &line.spoken
                && spoken.len() != line.text.len()
            {
                return Err(format!(
                    "{}: spoken has {} variants, text {}",
                    line.key,
                    spoken.len(),
                    line.text.len()
                ));
            }
            for v in 0..line.text.len() {
                for name in placeholders(line.spoken_text(v)).map_err(|e| format!("{}: spoken: {e}", line.key))? {
                    if !SPOKEN_PLACEHOLDERS.contains(&name) {
                        return Err(format!(
                            "{}: spoken variant {} may only use {{secs}} and {{matches}}, not {{{name}}}",
                            line.key,
                            v + 1
                        ));
                    }
                }
            }
        }
        Ok(Lines(file.line))
    }

    pub fn load(path: &Path) -> Result<Lines, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Lines::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn get(&self, key: &str) -> Option<&Line> {
        self.0.iter().find(|l| l.key == key)
    }

    #[cfg(test)]
    pub fn all(&self) -> &[Line] {
        &self.0
    }
}

/// `assets/radio/lines.toml` in the repo, baked in at build time.
pub fn lines_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("radio")
        .join("lines.toml")
}

/// A radio with the real lines and clips, for tests elsewhere.
#[cfg(test)]
pub fn test_radio() -> Radio {
    let mut radio = Radio::new(Lines::load(&lines_path()).unwrap(), "", true);
    radio.set_clips(Clips::load(&manifest_path(), &audio_dir()));
    radio
}

// ── Voice clips ───────────────────────────────────────────────────────────────
/// `assets/radio/audio/` in the repo (generated clips, `just voices`).
pub fn audio_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("radio")
        .join("audio")
}

/// The clips' manifest (`tools/gen_voices.py` writes it).
pub fn manifest_path() -> PathBuf {
    audio_dir().join("manifest.json")
}

/// The clip of variant `variant` (0-based; the name counts from 1) of
/// `key`, without `.ogg`: `<key>-<variant>[-s<secs>][-m<matches>]`. Pass
/// `secs` / `matches` only when the variant's spoken text uses them.
pub fn clip_name(key: &str, variant: usize, secs: Option<u32>, matches: Option<u32>) -> String {
    let mut name = format!("{key}-{}", variant + 1);
    if let Some(s) = secs {
        name += &format!("-s{s}");
    }
    if let Some(m) = matches {
        name += &format!("-m{m}");
    }
    name
}

#[derive(Deserialize)]
struct ManifestEntry {
    duration_s: f32,
}

#[derive(Deserialize)]
struct Manifest {
    clips: std::collections::BTreeMap<String, ManifestEntry>,
}

/// The voice clips there are, by name (without `.ogg`), with their length
/// (s) from the manifest.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clips(std::collections::BTreeMap<String, f32>);

impl Clips {
    /// Parses a manifest: every clip in it.
    pub fn parse(text: &str) -> Result<Clips, String> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let clips = m
            .clips
            .into_iter()
            .filter_map(|(file, e)| Some((file.strip_suffix(".ogg")?.to_owned(), e.duration_s)));
        Ok(Clips(clips.collect()))
    }

    /// The clips of the manifest at `path` whose file exists in `dir`; a
    /// missing or bad manifest means none (text only).
    pub fn load(path: &Path, dir: &Path) -> Clips {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let mut clips = Clips::parse(&text).unwrap_or_default();
        clips.0.retain(|name, _| dir.join(format!("{name}.ogg")).is_file());
        clips
    }

    /// The clip's length (s), if there is one.
    pub fn duration_s(&self, name: &str) -> Option<f32> {
        self.0.get(name).copied()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

// ── Speaking ──────────────────────────────────────────────────────────────────
/// Typewriter reveal speed.
pub const REVEAL_CHARS_PER_S: f32 = 40.0;
/// Once fully shown a line stays `max(HOLD_MIN_S, chars / HOLD_CHARS_PER_S)`.
pub const HOLD_MIN_S: f32 = 3.0;
pub const HOLD_CHARS_PER_S: f32 = 15.0;
/// A `Cue` cuts an `Alert` once the alert's current sentence has been
/// visible this long.
pub const ALERT_SENTENCE_S: f32 = 1.5;
/// A waiting line below `Cue` is dropped after waiting longer than this.
pub const MAX_WAIT_S: f32 = 8.0;
/// At most this many lines wait.
pub const QUEUE_MAX: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Everything else.
    Story,
    /// Under / over the band, matches, bunch closing, caught.
    Alert,
    /// Step warnings, pull / draft starts.
    Cue,
}

/// The priority a key is spoken with.
pub fn priority_of(key: &str) -> Priority {
    match key {
        "step_warning_pull"
        | "step_warning_draft"
        | "step_warning_harder"
        | "step_warning_easier"
        | "pull_start"
        | "draft_start" => Priority::Cue,
        "under_band" | "over_band" | "match_burned" | "out_of_matches" | "bunch_closing" | "caught" => Priority::Alert,
        _ => Priority::Story,
    }
}

/// After a line of this key is accepted, further pushes of it are ignored
/// for this long (s).
pub fn cooldown_s(key: &str) -> f32 {
    match key {
        "under_band" | "over_band" => 30.0,
        "in_band_streak" => 300.0,
        _ => 0.0,
    }
}

/// Values for the placeholders; `None` = not supplied.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vars {
    pub secs: Option<u32>,
    /// Band edges (W), next or current band.
    pub low_w: Option<f32>,
    pub high_w: Option<f32>,
    pub rep: Option<u32>,
    pub reps: Option<u32>,
    pub advantage_s: Option<f32>,
    pub matches: Option<u32>,
}

impl Vars {
    /// The text for placeholder `name`, if supplied: watts rounded to whole
    /// numbers, the advantage as `m:ss` (whole seconds rounded down: a
    /// 9.6 s gap reads `0:09`).
    pub fn value(&self, name: &str) -> Option<String> {
        match name {
            "secs" => self.secs.map(|s| s.to_string()),
            "low" => self.low_w.map(|w| format!("{:.0}", w.round())),
            "high" => self.high_w.map(|w| format!("{:.0}", w.round())),
            "rep" => self.rep.map(|r| r.to_string()),
            "reps" => self.reps.map(|r| r.to_string()),
            "advantage" => self.advantage_s.map(|a| {
                let s = a.max(0.0).floor() as u32;
                format!("{}:{:02}", s / 60, s % 60)
            }),
            "matches" => self.matches.map(|m| m.to_string()),
            _ => None,
        }
    }
}

/// `text` with every supplied placeholder filled in; one that is not
/// supplied stays as `{name}`.
pub fn fill(text: &str, vars: &Vars) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else {
            break;
        };
        out += &rest[..open];
        let name = &rest[open + 1..close];
        out += &vars.value(name).unwrap_or_else(|| format!("{{{name}}}"));
        rest = &rest[close + 1..];
    }
    out + rest
}

/// How long a line of `chars` characters is on screen: the reveal, then
/// the hold.
pub fn line_duration_s(chars: usize) -> f32 {
    let n = chars as f32;
    n / REVEAL_CHARS_PER_S + (n / HOLD_CHARS_PER_S).max(HOLD_MIN_S)
}

/// End (in characters) of the sentence that character position `at` is
/// in: just after its `.`, `!` or `?`, or the end of the text.
fn sentence_end(text: &str, at: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let from = at.max(1);
    (from..=chars.len())
        .find(|&i| i == chars.len() || (matches!(chars[i - 1], '.' | '!' | '?') && chars[i] == ' '))
        .unwrap_or(chars.len())
}

/// The line on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Spoken {
    /// Counts the lines started, so a repeat of the same text is a new line.
    pub id: u64,
    pub key: String,
    pub speaker: Speaker,
    pub text: String,
    /// Characters revealed so far.
    pub shown_chars: usize,
    pub priority: Priority,
    /// Seconds since the line started.
    pub elapsed_s: f32,
    /// The voice clip playing with it (name without `.ogg`); `None`: voice
    /// off or no clip.
    pub clip: Option<String>,
    /// On screen this long: `max(reveal + hold, clip length)`.
    pub duration_s: f32,
    /// A `Cue` waits: reveal stops at this character and the line ends at
    /// `cut_at_s`.
    cut: Option<(usize, f32)>,
}

impl Spoken {
    pub fn chars(&self) -> usize {
        self.text.chars().count()
    }

    /// The revealed part of the text.
    pub fn shown_text(&self) -> String {
        self.text.chars().take(self.shown_chars).collect()
    }

    /// Characters are still being revealed (the mouth moves).
    pub fn revealing(&self) -> bool {
        self.shown_chars < self.cut.map_or(self.chars(), |(end, _)| end)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Waiting {
    key: String,
    vars: Vars,
    priority: Priority,
    waited_s: f32,
}

/// FNV-1a: the seed for a workout name.
fn seed_of(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The radio: one line at a time, a short queue, per-key cooldowns.
/// Pure: `advance(dt)` moves time on.
#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct Radio {
    lines: Lines,
    /// The `radio_enabled` setting; off: silent, nothing waits.
    pub enabled: bool,
    /// The `voice_enabled` setting; off: text only.
    pub voice_enabled: bool,
    clips: Clips,
    current: Option<Spoken>,
    /// Waiting lines, highest priority first, then in arrival order.
    queue: Vec<Waiting>,
    /// splitmix64 state.
    rng: u64,
    /// The variant last spoken per key.
    last_variant: Vec<(String, usize)>,
    /// When each key with a cooldown was last accepted (radio time).
    accepted_at: Vec<(String, f64)>,
    clock_s: f64,
    started: u64,
}

impl Radio {
    /// A silent radio, variants seeded from `seed_name` (the workout's name).
    pub fn new(lines: Lines, seed_name: &str, enabled: bool) -> Radio {
        Radio {
            lines,
            enabled,
            voice_enabled: true,
            clips: Clips::default(),
            current: None,
            queue: Vec::new(),
            rng: seed_of(seed_name),
            last_variant: Vec::new(),
            accepted_at: Vec::new(),
            clock_s: 0.0,
            started: 0,
        }
    }

    /// A new ride: silent, nothing waiting, no cooldowns, variants seeded
    /// from `seed_name` again.
    pub fn reset(&mut self, seed_name: &str) {
        let fresh = Radio::new(std::mem::take(&mut self.lines), seed_name, self.enabled);
        *self = Radio {
            started: self.started,
            voice_enabled: self.voice_enabled,
            clips: std::mem::take(&mut self.clips),
            ..fresh
        };
    }

    /// The voice clips lines play with.
    pub fn set_clips(&mut self, clips: Clips) {
        self.clips = clips;
    }

    /// Voice on or off; off stops the current line's clip (its text stays).
    pub fn set_voice_enabled(&mut self, on: bool) {
        self.voice_enabled = on;
        if !on && let Some(c) = self.current.as_mut() {
            c.clip = None;
        }
    }

    /// Off: the current line and the queue are dropped.
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.current = None;
            self.queue.clear();
        }
    }

    pub fn current(&self) -> Option<&Spoken> {
        self.current.as_ref()
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// `push` with the key's own priority.
    pub fn say(&mut self, key: &str, vars: Vars) -> Result<(), String> {
        self.push(key, vars, priority_of(key))
    }

    /// Says the lines due in one frame, highest priority first (so a cue
    /// due together with a story line does not cut it off unseen).
    pub fn say_all(&mut self, due: &[(&str, Vars)]) -> Result<(), String> {
        let mut due = due.to_vec();
        due.sort_by_key(|(key, _)| std::cmp::Reverse(priority_of(key)));
        due.into_iter().try_for_each(|(key, vars)| self.say(key, vars))
    }

    /// Queues a line (`Err` for an unknown key). Ignored while disabled or
    /// within the key's cooldown. With nothing on screen it starts at once;
    /// a `Cue` replaces a `Story` line at once and cuts an `Alert` after its
    /// current sentence has been visible for 1.5 s. At most 3 lines wait:
    /// beyond that the lowest priority (the newest of those) is dropped.
    pub fn push(&mut self, key: &str, vars: Vars, priority: Priority) -> Result<(), String> {
        if self.lines.get(key).is_none() {
            return Err(format!("unknown radio key {key:?}"));
        }
        if !self.enabled {
            return Ok(());
        }
        let cooldown = f64::from(cooldown_s(key));
        if cooldown > 0.0 {
            if let Some((_, at)) = self.accepted_at.iter().find(|(k, _)| k == key)
                && self.clock_s - at < cooldown
            {
                return Ok(());
            }
            self.accepted_at.retain(|(k, _)| k != key);
            self.accepted_at.push((key.to_owned(), self.clock_s));
        }
        let waiting = Waiting {
            key: key.to_owned(),
            vars,
            priority,
            waited_s: 0.0,
        };
        match &self.current {
            None if self.queue.is_empty() => {
                self.start(waiting);
                return Ok(());
            }
            Some(c) if priority == Priority::Cue && c.priority == Priority::Story => {
                self.start(waiting);
                return Ok(());
            }
            _ => {}
        }
        let at = self
            .queue
            .iter()
            .position(|w| w.priority < priority)
            .unwrap_or(self.queue.len());
        self.queue.insert(at, waiting);
        if self.queue.len() > QUEUE_MAX {
            // The last of the lowest priority: the queue is sorted, so the
            // last entry.
            self.queue.pop();
        }
        self.plan_cut();
        Ok(())
    }

    /// Moves time on by `dt`: reveals, ends lines, drops stale waiting
    /// lines, starts the next one.
    pub fn advance(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.max(0.0) } else { 0.0 };
        self.clock_s += f64::from(dt);
        if !self.enabled {
            self.set_enabled(false);
            return;
        }
        for w in &mut self.queue {
            w.waited_s += dt;
        }
        self.queue
            .retain(|w| w.priority == Priority::Cue || w.waited_s <= MAX_WAIT_S);
        if let Some(c) = self.current.as_mut() {
            c.elapsed_s += dt;
            let limit = c.cut.map_or(c.chars(), |(end, _)| end);
            // The small slack keeps float sums of frame times from losing the
            // last character of a step.
            c.shown_chars = ((c.elapsed_s * REVEAL_CHARS_PER_S + 1e-3) as usize).min(limit);
            let over = match c.cut {
                Some((_, at)) => c.elapsed_s >= at,
                None => c.elapsed_s >= c.duration_s,
            };
            if over {
                self.current = None;
            }
        }
        if self.current.is_none() && !self.queue.is_empty() {
            let next = self.queue.remove(0);
            self.start(next);
        }
        self.plan_cut();
    }

    /// An `Alert` on screen with a `Cue` waiting: reveal stops at the end
    /// of its current sentence, which stays 1.5 s.
    fn plan_cut(&mut self) {
        let cue_waits = self.queue.first().is_some_and(|w| w.priority == Priority::Cue);
        let Some(c) = self.current.as_mut() else { return };
        if c.priority != Priority::Alert || !cue_waits || c.cut.is_some() {
            return;
        }
        let end = sentence_end(&c.text, c.shown_chars);
        c.cut = Some((end, end as f32 / REVEAL_CHARS_PER_S + ALERT_SENTENCE_S));
    }

    fn next_random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Picks a variant (never the one this key used last) and puts the line
    /// on screen.
    fn start(&mut self, w: Waiting) {
        let Some(line) = self.lines.get(&w.key) else { return };
        let (speaker, n) = (line.speaker, line.text.len());
        let last = self.last_variant.iter().find(|(k, _)| *k == w.key).map(|(_, v)| *v);
        let r = self.next_random() as usize;
        let variant = match last {
            Some(l) if n > 1 => {
                let v = r % (n - 1);
                if v >= l { v + 1 } else { v }
            }
            _ => r % n,
        };
        let Some(line) = self.lines.get(&w.key) else { return };
        let text = fill(&line.text[variant], &w.vars);
        let clip = self
            .voice_enabled
            .then(|| self.clip_for(line, variant, &w.vars))
            .flatten();
        let duration_s = clip
            .as_ref()
            .map_or(0.0, |(_, d)| *d)
            .max(line_duration_s(text.chars().count()));
        self.last_variant.retain(|(k, _)| *k != w.key);
        self.last_variant.push((w.key.clone(), variant));
        self.started += 1;
        self.current = Some(Spoken {
            id: self.started,
            key: w.key,
            speaker,
            text,
            shown_chars: 0,
            priority: w.priority,
            elapsed_s: 0.0,
            clip: clip.map(|(name, _)| name),
            duration_s,
            cut: None,
        });
    }

    /// The clip of `variant` of `line` with `vars` and its length, if
    /// there is one.
    fn clip_for(&self, line: &Line, variant: usize, vars: &Vars) -> Option<(String, f32)> {
        let names = placeholders(line.spoken_text(variant)).ok()?;
        let secs = if names.contains(&"secs") {
            Some(vars.secs?)
        } else {
            None
        };
        let matches = if names.contains(&"matches") {
            Some(vars.matches?)
        } else {
            None
        };
        let name = clip_name(&line.key, variant, secs, matches);
        self.clips.duration_s(&name).map(|d| (name, d))
    }
}

// ── Triggers ──────────────────────────────────────────────────────────────────
/// The step warning comes this long before a step ends...
pub const WARN_S: f32 = 10.0;
/// ...or this long for a step shorter than `WARN_SHORT_BELOW_S`.
pub const WARN_SHORT_S: f32 = 5.0;
pub const WARN_SHORT_BELOW_S: f32 = 20.0;
/// No harder / easier warning when the next band centre is within this
/// share of the current one.
pub const SAME_BAND_SHARE: f32 = 0.05;
/// Continuously under / over the band (outside the transition grace) or in
/// it this long (s).
pub const UNDER_AFTER_S: f32 = 5.0;
pub const OVER_AFTER_S: f32 = 2.0;
pub const STREAK_AFTER_S: f32 = 120.0;
/// `last_rep` only in repetition blocks of at least this many reps.
pub const LAST_REP_MIN_REPS: u32 = 3;

/// Band edges as placeholder values.
fn band_vars(band: Band) -> Vars {
    Vars {
        low_w: Some(band.low_w),
        high_w: Some(band.high_w),
        ..Vars::default()
    }
}

/// Step `i` is the last step of a repetition block (of more than one rep).
fn ends_block(steps: &[Step], i: usize) -> bool {
    let s = &steps[i];
    s.block_kind == BlockKind::Repetition && s.reps > 1 && steps.get(i + 1).is_none_or(|n| n.block != s.block)
}

/// Step `i` is the first step of the last repetition of a block with at
/// least 3 reps.
fn starts_last_rep(steps: &[Step], i: usize) -> bool {
    let s = &steps[i];
    let first = i == 0 || steps[i - 1].block != s.block || steps[i - 1].rep != s.rep;
    s.block_kind == BlockKind::Repetition && s.reps >= LAST_REP_MIN_REPS && s.rep == s.reps && first
}

/// The warning before step `i` ends: pull / draft when the next step has
/// that role, else harder / easier by the band centres, none when the next
/// centre is within 5 % (or there is no next step).
pub fn warning_key(steps: &[Step], plan: &[StepRole], i: usize, ftp_w: f32) -> Option<&'static str> {
    let next = steps.get(i + 1)?;
    match plan.get(i + 1) {
        Some(StepRole::Pull { .. }) => Some("step_warning_pull"),
        Some(StepRole::Draft { .. }) => Some("step_warning_draft"),
        _ => {
            let (now, then) = (
                step_band(&steps[i], ftp_w).centre_w(),
                step_band(next, ftp_w).centre_w(),
            );
            if (then - now).abs() <= SAME_BAND_SHARE * now {
                None
            } else if then > now {
                Some("step_warning_harder")
            } else {
                Some("step_warning_easier")
            }
        }
    }
}

/// The line for a scenario event; `brk` is the running break after it.
pub fn scenario_line(event: ScenarioEvent, brk: Option<&Break>, ride: &Ride) -> (&'static str, Vars) {
    let advantage = |a: f32| Vars {
        advantage_s: Some(a),
        ..Vars::default()
    };
    match event {
        ScenarioEvent::BreakStarted { .. } => ("break_start", Vars::default()),
        ScenarioEvent::PullStarted => ("pull_start", band_vars(ride.band)),
        ScenarioEvent::DraftStarted => ("draft_start", Vars::default()),
        ScenarioEvent::BunchClosing => ("bunch_closing", advantage(brk.map_or(0.0, |b| b.advantage_s))),
        ScenarioEvent::Caught => ("caught", Vars::default()),
        ScenarioEvent::BreakSurvived { advantage_s } => ("break_survived", advantage(advantage_s)),
    }
}

/// Values for a line said on request (`RadioSay`): the current band, step
/// rep, break advantage (0 outside a set) and matches, `{secs}` = 10.
pub fn current_vars(ride: &Ride, session: &WorkoutSession, scenario: &Scenario) -> Vars {
    let step = session.runner.as_ref().filter(|r| !r.finished).map(|r| r.current());
    let brk = scenario.brk.filter(|b| scenario.role.set() == Some(b.set));
    Vars {
        secs: Some(WARN_S as u32),
        rep: Some(step.map_or(1, |s| s.rep)),
        reps: Some(step.map_or(1, |s| s.reps)),
        advantage_s: Some(brk.map_or(0.0, |b| b.advantage_s)),
        matches: Some(ride.matches),
        ..band_vars(ride.band)
    }
}

/// The line for a ride event; none for burning the last match
/// (`out_of_matches` alone is said then).
pub fn ride_line(event: RideEvent) -> Option<(&'static str, Vars)> {
    Some(match event {
        RideEvent::MatchBurned { left: 0 } => return None,
        RideEvent::MatchBurned { left } => (
            "match_burned",
            Vars {
                matches: Some(left),
                ..Vars::default()
            },
        ),
        RideEvent::OutOfMatches => ("out_of_matches", Vars::default()),
        RideEvent::BackOnWheel => ("back_on_wheel", Vars::default()),
        RideEvent::Dropped => ("dropped", Vars::default()),
    })
}

/// Watches the workout, the ride and the scenario frame by frame and says
/// which lines are due (each once per occasion). Pure: `dt` comes in.
#[derive(bevy::prelude::Resource, Debug, Clone, Default)]
pub struct Triggers {
    /// `WorkoutSession::loads` this state belongs to.
    loads_seen: Option<u64>,
    /// `workout_start` was said for this load.
    started: bool,
    /// Last frame's running step and its remaining time.
    step: Option<(usize, f32)>,
    finished: bool,
    elapsed_s: Option<f64>,
    /// Continuous time under (outside grace), over (outside grace), in.
    under_s: f32,
    over_s: f32,
    in_s: f32,
}

impl Triggers {
    /// One frame, after the ride, the workout and the scenario were
    /// stepped by `dt`; their events of this frame come in. The lines due,
    /// in order.
    pub fn step(
        &mut self,
        session: &WorkoutSession,
        ride: &Ride,
        scenario: &Scenario,
        scenario_events: &[ScenarioEvent],
        ride_events: &[RideEvent],
        dt: f32,
    ) -> Vec<(&'static str, Vars)> {
        let mut out = Vec::new();
        if self.loads_seen != Some(session.loads) {
            *self = Triggers {
                loads_seen: Some(session.loads),
                ..Triggers::default()
            };
        }
        out.extend(
            scenario_events
                .iter()
                .map(|e| scenario_line(*e, scenario.brk.as_ref(), ride)),
        );
        out.extend(ride_events.iter().filter_map(|e| ride_line(*e)));
        match &session.runner {
            Some(r) => self.workout(r, session.ftp_w, ride, &scenario.plan, &mut out),
            None => (self.step, self.finished, self.elapsed_s) = (None, false, None),
        }
        self.band_state(ride, dt, &mut out);
        out
    }

    fn workout(
        &mut self,
        r: &WorkoutRunner,
        ftp_w: f32,
        ride: &Ride,
        plan: &[StepRole],
        out: &mut Vec<(&'static str, Vars)>,
    ) {
        let steps = &r.workout.steps;
        if !self.started && ride.state != BandState::Idle {
            self.started = true;
            out.push(("workout_start", Vars::default()));
        }
        let prev = self.step.map(|(i, _)| i);
        let now = (!r.finished).then_some(r.index);
        if now != prev || r.finished != self.finished {
            // A step ended into the next one, or into the finish.
            let ended = prev.filter(|&p| now == Some(p + 1) || (r.finished && r.index == p));
            if ended.is_some_and(|p| ends_block(steps, p)) {
                out.push(("block_done", Vars::default()));
            }
            if let Some(i) = now.filter(|&i| starts_last_rep(steps, i)) {
                out.push((
                    "last_rep",
                    Vars {
                        rep: Some(steps[i].rep),
                        reps: Some(steps[i].reps),
                        ..Vars::default()
                    },
                ));
            }
            if r.finished && !self.finished {
                out.push(("workout_done", Vars::default()));
            }
        }
        // The warning: once, when the remaining time of a running step
        // crosses the lead time.
        if let Some(i) = now {
            let remaining = r.step_remaining_s();
            let lead = if steps[i].duration_s < WARN_SHORT_BELOW_S {
                WARN_SHORT_S
            } else {
                WARN_S
            };
            let crossed = self
                .step
                .is_some_and(|(p, before)| p == i && before > lead && remaining <= lead);
            if let Some(key) = warning_key(steps, plan, i, ftp_w).filter(|_| crossed) {
                let vars = Vars {
                    secs: Some(lead as u32),
                    ..band_vars(step_band(&steps[i + 1], ftp_w))
                };
                out.push((key, vars));
            }
        }
        let half = f64::from(r.total_s()) / 2.0;
        if self
            .elapsed_s
            .is_some_and(|before| before < half && r.elapsed_s >= half)
        {
            out.push(("halfway", Vars::default()));
        }
        self.step = now.map(|i| (i, r.step_remaining_s()));
        self.finished = r.finished;
        self.elapsed_s = Some(r.elapsed_s);
    }

    /// Under / over the band outside the grace, and long spells in it.
    fn band_state(&mut self, ride: &Ride, dt: f32, out: &mut Vec<(&'static str, Vars)>) {
        let live = ride.grace.is_none();
        let cross = |t: &mut f32, on: bool, limit: f32| {
            let before = *t;
            *t = if on { before + dt } else { 0.0 };
            on && before < limit && *t >= limit
        };
        if cross(&mut self.under_s, ride.state == BandState::Under && live, UNDER_AFTER_S) {
            out.push((
                "under_band",
                Vars {
                    low_w: Some(ride.band.low_w),
                    ..Vars::default()
                },
            ));
        }
        if cross(&mut self.over_s, ride.state == BandState::Over && live, OVER_AFTER_S) {
            out.push((
                "over_band",
                Vars {
                    high_w: Some(ride.band.high_w),
                    ..Vars::default()
                },
            ));
        }
        if cross(&mut self.in_s, ride.state == BandState::In, STREAK_AFTER_S) {
            out.push(("in_band_streak", Vars::default()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Lines {
        Lines::load(&lines_path()).unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn the_lines_file_parses_and_has_every_key() {
        let lines = lines();
        let keys: Vec<&str> = lines.all().iter().map(|l| l.key.as_str()).collect();
        assert_eq!(keys, KEYS);
        for key in KEYS {
            let line = lines.get(key).unwrap();
            assert!(line.text.len() >= 2, "{key}");
            for variant in &line.text {
                assert!(
                    placeholders(variant).unwrap().iter().all(|p| PLACEHOLDERS.contains(p)),
                    "{key}: {variant}"
                );
            }
        }
        assert_eq!(lines.get("pull_start").unwrap().speaker, Speaker::Partner);
        assert_eq!(lines.get("over_band").unwrap().speaker, Speaker::Ds);
        assert_eq!(
            (Speaker::Ds.name(), Speaker::Partner.name(), Speaker::Partner.label()),
            ("DS", "Marco", "partner")
        );
        // The keys with their own spoken text; the rest speak `text`.
        let with_spoken: Vec<&str> = lines
            .all()
            .iter()
            .filter(|l| l.spoken.is_some())
            .map(|l| l.key.as_str())
            .collect();
        assert_eq!(
            with_spoken,
            [
                "step_warning_harder",
                "step_warning_easier",
                "pull_start",
                "last_rep",
                "bunch_closing",
                "break_survived",
                "under_band",
                "over_band",
            ]
        );
        assert_eq!(lines.get("pull_start").unwrap().spoken_text(0), "You're on. Steady.");
        assert_eq!(lines.get("draft_start").unwrap().spoken_text(2), "Tuck in behind me.");
    }

    #[test]
    fn the_last_match_is_out_of_matches_alone() {
        let key = |e| ride_line(e).map(|(k, v)| (k, v.matches));
        assert_eq!(key(RideEvent::MatchBurned { left: 1 }), Some(("match_burned", Some(1))));
        assert_eq!(key(RideEvent::MatchBurned { left: 0 }), None);
        assert_eq!(key(RideEvent::OutOfMatches), Some(("out_of_matches", None)));
        // Burning all five: four `match_burned`, then `out_of_matches` alone.
        let mut ride = Ride::new(Band::new(100.0, 120.0).unwrap());
        let mut said = Vec::new();
        for _ in 0..600 {
            let events = crate::race::step(&mut ride, 200.0, 0.1);
            said.extend(events.into_iter().filter_map(ride_line).map(|(k, v)| (k, v.matches)));
        }
        let burned: Vec<Option<u32>> = said
            .iter()
            .filter(|(k, _)| *k == "match_burned")
            .map(|(_, m)| *m)
            .collect();
        assert_eq!(burned, [Some(4), Some(3), Some(2), Some(1)]);
        assert_eq!(said.iter().filter(|(k, _)| *k == "out_of_matches").count(), 1);
        assert_eq!(said.last().map(|(k, _)| *k), Some("out_of_matches"));
    }

    #[test]
    fn clip_names_cover_the_manifest_exactly() {
        let lines = lines();
        let clips = Clips::load(&manifest_path(), &audio_dir());
        let mut names = Vec::new();
        for line in lines.all() {
            for v in 0..line.text.len() {
                let used = placeholders(line.spoken_text(v)).unwrap();
                let secs: Vec<Option<u32>> = if used.contains(&"secs") {
                    vec![Some(10), Some(5)]
                } else {
                    vec![None]
                };
                let matches: Vec<Option<u32>> = if used.contains(&"matches") {
                    (1..=4).map(Some).collect()
                } else {
                    vec![None]
                };
                for s in &secs {
                    for m in &matches {
                        let name = clip_name(&line.key, v, *s, *m);
                        assert!(clips.duration_s(&name).is_some_and(|d| d > 0.0), "{name} has no clip");
                        names.push(name);
                    }
                }
            }
        }
        // Every clip in the manifest is one of them, and no name twice.
        assert_eq!(names.len(), clips.len());
        names.sort();
        names.dedup();
        assert_eq!(names.len(), clips.len());
        assert_eq!(clip_name("workout_start", 0, None, None), "workout_start-1");
        assert_eq!(
            clip_name("step_warning_pull", 2, Some(5), None),
            "step_warning_pull-3-s5"
        );
        assert_eq!(clip_name("match_burned", 1, None, Some(4)), "match_burned-2-m4");
        // A missing or bad manifest: no clips; a listed file that is not
        // there is left out.
        assert_eq!(
            Clips::load(Path::new("/nonexistent/manifest.json"), &audio_dir()).len(),
            0
        );
        assert!(Clips::parse("not json").is_err());
        let dir = std::env::temp_dir().join(format!("breakaway-clips-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-1.ogg"), b"").unwrap();
        std::fs::write(
            dir.join("manifest.json"),
            r#"{"clips": {"a-1.ogg": {"hash": "x", "duration_s": 1.5}, "b-1.ogg": {"hash": "y", "duration_s": 2.0}}}"#,
        )
        .unwrap();
        let some = Clips::load(&dir.join("manifest.json"), &dir);
        assert_eq!(
            (some.duration_s("a-1"), some.duration_s("b-1"), some.len()),
            (Some(1.5), None, 1)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lines_play_their_clip_and_stay_as_long_as_it() {
        let mut r = test_radio();
        let warn = Vars {
            secs: Some(10),
            low_w: Some(240.0),
            high_w: Some(290.0),
            ..Vars::default()
        };
        r.say("step_warning_harder", warn).unwrap();
        let c = r.current().unwrap();
        let clip = c.clip.clone().unwrap();
        assert!(
            clip.starts_with("step_warning_harder-") && clip.ends_with("-s10"),
            "{clip}"
        );
        assert!(audio_dir().join(format!("{clip}.ogg")).is_file());
        // Matches: the clip names the count; 5 left has no clip (text only).
        let mut r = test_radio();
        r.say(
            "match_burned",
            Vars {
                matches: Some(3),
                ..Vars::default()
            },
        )
        .unwrap();
        assert!(r.current().unwrap().clip.as_deref().unwrap().ends_with("-m3"));
        let mut r = test_radio();
        r.say(
            "match_burned",
            Vars {
                matches: Some(5),
                ..Vars::default()
            },
        )
        .unwrap();
        assert_eq!(
            (
                r.current().unwrap().clip.as_deref(),
                r.current().unwrap().text.contains('5')
            ),
            (None, true)
        );
        // Spoken text without placeholders: the clip of the variant shown.
        let mut r = test_radio();
        r.say(
            "pull_start",
            Vars {
                low_w: Some(240.0),
                high_w: Some(290.0),
                ..Vars::default()
            },
        )
        .unwrap();
        let c = r.current().unwrap();
        let v = lines()
            .get("pull_start")
            .unwrap()
            .text
            .iter()
            .position(|t| {
                fill(
                    t,
                    &Vars {
                        low_w: Some(240.0),
                        high_w: Some(290.0),
                        ..Vars::default()
                    },
                ) == c.text
            })
            .unwrap();
        assert_eq!(c.clip.as_deref(), Some(clip_name("pull_start", v, None, None).as_str()));

        // On screen max(text rule, clip): a 20-character line (3.5 s) with a
        // 9 s clip stays 9 s; with a 1 s clip, 3.5 s.
        let mut r = radio();
        r.set_clips(Clips::parse(r#"{"clips": {"s20-1.ogg": {"duration_s": 9.0}, "s20-2.ogg": {"duration_s": 9.0}, "a20-1.ogg": {"duration_s": 1.0}, "a20-2.ogg": {"duration_s": 1.0}}}"#).unwrap());
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        assert_eq!(r.current().unwrap().duration_s, 9.0);
        run(&mut r, 8.9);
        assert_eq!(key(&r), Some("s20"));
        run(&mut r, 0.15);
        assert_eq!(key(&r), None);
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        assert!((r.current().unwrap().duration_s - 3.5).abs() < 1e-6);
        assert!(r.current().unwrap().clip.is_some());
        // Voice off: no clip, the text rule; turning it off stops the clip
        // of the line on screen, the text stays.
        r.set_voice_enabled(false);
        assert_eq!((key(&r), r.current().unwrap().clip.as_deref()), (Some("a20"), None));
        run(&mut r, 4.0);
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        assert_eq!(
            (r.current().unwrap().clip.as_deref(), r.current().unwrap().duration_s),
            (None, 3.5)
        );
        // A reset keeps the voice setting and the clips.
        r.set_voice_enabled(true);
        r.reset("x");
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        assert!(r.current().unwrap().clip.is_some() && r.voice_enabled);
    }

    #[test]
    fn placeholders_and_bad_files() {
        assert_eq!(
            placeholders("{low} to {high}, in {secs}.").unwrap(),
            ["low", "high", "secs"]
        );
        assert!(placeholders("none").unwrap().is_empty());
        assert!(placeholders("open {low").is_err());
        assert!(placeholders("stray }").is_err());
        let one =
            |speaker: &str, text: &str| format!("[[line]]\nkey = \"k\"\nspeaker = \"{speaker}\"\ntext = [{text}]\n");
        assert!(Lines::parse(&one("ds", "\"a {secs}\", \"b {matches}\"")).is_ok());
        let spoken =
            Lines::parse(&(one("ds", "\"a {low}\", \"b\"") + "spoken = [\"a {secs}\", \"b {matches}\"]\n")).unwrap();
        let line = spoken.get("k").unwrap();
        assert_eq!(
            (line.text[0].as_str(), line.spoken_text(0), line.spoken_text(1)),
            ("a {low}", "a {secs}", "b {matches}")
        );
        for bad in [
            one("ds", "\"only one\""),
            one("coach", "\"a\", \"b\""),
            one("ds", "\"a {watts}\", \"b\""),
            one("ds", "\"a {low\", \"b\""),
            one("ds", "\"a\", \"b\"") + &one("ds", "\"c\", \"d\""),
            one("ds", "\"a\", \"b\"").replace("key =", "mood = \"x\"\nkey ="),
            // Spoken: same length as text, only {secs} and {matches}; a
            // text without spoken is spoken as it is.
            one("ds", "\"a\", \"b\"") + "spoken = [\"a\"]\n",
            one("ds", "\"a\", \"b\"") + "spoken = [\"a\", \"b {low}\"]\n",
            one("ds", "\"a\", \"b\"") + "spoken = [\"a\", \"b {advantage\"]\n",
            one("ds", "\"a {high}\", \"b\""),
            "not toml".to_owned(),
        ] {
            assert!(Lines::parse(&bad).is_err(), "{bad}");
        }
        assert!(
            Lines::load(Path::new("/nonexistent/lines.toml"))
                .unwrap_err()
                .contains("/nonexistent/lines.toml")
        );
    }

    #[test]
    fn fills_placeholders() {
        let v = Vars {
            secs: Some(10),
            low_w: Some(115.6),
            high_w: Some(140.0),
            advantage_s: Some(34.0),
            ..Vars::default()
        };
        assert_eq!(
            fill("{low} to {high} in {secs}, {advantage}.", &v),
            "116 to 140 in 10, 0:34."
        );
        assert_eq!(fill("Final rep, {rep} of {reps}.", &v), "Final rep, {rep} of {reps}.");
        let adv = |a: f32| {
            Vars {
                advantage_s: Some(a),
                ..Vars::default()
            }
            .value("advantage")
            .unwrap()
        };
        assert_eq!(
            (adv(125.0), adv(0.3), adv(0.0)),
            ("2:05".to_owned(), "0:00".to_owned(), "0:00".to_owned())
        );
        // Rounded down: a 9.6 s gap reads 0:09.
        assert_eq!(
            (adv(9.6), adv(59.99), adv(60.0)),
            ("0:09".to_owned(), "0:59".to_owned(), "1:00".to_owned())
        );
        assert_eq!(
            Vars {
                matches: Some(0),
                ..Vars::default()
            }
            .value("matches")
            .as_deref(),
            Some("0")
        );
        assert_eq!(v.value("watts"), None);
    }

    /// Test lines whose lengths are known: `s<n>`/`a<n>`/`c<n>` with `n`
    /// characters (an `x` run), plus two-sentence and cooldown keys.
    fn radio() -> Radio {
        let mut text = String::new();
        let mut add = |key: &str, a: &str, b: &str| {
            text += &format!("[[line]]\nkey = \"{key}\"\nspeaker = \"ds\"\ntext = [\"{a}\", \"{b}\"]\n");
        };
        for n in [20, 60, 150] {
            let x = "x".repeat(n);
            for kind in ["s", "a", "c"] {
                add(&format!("{kind}{n}"), &x, &x);
            }
        }
        for k in ["s1", "s2", "s3", "s4"] {
            add(k, &"y".repeat(20), &"z".repeat(20));
        }
        add(
            "two",
            "First part here. Second part is longer.",
            "First part here. Second part is longer.",
        );
        for k in ["under_band", "over_band", "in_band_streak", "block_done"] {
            add(k, "One.", "Two.");
        }
        Radio::new(Lines::parse(&text).unwrap(), "test", true)
    }

    fn key(r: &Radio) -> Option<&str> {
        r.current().map(|c| c.key.as_str())
    }

    /// `secs` in 0.05 s steps.
    fn run(r: &mut Radio, secs: f32) {
        for _ in 0..(secs / 0.05).round() as usize {
            r.advance(0.05);
        }
    }

    #[test]
    fn typewriter_at_forty_per_second_then_held() {
        let mut r = radio();
        r.push("s60", Vars::default(), Priority::Story).unwrap();
        assert_eq!((key(&r), r.current().unwrap().shown_chars), (Some("s60"), 0));
        run(&mut r, 0.5);
        assert_eq!(r.current().unwrap().shown_chars, 20);
        assert!(r.current().unwrap().revealing());
        run(&mut r, 1.0);
        assert_eq!(r.current().unwrap().shown_chars, 60);
        assert!(!r.current().unwrap().revealing());
        // 1.5 s reveal + max(3, 60 / 15 = 4) s.
        assert!((line_duration_s(60) - 5.5).abs() < 1e-6);
        run(&mut r, 3.9);
        assert_eq!(key(&r), Some("s60"));
        run(&mut r, 0.2);
        assert_eq!(key(&r), None);
        // Short lines stay at least 3 s.
        assert!((line_duration_s(20) - 3.5).abs() < 1e-6);
        assert_eq!(r.current().map(|c| c.shown_text()), None);
    }

    #[test]
    fn higher_priority_waits_first() {
        let mut r = radio();
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        r.push("s1", Vars::default(), Priority::Story).unwrap();
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        r.push("c20", Vars::default(), Priority::Cue).unwrap();
        // The cue replaced the story line at once; the alert waits first.
        assert_eq!((key(&r), r.queue_len()), (Some("c20"), 2));
        let mut seen = vec![key(&r).unwrap().to_owned()];
        for _ in 0..200 {
            r.advance(0.05);
            if let Some(k) = key(&r).filter(|k| *k != seen.last().unwrap()) {
                seen.push(k.to_owned());
            }
        }
        assert_eq!(seen, ["c20", "a20", "s1"]);
        // An alert does not interrupt a story line, a cue does not interrupt
        // a cue.
        let mut r = radio();
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        assert_eq!(key(&r), Some("s20"));
        let mut r = radio();
        r.push("c20", Vars::default(), Priority::Cue).unwrap();
        r.push("c60", Vars::default(), Priority::Cue).unwrap();
        assert_eq!((key(&r), r.queue_len()), (Some("c20"), 1));
    }

    #[test]
    fn a_cue_cuts_an_alert_after_its_sentence_has_shown_for_one_and_a_half_seconds() {
        let mut r = radio();
        r.push("two", Vars::default(), Priority::Alert).unwrap();
        run(&mut r, 0.2);
        assert_eq!(r.current().unwrap().shown_chars, 8);
        r.push("c20", Vars::default(), Priority::Cue).unwrap();
        // "First part here." (16 characters) is fully shown at 0.4 s; the
        // reveal stops there and the cue takes over at 1.9 s.
        run(&mut r, 0.8);
        assert_eq!((key(&r), r.current().unwrap().shown_chars), (Some("two"), 16));
        assert_eq!(r.current().unwrap().shown_text(), "First part here.");
        run(&mut r, 0.85);
        assert_eq!(key(&r), Some("two"));
        run(&mut r, 0.1);
        assert_eq!(key(&r), Some("c20"));
        // Already shown in full for longer than 1.5 s: cut on the next frame.
        let mut r = radio();
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        run(&mut r, 2.5);
        r.push("c20", Vars::default(), Priority::Cue).unwrap();
        assert_eq!(key(&r), Some("a20"));
        r.advance(0.05);
        assert_eq!(key(&r), Some("c20"));
    }

    #[test]
    fn waiting_lines_below_cue_are_dropped_after_eight_seconds() {
        let mut r = radio();
        // A 150-character cue lasts 3.75 + 10 s.
        r.push("c150", Vars::default(), Priority::Cue).unwrap();
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        r.push("c20", Vars::default(), Priority::Cue).unwrap();
        run(&mut r, 7.9);
        assert_eq!(r.queue_len(), 3);
        run(&mut r, 0.2);
        // The story and the alert are gone, the cue still waits.
        assert_eq!(r.queue_len(), 1);
        run(&mut r, 6.0);
        assert_eq!(key(&r), Some("c20"));
    }

    #[test]
    fn at_most_three_wait() {
        let mut r = radio();
        r.push("c150", Vars::default(), Priority::Cue).unwrap();
        for k in ["s1", "s2", "s3", "s4"] {
            r.push(k, Vars::default(), Priority::Story).unwrap();
        }
        assert_eq!(r.queue_len(), QUEUE_MAX);
        // An alert pushes out the last story line.
        r.push("a20", Vars::default(), Priority::Alert).unwrap();
        let order: Vec<&str> = r.queue.iter().map(|w| w.key.as_str()).collect();
        assert_eq!(order, ["a20", "s1", "s2"]);
    }

    #[test]
    fn never_the_same_variant_twice_in_a_row_and_seeded_by_the_workout() {
        let lines = Lines::load(&lines_path()).unwrap();
        let spoken = |seed: &str| {
            let mut r = Radio::new(lines.clone(), seed, true);
            (0..60)
                .map(|_| {
                    r.say("draft_start", Vars::default()).unwrap();
                    let text = r.current().unwrap().text.clone();
                    run(&mut r, 6.0);
                    text
                })
                .collect::<Vec<_>>()
        };
        let a = spoken("HIT - 2x20x30/30 + 2x12 min tempo");
        for w in a.windows(2) {
            assert_ne!(w[0], w[1]);
        }
        for variant in &lines.get("draft_start").unwrap().text {
            assert!(a.contains(variant), "{variant}");
        }
        assert_eq!(a, spoken("HIT - 2x20x30/30 + 2x12 min tempo"));
        assert_ne!(a, spoken("Sweet spot - 6x5 min"));
        // A reset reseeds: the same sequence again.
        let mut r = Radio::new(lines.clone(), "x", true);
        r.say("halfway", Vars::default()).unwrap();
        let first = r.current().unwrap().text.clone();
        r.reset("x");
        assert_eq!(r.current(), None);
        r.say("halfway", Vars::default()).unwrap();
        assert_eq!(r.current().unwrap().text, first);
    }

    #[test]
    fn cooldowns_keep_it_from_nagging() {
        let mut r = radio();
        r.say("under_band", Vars::default()).unwrap();
        assert_eq!(key(&r), Some("under_band"));
        run(&mut r, 10.0);
        r.say("under_band", Vars::default()).unwrap();
        run(&mut r, 19.9);
        r.say("under_band", Vars::default()).unwrap();
        assert_eq!((key(&r), r.queue_len()), (None, 0));
        run(&mut r, 0.1);
        r.say("under_band", Vars::default()).unwrap();
        assert_eq!(key(&r), Some("under_band"));
        // Each key has its own.
        r.say("over_band", Vars::default()).unwrap();
        assert_eq!(r.queue_len(), 1);
        r.say("in_band_streak", Vars::default()).unwrap();
        run(&mut r, 299.0);
        r.say("in_band_streak", Vars::default()).unwrap();
        assert_eq!(key(&r), None);
        run(&mut r, 1.0);
        r.say("in_band_streak", Vars::default()).unwrap();
        assert_eq!(key(&r), Some("in_band_streak"));
        // Others have none.
        r.say("block_done", Vars::default()).unwrap();
        r.say("block_done", Vars::default()).unwrap();
        assert_eq!(r.queue_len(), 2);
        assert_eq!(
            (
                cooldown_s("under_band"),
                cooldown_s("over_band"),
                cooldown_s("in_band_streak"),
                cooldown_s("halfway")
            ),
            (30.0, 30.0, 300.0, 0.0)
        );
    }

    #[test]
    fn disabled_is_silent_and_unknown_keys_are_refused() {
        let mut r = radio();
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        r.push("s1", Vars::default(), Priority::Story).unwrap();
        r.set_enabled(false);
        assert_eq!((key(&r), r.queue_len()), (None, 0));
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        run(&mut r, 1.0);
        assert_eq!(key(&r), None);
        assert!(r.push("nope", Vars::default(), Priority::Story).is_err());
        r.set_enabled(true);
        r.push("s20", Vars::default(), Priority::Story).unwrap();
        assert_eq!(key(&r), Some("s20"));
        assert!(radio().say("nope", Vars::default()).is_err());
        // Priorities by key.
        assert_eq!(priority_of("step_warning_easier"), Priority::Cue);
        assert_eq!(priority_of("draft_start"), Priority::Cue);
        assert_eq!(priority_of("caught"), Priority::Alert);
        assert_eq!(priority_of("out_of_matches"), Priority::Alert);
        assert_eq!(priority_of("break_start"), Priority::Story);
        assert_eq!(priority_of("dropped"), Priority::Story);
    }
    // ── Triggers, with simulated riders ───────────────────────────────────────

    use crate::race::{Band, Ride};
    use crate::scenario::Scenario;
    use crate::sim::RaceState;
    use crate::terrain::Profile;
    use crate::workout::{StepClass, Workout, WorkoutSession, load_path};

    fn sample(name: &str) -> Workout {
        load_path(&format!("samples/{name}.json")).unwrap()
    }

    /// One simulated ride with the radio on.
    struct Heard {
        /// `(workout time, key, vars)` of every line due.
        due: Vec<(f64, &'static str, Vars)>,
        /// `(workout time, key)` of every line that went on screen.
        shown: Vec<(f64, String)>,
    }

    impl Heard {
        fn count(&self, key: &str) -> usize {
            self.due.iter().filter(|d| d.1 == key).count()
        }

        /// Keys shown in `[from, from + secs)`.
        fn shown_between(&self, from: f64, secs: f64) -> Vec<&str> {
            self.shown
                .iter()
                .filter(|(t, _)| *t >= from && *t < from + secs)
                .map(|(_, k)| k.as_str())
                .collect()
        }
    }

    /// Rides `w` at FTP 200 in 0.1 s frames as the game does (ride,
    /// workout, scenario, triggers, radio), on to 10 s after the finish;
    /// `power(time, band centre)`.
    fn ride_with_radio(w: Workout, mut power: impl FnMut(f64, f32) -> f32) -> Heard {
        let dt = 0.1;
        let free = Band::new(180.0, 220.0).unwrap();
        let (mut session, mut ride) = (WorkoutSession::new(200.0, free), Ride::new(free));
        let (mut profile, mut race) = (Profile::demo(), RaceState::default());
        let name = w.name.clone();
        session.load(w, &mut ride, &mut profile, &mut race);
        let (mut sc, mut triggers) = (Scenario::default(), Triggers::default());
        let mut radio = Radio::new(Lines::load(&lines_path()).unwrap(), &name, true);
        let mut heard = Heard {
            due: Vec::new(),
            shown: Vec::new(),
        };
        let (mut t, mut after) = (0.0_f64, 0.0);
        let mut last_id = None;
        while after < 10.0 && t < 20_000.0 {
            let p = power(t, ride.band.centre_w());
            let ride_events = crate::race::step(&mut ride, p, dt);
            session.advance(&mut ride, dt);
            sc.step(&session, &mut ride, dt);
            let scenario_events = sc.take_events();
            let runner = session.runner.as_ref().unwrap();
            let at = runner.elapsed_s;
            let due = triggers.step(&session, &ride, &sc, &scenario_events, &ride_events, dt);
            heard.due.extend(due.iter().map(|(key, vars)| (at, *key, *vars)));
            radio.say_all(&due).unwrap();
            radio.advance(dt);
            if let Some(c) = radio.current().filter(|c| Some(c.id) != last_id) {
                last_id = Some(c.id);
                heard.shown.push((at, c.key.clone()));
            }
            if runner.finished {
                after += dt;
            }
            t += f64::from(dt);
        }
        heard
    }

    fn exact(_: f64, centre: f32) -> f32 {
        centre
    }

    /// Workout time where the main part starts: the first step that is
    /// not a warm-up step.
    fn main_part_start(w: &Workout) -> f64 {
        let first = w.steps.iter().position(|s| s.class != StepClass::WarmUp).unwrap();
        w.steps[..first].iter().map(|s| f64::from(s.duration_s)).sum()
    }

    #[test]
    fn exact_rider_hears_the_first_ten_minutes_of_the_main_part() {
        let w = sample("hit-30-30");
        let start = main_part_start(&w);
        let heard = ride_with_radio(w, exact);
        let got = heard.shown_between(start, 600.0);
        eprintln!("hit-30-30 from {start} s: {} lines {got:?}", got.len());
        assert_eq!(start, 900.0);
        assert_eq!(
            got,
            [
                "step_warning_harder",
                "step_warning_harder",
                "step_warning_easier",
                "step_warning_pull",
                "pull_start",
                "break_start",
                "step_warning_draft",
                "draft_start",
                "step_warning_pull",
                "pull_start",
                "step_warning_draft",
                "draft_start",
                "step_warning_pull",
                "pull_start",
                "last_rep",
                "step_warning_draft",
                "draft_start",
                "break_survived",
                "block_done",
            ]
        );
        let w = sample("mit-sweetspot-6x5");
        let start = main_part_start(&w);
        let heard = ride_with_radio(w, exact);
        let got = heard.shown_between(start, 600.0);
        eprintln!("mit-sweetspot-6x5 from {start} s: {} lines {got:?}", got.len());
        assert_eq!(start, 600.0);
        assert_eq!(
            got,
            [
                "step_warning_harder",
                "step_warning_harder",
                "step_warning_easier",
                "in_band_streak",
                "step_warning_harder"
            ]
        );
    }

    #[test]
    fn each_trigger_fires_once_per_occasion() {
        let w = sample("hit-30-30");
        let plan = crate::scenario::plan(&w);
        let warnings = (0..w.steps.len())
            .filter(|&i| warning_key(&w.steps, &plan, i, 200.0).is_some())
            .count();
        let heard = ride_with_radio(w, exact);
        let warned: usize = [
            "step_warning_pull",
            "step_warning_draft",
            "step_warning_harder",
            "step_warning_easier",
        ]
        .iter()
        .map(|k| heard.count(k))
        .sum();
        assert_eq!(warned, warnings);
        // 3 + 20 + 20 pulls and drafts, three sets, three blocks of at
        // least 3 reps, four repetition blocks of more than one rep.
        let counts: Vec<usize> = [
            "workout_start",
            "pull_start",
            "draft_start",
            "break_start",
            "break_survived",
            "last_rep",
            "block_done",
            "halfway",
            "workout_done",
        ]
        .iter()
        .map(|k| heard.count(k))
        .collect();
        assert_eq!(counts, [1, 43, 43, 3, 3, 3, 4, 1, 1]);
        // `last_rep` names the rep.
        let reps: Vec<(Option<u32>, Option<u32>)> = heard
            .due
            .iter()
            .filter(|d| d.1 == "last_rep")
            .map(|d| (d.2.rep, d.2.reps))
            .collect();
        assert_eq!(reps, [(Some(3), Some(3)), (Some(20), Some(20)), (Some(20), Some(20))]);
        // The warnings before the first 20× set: 10 s before, the pull's band.
        let (t, _, v) = heard
            .due
            .iter()
            .find(|d| d.1 == "step_warning_pull" && d.0 > 1700.0)
            .unwrap();
        assert!((t - 1730.0).abs() < 0.11, "{t}");
        assert_eq!((v.secs, v.low_w, v.high_w), (Some(10), Some(224.0), Some(276.0)));
        assert_eq!(heard.count("halfway"), 1);
        let (t, _, _) = heard.due.iter().find(|d| d.1 == "halfway").unwrap();
        assert!((t - 4200.0).abs() < 0.11, "{t}");
    }

    #[test]
    fn short_steps_are_warned_five_seconds_before() {
        let step = |d: f32, pct: f32| Step {
            name: String::new(),
            duration_s: d,
            low_pct: pct,
            high_pct: pct,
            class: StepClass::Active,
            block: 0,
            block_kind: BlockKind::Single,
            rep: 1,
            reps: 1,
            cadence_low: None,
            cadence_high: None,
            fixed_rest: false,
        };
        // 19 s and 20 s at 100 %, then 60 s at 50 % and 51 % (within 5 %).
        let w = Workout {
            name: "w".into(),
            date: None,
            source: None,
            steps: vec![
                step(60.0, 50.0),
                step(19.0, 100.0),
                step(20.0, 100.0),
                step(60.0, 50.0),
                step(60.0, 51.0),
            ],
        };
        let heard = ride_with_radio(w, exact);
        let warned: Vec<(f64, &str, Option<u32>)> = heard
            .due
            .iter()
            .filter(|d| d.1.starts_with("step_warning"))
            .map(|d| (d.0, d.1, d.2.secs))
            .collect();
        assert_eq!(warned.len(), 2, "{warned:?}");
        assert!((warned[0].0 - 50.0).abs() < 0.11 && warned[0].1 == "step_warning_harder" && warned[0].2 == Some(10));
        // 100 % to 100 %: none; the 19 s step would be warned 5 s before.
        assert!(
            (warned[1].0 - 89.0).abs() < 0.11 && warned[1].1 == "step_warning_easier" && warned[1].2 == Some(10),
            "{warned:?}"
        );
        let w2 = Workout {
            name: "w".into(),
            date: None,
            source: None,
            steps: vec![step(60.0, 50.0), step(19.0, 100.0), step(60.0, 50.0)],
        };
        let heard = ride_with_radio(w2, exact);
        let short = heard.due.iter().find(|d| d.1 == "step_warning_easier").unwrap();
        assert!(
            (short.0 - 74.0).abs() < 0.11 && short.2.secs == Some(5),
            "{:?}",
            short.0
        );
    }

    #[test]
    fn the_exact_rider_never_hears_under_over_matches_or_caught() {
        for name in [
            "hidits-4x12",
            "hit-30-30",
            "lactate-elimination",
            "lit-1h",
            "mit-sweetspot-6x5",
        ] {
            let heard = ride_with_radio(sample(name), exact);
            for key in ["under_band", "over_band", "match_burned", "caught"] {
                assert_eq!(heard.count(key), 0, "{name} {key}");
            }
            assert_eq!(heard.count("workout_done"), 1, "{name}");
        }
    }

    /// Exact, a quarter under, a quarter over, and a rider who drops off
    /// and comes back: every key is due at least once, and every
    /// placeholder of every variant of a key is supplied by its trigger.
    #[test]
    fn every_key_fires_and_every_placeholder_is_supplied() {
        let lines = Lines::load(&lines_path()).unwrap();
        let riders: [Box<dyn Fn(f64, f32) -> f32>; 4] = [
            Box::new(exact),
            Box::new(|_, c| 0.75 * c),
            Box::new(|_, c| 1.25 * c),
            Box::new(|t, c| if t % 300.0 < 150.0 { 0.6 * c } else { c }),
        ];
        let mut seen = Vec::new();
        for rider in riders {
            for (_, key, vars) in ride_with_radio(sample("hit-30-30"), rider).due {
                for variant in &lines.get(key).unwrap().text {
                    for p in placeholders(variant).unwrap() {
                        assert!(vars.value(p).is_some(), "{key}: {{{p}}} not supplied in {vars:?}");
                    }
                }
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
        }
        for key in KEYS {
            assert!(seen.contains(&key), "{key} never due");
        }
    }
}
