//! The manual's shortcut page (docs/manual/shortcuts.md) is written from the
//! keymap, so it can't drift from what the keys do. The test fails when the
//! page is stale; run it with `SYMPHONY_UPDATE_MANUAL=1` to rewrite it.

use crate::keymap::{self, Action, Profile, Scope};
use eframe::egui::{Key, Modifiers};

/// Where a shortcut works, in the words the manual uses.
fn place(scope: Scope) -> &'static str {
    match scope {
        Scope::Global => "Anywhere",
        Scope::Canvas => "Drawing and Camera",
        Scope::Camera => "Camera",
        Scope::Viewport => "Drawing, Camera and Node",
        Scope::Timing => "Timeline and Xsheet",
        Scope::Timeline => "Timeline",
        Scope::Node => "Node view",
        Scope::OutsideTiming => "Anywhere but the Timeline and Xsheet",
        Scope::OutsideTimeline => "Anywhere but the Timeline",
    }
}

/// A shortcut as the manual prints it: Windows and Linux names, with the
/// Mac difference stated once at the top of the page.
fn shortcut(profile: Profile, action: Action) -> String {
    match action {
        Action::CreateColourArt if profile == Profile::Harmony => return code("*"),
        // These two differ by OS inside the keymap itself.
        Action::Redo => {
            return match profile {
                Profile::Harmony => {
                    format!("{} (the Control key, also on a Mac)", code("Ctrl+Shift+Z"))
                }
                Profile::Symphony => code("Ctrl+Shift+Z"),
            }
        }
        Action::Delete => {
            return format!("{} (Backspace on Mac)", code("Delete"));
        }
        _ => {}
    }
    let Some(bind) = keymap::default_binding(profile, action) else {
        return "–".into();
    };
    code(&combo(bind.key, bind.mods))
}

fn combo(key: Key, mods: Modifiers) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if mods.command || mods.ctrl || mods.mac_cmd {
        parts.push("Ctrl");
    }
    if mods.alt {
        parts.push("Alt");
    }
    // ! is typed with Shift already.
    if mods.shift && key != Key::Exclamationmark {
        parts.push("Shift");
    }
    parts.push(key.symbol_or_name());
    parts.join("+")
}

/// Inline code that survives a backtick inside it.
fn code(text: &str) -> String {
    if text.contains('`') {
        format!("`` {text} ``")
    } else {
        format!("`{text}`")
    }
}

pub fn markdown() -> String {
    let mut out = String::from(
        "# Keyboard shortcuts\n\n\
         <!-- Written by crates/symphony-app/src/manual_shortcuts.rs from the keymap. Don't edit by hand: run\n\
         SYMPHONY_UPDATE_MANUAL=1 cargo test -p symphony-app manual_shortcuts -->\n\n\
         Symphony has two shortcut profiles. Choose one in **Edit → Preferences → \
         Workflow and shortcuts**.\n\n\
         - **Symphony** (the default) uses plain letters, and the same letter can do \
         different things depending on the view under the pointer: `D` is the Dropper \
         over a drawing and Duplicate Drawing over the Timeline.\n\
         - **Harmony** follows the default shortcuts Toon Boom publishes for Harmony 27. \
         Actions Harmony has no shortcut for are left free; find them with Search \
         (`Tab`).\n\n\
         On a Mac, read `Ctrl` as `Cmd` and `Alt` as `Option`. `Cmd+Shift+Z` also redoes \
         in both profiles.\n\n\
         The tables list the profiles' own shortcuts. To change one, right-click the action \
         in Search (`Tab`) and press the new keys; Preferences shows your current set.\n\n\
         Symphony also keeps a few keys that every tool understands:\n\n\
         | Key | What it does |\n\
         | --- | --- |\n\
         | Hold `Space` | Pan the view with any tool |\n\
         | Hold `Shift+Space` | Rotate the view |\n\
         | `Enter` | Finish a point-by-point selection or an onion-skin alignment |\n\
         | `Ctrl+Enter` | Finish typing with the Text tool (`Escape` cancels) |\n\
         | `Escape` | Cancel a drag in the Node view or an onion-skin alignment; finish editing a Smart Chart |\n\n",
    );
    let order = [
        Scope::Global,
        Scope::Viewport,
        Scope::Canvas,
        Scope::Camera,
        Scope::Timing,
        Scope::Timeline,
        Scope::Node,
    ];
    for scope in order {
        let actions: Vec<Action> = Action::ALL
            .into_iter()
            .filter(|a| keymap::scope(Profile::Symphony, *a) == scope)
            .collect();
        if actions.is_empty() {
            continue;
        }
        out.push_str(&format!("## {}\n\n", place(scope)));
        if scope == Scope::Canvas {
            out.push_str(
                "The Smart Chart keys work only while a chart is being edited; see \
                 [Smart Charts](smart-charts.md#keys).\n\n",
            );
        }
        out.push_str("| Action | Symphony | Harmony |\n| --- | --- | --- |\n");
        for a in actions {
            out.push_str(&format!(
                "| {} | {} | {} |\n",
                a.name(),
                shortcut(Profile::Symphony, a),
                shortcut(Profile::Harmony, a)
            ));
        }
        out.push('\n');
    }
    out.push_str(
        "## Where Harmony-profile shortcuts work\n\n\
         In the Harmony profile, shortcuts work in every view, as they do in Harmony. \
         These are the exceptions:\n\n",
    );
    for a in Action::ALL {
        let scope = keymap::scope(Profile::Harmony, a);
        if scope != Scope::Global && keymap::default_binding(Profile::Harmony, a).is_some() {
            out.push_str(&format!("- {}: {}\n", a.name(), place(scope)));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn manual_shortcuts_page_matches_the_keymap() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/manual/shortcuts.md"
        );
        let want = super::markdown();
        if std::env::var_os("SYMPHONY_UPDATE_MANUAL").is_some() {
            std::fs::write(path, &want).expect("write shortcuts.md");
            return;
        }
        let have = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            have == want,
            "docs/manual/shortcuts.md is out of date with the keymap. Run\n\
             SYMPHONY_UPDATE_MANUAL=1 cargo test -p symphony-app manual_shortcuts"
        );
    }
}
