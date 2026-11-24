//! Search everything (Tab): tools, menu actions, views, settings, layers,
//! colours and presets, from one box near the mouse.
//!
//! It understands plain language without any AI (DESIGN §16, AI-POLICY):
//! words are lower-cased and trimmed to a stem, filler words are dropped,
//! a hand-written thesaurus maps everyday words onto Symphony's ("rubber"
//! finds the Eraser, "ghost" finds Onion Skin), typos within an edit or two
//! still match, and a few phrase patterns read numbers ("go to frame 12",
//! "zoom 200%", "brush size 8"). Results that belong to the view under the
//! mouse rank higher, and so do things you use often.

use crate::keymap::Action;
use crate::workspace::View;

/// What a result does when chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Action(Action),
    ShowView(View),
    OpenPreferences,
    OpenExport(symphony_render::Format),
    Setting(Setting),
    GoToFrame(u32),
    SetZoom(f32),
    SetToolSize(f32),
    ToolSizeBy(f32),
    SetLength(u32),
    Timeline(crate::views::timing::TimelineAction),
    InsertInbetweens(u32),
    ReuseDrawing(symphony_core::id::Id),
    SetFrameRate(f32),
    AddSwatch,
    ReduceStrokePoints,
    #[cfg(target_os = "macos")]
    PickScreenColour,
    ToggleAnimate,
    ShowNodeProperties,
    RenderNodePreview,
    ToggleCameraEffects,
    ResetNodeView,
    ResetWorkspace,
    UpdateApp,
    ToggleLayerVisible,
    SelectLayer(usize),
    SelectSwatch(symphony_core::id::Id),
    UsePreset {
        pencil: bool,
        name: String,
    },
    SetArt(symphony_core::stroke::ArtLayer),
    ArrangeNodes,
    ConvertDrawing(symphony_core::graph::NodeId),
    ToggleSmartJoint(symphony_core::graph::NodeId),
    AddDrawingNode,
    AddNode(symphony_core::graph::NodeKind),
}

/// Settings that search can flip directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    BrushSizeCursor,
    SizeFollowsZoom,
    OverlayUnderlay,
    InterfaceAnimations,
    InterfaceEffects,
    UseNoAi,
    ProfileHarmony,
    ProfileSymphony,
    StrokeDetail,
    StageColour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Tool,
    Action,
    View,
    Setting,
    Layer,
    Colour,
    Preset,
    Node,
    Go,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Tool => "Tool",
            Kind::Action => "Action",
            Kind::View => "View",
            Kind::Setting => "Setting",
            Kind::Layer => "Layer",
            Kind::Colour => "Colour",
            Kind::Preset => "Preset",
            Kind::Node => "Node",
            Kind::Go => "Go",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub title: String,
    pub kind: Kind,
    /// Extra words people might use for it.
    pub keywords: Vec<String>,
    /// Views where it is most useful (ranked up when the mouse is there).
    pub context: Vec<View>,
    pub command: Command,
    /// Stable key for remembering how often it is used.
    pub key: String,
    /// Shortcut text to show, if any.
    pub shortcut: String,
}

impl Entry {
    pub fn new(title: &str, kind: Kind, command: Command, key: &str) -> Self {
        Entry {
            title: title.to_owned(),
            kind,
            keywords: Vec::new(),
            context: Vec::new(),
            command,
            key: key.to_owned(),
            shortcut: String::new(),
        }
    }

    pub fn words(mut self, w: &str) -> Self {
        self.keywords
            .extend(w.split_whitespace().map(str::to_owned));
        self
    }

    pub fn ctx(mut self, views: &[View]) -> Self {
        self.context.extend_from_slice(views);
        self
    }
}

/// Words that carry no meaning in a request.
const STOP: &[&str] = &[
    "a", "an", "the", "to", "of", "for", "in", "on", "at", "my", "me", "i", "it", "this", "that",
    "please", "how", "do", "can", "you", "want", "wanna", "would", "like", "where", "is", "are",
    "with", "and", "some", "go", "set", "let", "get", "find", "use", "using", "show", "open",
    "put", "make", "so", "be", "should", "could", "will", "just", "what", "which", "into",
];

/// Groups of words that mean the same thing here. A query word in a group
/// also searches for the others.
const THESAURUS: &[&[&str]] = &[
    &["erase", "eraser", "rubber", "wipe", "scrub"],
    &["delete", "remove", "clear", "trash", "bin", "discard"],
    &["paint", "fill", "bucket", "colour", "color", "flood", "ink"],
    &[
        "dropper",
        "eyedropper",
        "picker",
        "pick",
        "sample",
        "eyedrop",
    ],
    &["brush", "draw", "paintbrush", "stroke", "sketch"],
    &["pencil", "line", "outline", "lineart"],
    &["select", "selection", "choose", "lasso", "marquee", "grab"],
    &[
        "transform",
        "move",
        "scale",
        "resize",
        "rotate",
        "position",
        "peg",
    ],
    &["hand", "pan", "scroll", "drag"],
    &["zoom", "magnify", "enlarge", "closer"],
    &["undo", "revert", "back", "oops", "mistake"],
    &["redo", "again", "forward"],
    &["onion", "ghost", "ghosting", "lighttable", "skin"],
    &["play", "playback", "preview", "watch", "run", "animate"],
    &["pause", "stop"],
    &[
        "export", "render", "movie", "video", "gif", "mp4", "png", "output", "share",
    ],
    &["save", "store", "keep"],
    &["open", "load", "import"],
    &["new", "create", "add", "make", "blank", "empty"],
    &["layer", "column", "track"],
    &["frame", "cell", "time", "timing"],
    &["exposure", "hold", "length", "duration", "longer", "extend"],
    &["drawing", "picture", "image", "art", "artwork"],
    &["view", "window", "panel", "canvas", "screen"],
    &["rotate", "rotation", "turn", "spin", "tilt", "angle"],
    &[
        "setting",
        "settings",
        "preference",
        "preferences",
        "option",
        "options",
        "config",
    ],
    &["hide", "visible", "visibility", "show", "invisible", "eye"],
    &["flip", "mirror", "reverse"],
    &["swap", "substitute", "substitution", "replace", "cycle"],
    &[
        "keyframe", "key", "curve", "function", "graph", "ease", "easing", "bezier",
    ],
    &["gap", "close", "closegap", "seal", "hole"],
    &[
        "size",
        "thickness",
        "width",
        "thick",
        "thin",
        "bigger",
        "smaller",
        "larger",
        "weight",
    ],
    &[
        "smooth",
        "smoothness",
        "smoothing",
        "stabilise",
        "stabilize",
        "jitter",
        "shaky",
        "wobbly",
        "detail",
        "points",
    ],
    &["grey", "gray", "background", "stage", "canvas", "paper"],
    &["shortcut", "keybind", "hotkey", "keyboard", "key"],
    &["copy", "duplicate", "clone"],
    &["paste", "insert"],
    &["cut"],
    &["protect", "lock", "locked", "protected"],
    &["preset", "presets", "brushes", "pencils"],
    &["next", "forward", "following", "after"],
    &["previous", "prev", "before", "last", "earlier"],
    &["first", "start", "beginning"],
    &["end", "final"],
    &["node", "nodes", "rig", "rigging", "network"],
    &["xsheet", "exposure", "sheet", "dopesheet"],
    &["timeline", "time", "track"],
];

/// Trim a word to a rough stem so "erasing", "erased" and "erases" meet.
pub fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    for suf in ["ings", "ing", "ers", "ed", "es", "er", "s"] {
        if w.len() > suf.len() + 2 && w.ends_with(suf) {
            let base = &w[..w.len() - suf.len()];
            // "erasing" -> "eras" would miss "erase": keep a silent e.
            return base.trim_end_matches('e').to_owned();
        }
    }
    w.trim_end_matches('e').to_owned()
}

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// The words of a query that matter: no filler, stemmed.
pub fn query_terms(q: &str) -> Vec<String> {
    let all = tokens(q);
    let kept: Vec<String> = all
        .iter()
        .filter(|w| !STOP.contains(&w.as_str()))
        .cloned()
        .collect();
    // "show" or "open" alone is still a request worth searching.
    let words = if kept.is_empty() { all } else { kept };
    words.iter().map(|w| stem(w)).collect()
}

/// Other stems that mean the same as `term`.
fn synonyms(term: &str) -> Vec<String> {
    let mut out = Vec::new();
    for group in THESAURUS {
        if group.iter().any(|w| stem(w) == term) {
            out.extend(group.iter().map(|w| stem(w)));
        }
    }
    out.retain(|w| w != term);
    out.sort();
    out.dedup();
    out
}

/// Damerau-Levenshtein distance (optimal string alignment), capped.
fn distance(a: &str, b: &str, cap: usize) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// How well one query term matches one word of an entry, 0..=1.
fn term_match(q: &str, w: &str) -> f32 {
    if q == w {
        return 1.0;
    }
    if q.len() >= 2 && w.starts_with(q) {
        return 0.85;
    }
    if w.len() >= 3 && q.starts_with(w) {
        return 0.6;
    }
    let cap = match q.len() {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    };
    if cap > 0 && distance(q, w, cap) <= cap {
        return 0.65;
    }
    0.0
}

/// A scored result.
#[derive(Clone, Debug)]
pub struct Hit {
    pub index: usize,
    pub score: f32,
}

/// Rank `entries` for `query`. `here` is the view under the mouse; `uses`
/// says how often each entry key was chosen before.
pub fn rank(
    entries: &[Entry],
    query: &str,
    here: Option<View>,
    uses: &dyn Fn(&str) -> u32,
) -> Vec<Hit> {
    let terms = query_terms(query);
    let mut hits: Vec<Hit> = Vec::new();
    for (index, e) in entries.iter().enumerate() {
        let title: Vec<String> = tokens(&e.title).iter().map(|w| stem(w)).collect();
        let extra: Vec<String> = e.keywords.iter().map(|w| stem(w)).collect();
        let mut score = 0.0f32;
        let mut matched = 0usize;
        for t in &terms {
            let direct = title
                .iter()
                .map(|w| term_match(t, w) * 3.0)
                .chain(extra.iter().map(|w| term_match(t, w) * 2.0))
                .fold(0.0f32, f32::max);
            let via_synonym = synonyms(t)
                .iter()
                .flat_map(|s| {
                    title
                        .iter()
                        .map(move |w| if s == w { 2.4 } else { 0.0 })
                        .chain(extra.iter().map(move |w| if s == w { 1.6 } else { 0.0 }))
                })
                .fold(0.0f32, f32::max);
            let best = direct.max(via_synonym);
            if best > 0.0 {
                matched += 1;
            }
            score += best;
        }
        if !terms.is_empty() {
            if matched == 0 {
                continue;
            }
            // Most of the words should land somewhere.
            score *= matched as f32 / terms.len() as f32;
            let whole = query.trim().to_lowercase();
            if !whole.is_empty() && e.title.to_lowercase().starts_with(&whole) {
                score += 2.0;
            }
        }
        if here.is_some_and(|v| e.context.contains(&v)) {
            score += if terms.is_empty() { 2.0 } else { 0.9 };
        }
        score += (uses(&e.key) as f32 + 1.0).ln() * 0.4;
        if terms.is_empty() && score <= 0.0 {
            continue;
        }
        hits.push(Hit { index, score });
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    // With words typed, drop weak matches far below the best one.
    if !terms.is_empty() {
        if let Some(best) = hits.first().map(|h| h.score) {
            hits.retain(|h| h.score >= best * 0.4);
        }
    }
    hits
}

/// Commands read from phrases with a number in them: "go to frame 12",
/// "zoom 200%", "brush size 8", "scene length 96". Shown above the
/// ranked results.
pub fn phrase(query: &str) -> Vec<Entry> {
    let words = tokens(query);
    let number = words
        .iter()
        .find_map(|w| w.trim_end_matches('%').parse::<f32>().ok());
    let Some(n) = number.filter(|n| n.is_finite() && *n >= 0.0) else {
        return Vec::new();
    };
    let has = |options: &[&str]| words.iter().any(|w| options.contains(&w.as_str()));
    if has(&["insert", "add"])
        && has(&[
            "frame",
            "frames",
            "inbetween",
            "inbetweens",
            "between",
            "betweens",
        ])
    {
        let count = (n as u32).clamp(1, 100_000);
        return vec![Entry::new(
            &format!("Insert {count} in-between frames"),
            Kind::Go,
            Command::InsertInbetweens(count),
            "phrase.insert_frames",
        )];
    }
    if has(&["fps"]) || (has(&["frame"]) && has(&["rate"])) {
        if [12.0, 24.0, 30.0, 48.0].contains(&n) {
            return vec![Entry::new(
                &format!("Set Scene to {n} FPS"),
                Kind::Go,
                Command::SetFrameRate(n),
                "phrase.fps",
            )];
        }
        return Vec::new();
    }
    if has(&["reuse", "drawing"]) && !has(&["go", "jump"]) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if has(&["zoom", "magnify", "%"]) || query.contains('%') {
        let pct = n.clamp(5.0, 3200.0);
        out.push(Entry::new(
            &format!("Zoom to {pct}%"),
            Kind::Go,
            Command::SetZoom(pct / 100.0),
            "phrase.zoom",
        ));
    }
    if has(&["size", "thickness", "width", "brush", "pencil", "eraser"]) {
        out.push(Entry::new(
            &format!("Set the tool size to {n}"),
            Kind::Go,
            Command::SetToolSize(n.clamp(0.5, 2000.0)),
            "phrase.size",
        ));
    }
    if has(&["length", "duration", "long"]) {
        out.push(Entry::new(
            &format!("Set the scene length to {} frames", n as u32),
            Kind::Go,
            Command::SetLength((n as u32).clamp(1, 100_000)),
            "phrase.length",
        ));
    }
    if out.is_empty() || has(&["frame", "go", "jump"]) {
        out.insert(
            0,
            Entry::new(
                &format!("Go to frame {}", (n as u32).max(1)),
                Kind::Go,
                Command::GoToFrame((n as u32).max(1)),
                "phrase.frame",
            ),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeline_phrases_do_not_steal_named_drawing_searches() {
        assert!(phrase("reuse drawing 12").is_empty());
        assert!(matches!(
            phrase("insert 3 frames")[0].command,
            Command::InsertInbetweens(3)
        ));
        assert!(matches!(
            phrase("set scene to 24 fps")[0].command,
            Command::SetFrameRate(24.0)
        ));
    }

    fn catalogue() -> Vec<Entry> {
        vec![
            Entry::new(
                "Eraser",
                Kind::Tool,
                Command::Action(Action::ToolEraser),
                "tool.eraser",
            )
            .ctx(&[View::Camera, View::Drawing]),
            Entry::new(
                "Paint",
                Kind::Tool,
                Command::Action(Action::ToolPaint),
                "tool.paint",
            )
            .ctx(&[View::Camera, View::Drawing]),
            Entry::new(
                "Onion skin",
                Kind::Action,
                Command::Action(Action::ToggleOnionSkin),
                "onion",
            ),
            Entry::new("Undo", Kind::Action, Command::Action(Action::Undo), "undo"),
            Entry::new(
                "Add drawing layer",
                Kind::Action,
                Command::Action(Action::AddLayer),
                "layer.add",
            )
            .ctx(&[View::Timeline, View::Xsheet]),
            Entry::new(
                "Export",
                Kind::Action,
                Command::OpenExport(symphony_render::Format::Mp4Movie),
                "export",
            )
            .words("movie gif png"),
            Entry::new(
                "Preferences",
                Kind::Setting,
                Command::OpenPreferences,
                "prefs",
            ),
            Entry::new(
                "Stroke detail",
                Kind::Setting,
                Command::Setting(Setting::StrokeDetail),
                "detail",
            )
            .words("points smoothing"),
            Entry::new(
                "Rotate view CW",
                Kind::Action,
                Command::Action(Action::RotateCw),
                "rotate",
            ),
        ]
    }

    fn top(q: &str, here: Option<View>) -> String {
        let c = catalogue();
        let hits = rank(&c, q, here, &|_| 0);
        hits.first()
            .map(|h| c[h.index].title.clone())
            .unwrap_or_default()
    }

    #[test]
    fn plain_words_find_harmony_names() {
        assert_eq!(top("rubber", None), "Eraser");
        assert_eq!(top("fill bucket", None), "Paint");
        assert_eq!(top("ghosting", None), "Onion skin");
        assert_eq!(top("oops", None), "Undo");
        assert_eq!(top("how do I make a movie", None), "Export");
        assert_eq!(top("settings", None), "Preferences");
        assert_eq!(top("my lines are wobbly", None), "Stroke detail");
        assert_eq!(top("turn the canvas", None), "Rotate view CW");
        assert_eq!(top("new layer", None), "Add drawing layer");
    }

    #[test]
    fn typos_still_match() {
        assert_eq!(top("earser", None), "Eraser");
        assert_eq!(top("onoin", None), "Onion skin");
        assert_eq!(top("expotr", None), "Export");
    }

    #[test]
    fn the_view_under_the_mouse_ranks_its_tools_first() {
        let c = catalogue();
        let hits = rank(&c, "", Some(View::Timeline), &|_| 0);
        assert_eq!(c[hits[0].index].title, "Add drawing layer");
        let hits = rank(&c, "", Some(View::Camera), &|_| 0);
        assert!(["Eraser", "Paint"].contains(&c[hits[0].index].title.as_str()));
    }

    #[test]
    fn frequent_choices_rise() {
        let c = catalogue();
        let hits = rank(&c, "", None, &|k| if k == "undo" { 50 } else { 0 });
        assert_eq!(c[hits[0].index].title, "Undo");
    }

    #[test]
    fn phrases_with_numbers() {
        assert_eq!(phrase("go to frame 12")[0].command, Command::GoToFrame(12));
        assert_eq!(phrase("24")[0].command, Command::GoToFrame(24));
        assert!(phrase("zoom 200%")
            .iter()
            .any(|e| e.command == Command::SetZoom(2.0)));
        assert!(phrase("brush size 8")
            .iter()
            .any(|e| e.command == Command::SetToolSize(8.0)));
        assert!(phrase("scene length 96")
            .iter()
            .any(|e| e.command == Command::SetLength(96)));
        assert!(phrase("eraser").is_empty());
    }

    #[test]
    fn nonsense_finds_nothing() {
        let c = catalogue();
        assert!(rank(&c, "qzxv", None, &|_| 0).is_empty());
    }
}
