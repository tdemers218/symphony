//! The search box (Tab): a field centred on the mouse that shows results as
//! you type. Ranking and the plain-language rules live in `search.rs`. It
//! opens on the field, then expands to show contextual recommendations.

use eframe::egui::{self, Align2, Id, Pos2, Rect, Vec2};
use symphony_ui::motion;
use symphony_ui::{theme, widgets};

use crate::keymap::{self, Action, Profile};
use crate::search::{self, Command, Entry};
use crate::workspace::View;

pub struct PaletteState {
    pub query: String,
    /// Where it opened (the mouse), in screen points.
    pub at: Pos2,
    /// The view the mouse was over, for ranking.
    pub here: Option<View>,
    selected: usize,
    scroll_offset: f32,
    first_frame: bool,
    opened_at: Option<f64>,
    opened_empty: bool,
    /// Closing: fading out, no longer taking input.
    pub closing: bool,
    /// Waiting for a new shortcut for this action (from the right-click
    /// menu), and the result's title for the prompt.
    recording: Option<(Action, String)>,
    /// The keys just pressed clash with other commands: ask before taking
    /// them.
    clash: Option<Clash>,
    /// While a change displaces other commands' shortcuts: every shortcut
    /// as it was before (for "Undo all"), the commands still waiting for a
    /// new shortcut, and the shortcut the current one lost.
    chain: Option<(Vec<keymap::Remap>, Vec<Action>, String)>,
    /// A short note after a menu choice ("Alt+K now runs Mirror view").
    note: Option<String>,
}

/// A shortcut that clashes, waiting on the user's choice.
#[derive(Clone)]
struct Clash {
    action: Action,
    title: String,
    key: egui::Key,
    mods: egui::Modifiers,
    others: Vec<Action>,
}

/// What a result's right-click menu asked for.
enum Pick {
    Run,
    Record(Action),
    Unbind(Action),
    Reset(Action),
    Copy,
}

impl PaletteState {
    /// Screenshot review: open on the shortcut prompt.
    pub fn record_for_review(&mut self, action: Action, title: &str) {
        self.recording = Some((action, title.to_owned()));
    }

    pub fn new(at: Pos2, here: Option<View>) -> Self {
        PaletteState {
            query: String::new(),
            at,
            here,
            selected: 0,
            scroll_offset: 0.0,
            first_frame: true,
            opened_at: None,
            opened_empty: false,
            closing: false,
            recording: None,
            clash: None,
            chain: None,
            note: None,
        }
    }
}

fn placeholder(here: Option<View>) -> &'static str {
    match here {
        Some(View::Node | View::NodeLibrary) => "Search nodes and effects…",
        Some(View::Drawing) => "Search drawing tools and actions…",
        Some(View::Multiplane) => "Search camera, depth, and staging…",
        Some(View::Camera) => "Search camera, playback, and views…",
        Some(View::Timeline | View::Xsheet) => "Search frames, layers, and timing…",
        Some(View::Colour) => "Search colours and palettes…",
        Some(View::Function) => "Search keyframes and timing…",
        Some(View::ToolProperties | View::LayerProperties) => {
            "Search tools, properties, and settings…"
        }
        Some(View::Library) => "Search the library and views…",
        None => "Search tools, actions, and settings…",
    }
}

const WIDTH: f32 = 560.0;
const ROWS: usize = 16;
const RECOMMEND_AFTER: f64 = 0.5;
const ROW_H: f32 = 34.0;
const FIELD_H: f32 = 30.0;

pub enum Outcome {
    Open,
    /// Fading out; keep calling until `Closed`.
    Closing,
    Closed,
    Chosen(Command, String),
}

fn presence_id() -> Id {
    Id::unique("symphony-search-presence")
}

fn reveal_offset(selected: usize, offset: f32, height: f32) -> f32 {
    let row_top = selected as f32 * ROW_H;
    let row_bottom = row_top + ROW_H;
    if row_top < offset {
        row_top
    } else if row_bottom > offset + height {
        (row_bottom - height).max(0.0)
    } else {
        offset
    }
}

/// Draw the palette. `entries` is everything searchable; `uses` how often
/// each key was chosen.
pub fn ui(
    ctx: &egui::Context,
    st: &mut PaletteState,
    entries: &[Entry],
    uses: &dyn Fn(&str) -> u32,
    profile: Profile,
) -> Outcome {
    // Recording a new shortcut: the next key press (with its modifiers)
    // becomes the shortcut, and it doesn't type or close the box. If those
    // keys belong to other commands, ask first; taking them means picking
    // new keys for those commands in turn, until nothing clashes.
    if st.recording.is_some() || st.clash.is_some() {
        let pressed = ctx.input_mut(|i| {
            let found = i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            });
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            found
        });
        match (pressed, st.recording.clone()) {
            // Esc while asking about a clash: don't take those keys.
            (Some((egui::Key::Escape, _)), None) => clash_cancel(st),
            (Some((egui::Key::Escape, _)), Some((_, title))) => {
                st.recording = None;
                st.note = Some(if st.chain.is_some() {
                    format!("{title} has no shortcut now")
                } else {
                    "Shortcut unchanged".into()
                });
                next_in_chain(st, profile);
            }
            (Some((key, mods)), Some((action, title))) => {
                st.recording = None;
                let others = keymap::clashes(profile, action, key, mods);
                if others.is_empty() {
                    keymap::remap(profile, action, key, mods);
                    st.note = Some(format!(
                        "{} now runs {title}",
                        keymap::label(profile, action)
                    ));
                    next_in_chain(st, profile);
                } else {
                    st.clash = Some(Clash {
                        action,
                        title,
                        key,
                        mods,
                        others,
                    });
                }
            }
            _ => {}
        }
    }
    if st.first_frame {
        ctx.move_to_top(egui::LayerId::new(
            egui::Order::Foreground,
            Id::unique("symphony-search"),
        ));
    }
    let presence_id = presence_id();
    if st.first_frame {
        // Every opening fades in from nothing.
        motion::start(ctx, presence_id, 0.0);
    }
    let shown = motion::presence(ctx, presence_id, !st.closing);
    if st.closing && shown <= 0.0 {
        return Outcome::Closed;
    }

    let typed = !st.query.trim().is_empty();
    let now = ctx.input(|input| input.time);
    if st.opened_at.is_none() {
        st.opened_at = Some(now);
        st.opened_empty = !typed;
    }
    let elapsed = (now - st.opened_at.unwrap_or(now)).max(0.0);
    let recommend = elapsed >= RECOMMEND_AFTER;
    if !typed && !recommend && !st.closing {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            RECOMMEND_AFTER - elapsed,
        ));
    }

    let here = st.here.or(Some(View::Camera));
    let mut results = if typed {
        search::phrase(&st.query)
    } else {
        Vec::new()
    };
    if typed || recommend {
        let hits = search::rank(entries, &st.query, here, uses);
        let limit = if typed { usize::MAX } else { ROWS };
        results.extend(
            hits.iter()
                .take(limit)
                .map(|hit| entries[hit.index].clone()),
        );
    }
    st.selected = st.selected.min(results.len().saturating_sub(1));

    // Tab toggles it closed again; Esc closes it too.
    let (up, down, enter, esc, tab) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.key_pressed(egui::Key::Enter),
            i.key_pressed(egui::Key::Escape),
            !st.first_frame && i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
        )
    });
    // Closing keeps drawing this frame too: returning early here left one
    // blank frame before the fade-out, which looked like fading twice.
    if !st.closing && (esc || tab) {
        st.closing = true;
    }
    if up {
        st.selected = st.selected.saturating_sub(1);
    }
    if down && st.selected + 1 < results.len() {
        st.selected += 1;
    }

    let screen = ctx.content_rect();
    let field_width = WIDTH.min((screen.width() - 36.0).max(40.0));
    let max_list_h = (screen.height() - FIELD_H - 72.0).max(ROW_H);
    let rows = results.len().clamp(1, ROWS);
    let target_height = (rows as f32 * ROW_H).min(max_list_h);
    let list_h = if typed {
        target_height
    } else if !recommend || results.is_empty() {
        0.0
    } else if motion::enabled(ctx) {
        let elapsed = (elapsed - RECOMMEND_AFTER) / motion::Motion::Expand.seconds();
        if elapsed < 1.0 {
            ctx.request_repaint();
        }
        target_height * motion::Motion::Expand.ease(elapsed as f32)
    } else {
        target_height
    };

    // Keep the field in the same place when recommendations expand.
    let reserved_list = if st.opened_empty {
        (ROWS as f32 * ROW_H).min(max_list_h)
    } else {
        target_height
    };
    let opacity = shown;
    let box_w = field_width + 16.0;
    let full_h = FIELD_H + 54.0 + reserved_list;
    let x =
        (st.at.x - box_w / 2.0).clamp(screen.left() + 8.0, (screen.right() - box_w - 8.0).max(8.0));
    let y = (st.at.y + 12.0).clamp(
        screen.top() + 8.0,
        (screen.bottom() - full_h - 8.0).max(8.0),
    );
    let pos = Pos2::new(x, y);

    let mut chosen: Option<usize> = None;
    let mut pick: Option<(usize, Pick)> = None;
    let area = egui::Area::new(Id::unique("symphony-search"))
        // Above panels and floating windows (Foreground, raised when it
        // opens), with its own right-click menus able to open above it.
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .fade_in(false)
        .interactable(!st.closing)
        .show(ctx, |ui| {
            ui.multiply_opacity(opacity);
            // Live blur behind the box (with interface effects on).
            let slot = ui.painter().add(egui::Shape::Noop);
            let frame = egui::Frame::new()
                .fill(egui::Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(1.0, theme::LINE_STRONG))
                .corner_radius(theme::RADIUS_FLOATING)
                .shadow(ui.visuals().popup_shadow)
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(field_width);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut st.query)
                            .hint_text(placeholder(st.here))
                            .desired_width(f32::INFINITY)
                            .font(theme::body()),
                    );
                    if st.first_frame {
                        edit.request_focus();
                        st.first_frame = false;
                    }
                    if edit.changed() {
                        st.selected = 0;
                        st.note = None;
                    }
                    if let Some(c) = st.clash.clone() {
                        // A clash: take the keys (and re-pick for the
                        // others), or don't.
                        let names: Vec<&str> = c.others.iter().map(|a| a.name()).collect();
                        ui.label(
                            egui::RichText::new(format!(
                                "{} is already used by {}.",
                                keymap::combo_label(c.key, c.mods),
                                names.join(", ")
                            ))
                            .font(theme::dense())
                            .color(theme::TEXT_PRIMARY),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .button(format!("Use it for {}, then re-pick for {}", c.title, names.join(", ")))
                                .clicked()
                            {
                                clash_take(st, profile, &c);
                            }
                            if ui.button("Cancel").clicked() {
                                clash_cancel(st);
                            }
                            if st.chain.is_some() && ui.button("Undo all").clicked() {
                                undo_chain(st);
                            }
                        });
                    } else if let Some((_, title)) = &st.recording {
                        let text = match &st.chain {
                            Some((_, _, lost)) => format!(
                                "Now pick a new shortcut for {title} (it lost {lost}). Esc leaves it with none."
                            ),
                            None => format!("Press the new shortcut for {title}. Esc cancels."),
                        };
                        ui.label(
                            egui::RichText::new(text)
                                .font(theme::dense())
                                .color(theme::ACCENT_SELECT),
                        );
                        if st.chain.is_some() && ui.button("Undo all").clicked() {
                            undo_chain(st);
                        }
                    } else if let Some(note) = &st.note {
                        ui.label(
                            egui::RichText::new(note)
                                .font(theme::dense())
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                    if list_h < 0.5 {
                        return;
                    }
                    ui.add_space(4.0);
                    if results.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "Nothing matches. Try other words, like \"rubber\" or \"fill\".",
                            )
                            .font(theme::dense())
                            .color(theme::TEXT_SECONDARY),
                        );
                        return;
                    }
                    // The popup's last measured size can be only one row.
                    // Reserve space so the expanded scroll list receives input.
                    ui.set_min_height(ui.min_size().y + list_h);
                    let mut scroll = egui::ScrollArea::vertical()
                        .id_salt("search-results")
                        // Grow a previously short popup when its query changes.
                        .max_height(list_h)
                        .min_scrolled_height(list_h)
                        .auto_shrink([false, false]);
                    if edit.changed() {
                        scroll = scroll.vertical_scroll_offset(0.0);
                    } else if up || down {
                        scroll = scroll.vertical_scroll_offset(reveal_offset(
                            st.selected,
                            st.scroll_offset,
                            list_h,
                        ));
                    }
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let output = scroll.show_rows(ui, ROW_H - 2.0, results.len(), |ui, range| {
                        for i in range {
                            let e = &results[i];
                            let sel = i == st.selected;
                            let (rect, row) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), ROW_H - 2.0),
                                egui::Sense::click(),
                            );
                            let fill = if sel {
                                theme::ACCENT_SELECT_FILL
                            } else if row.hovered() {
                                theme::BG_HOVER
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            let p = ui.painter();
                            p.rect_filled(rect, theme::RADIUS_CONTROL as f32, fill);
                            let fg = if sel {
                                theme::ON_ACCENT
                            } else {
                                theme::TEXT_PRIMARY
                            };
                            let fg2 = if sel {
                                theme::ON_ACCENT
                            } else {
                                theme::TEXT_SECONDARY
                            };
                            let right = rect.right() - 8.0;
                            let shortcut_w = if e.shortcut.is_empty() { 0.0 } else { 90.0 };
                            widgets::fitted_text(
                                ui,
                                p,
                                Pos2::new(rect.left() + 10.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &e.title,
                                theme::body(),
                                fg,
                                rect.width() - 90.0 - shortcut_w,
                            );
                            p.text(
                                Pos2::new(right - shortcut_w, rect.center().y),
                                Align2::RIGHT_CENTER,
                                e.kind.label(),
                                theme::dense(),
                                fg2,
                            );
                            if !e.shortcut.is_empty() {
                                p.text(
                                    Pos2::new(right, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    &e.shortcut,
                                    theme::dense(),
                                    fg2,
                                );
                            }
                            row.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::Role::Button, true, &e.title)
                            });
                            // Right-click: run it, change or remove its
                            // shortcut, or copy its name.
                            row.context_menu(|ui| {
                                if ui.button("Run").clicked() {
                                    pick = Some((i, Pick::Run));
                                }
                                if let Command::Action(a) = e.command {
                                    ui.separator();
                                    let label = if e.shortcut.is_empty() {
                                        "Add shortcut…"
                                    } else {
                                        "Change shortcut…"
                                    };
                                    if ui.button(label).clicked() {
                                        pick = Some((i, Pick::Record(a)));
                                    }
                                    if !e.shortcut.is_empty()
                                        && ui.button("Remove shortcut").clicked()
                                    {
                                        pick = Some((i, Pick::Unbind(a)));
                                    }
                                    if keymap::is_remapped(profile, a)
                                        && ui
                                            .button(format!("Reset to {} default", profile.title()))
                                            .clicked()
                                    {
                                        pick = Some((i, Pick::Reset(a)));
                                    }
                                }
                                ui.separator();
                                if ui.button("Copy name").clicked() {
                                    pick = Some((i, Pick::Copy));
                                }
                            });
                            if row.clicked() {
                                chosen = Some(i);
                            }
                            if row.hovered() && ui.input(|i| i.pointer.delta() != Vec2::ZERO) {
                                st.selected = i;
                            }
                        }
                    });
                    st.scroll_offset = output.state.clamped_offset().y;
                    let view = output.inner_rect;
                    let fade = 9.0_f32.min(view.height() * 0.5);
                    if st.scroll_offset > 0.5 {
                        let strip = Rect::from_min_size(view.min, Vec2::new(view.width(), fade));
                        theme::fade_strip(ui.painter(), strip, theme::BG_RAISED, true);
                    }
                    if st.scroll_offset + view.height() + 0.5 < output.content_size.y {
                        let min = Pos2::new(view.left(), view.bottom() - fade);
                        let strip = Rect::from_min_max(min, view.max);
                        theme::fade_strip(ui.painter(), strip, theme::BG_RAISED, false);
                    }
                });
            ui.painter().set(
                slot,
                egui::Shape::Vec(crate::backdrop::glass(
                    ui.ctx(),
                    frame.response.rect,
                    f32::from(theme::RADIUS_FLOATING),
                )),
            );
        });
    if st.closing {
        return Outcome::Closing;
    }
    if let Some((i, p)) = pick {
        match p {
            Pick::Run => chosen = Some(i),
            Pick::Record(a) => {
                st.recording = Some((a, results[i].title.clone()));
                st.note = None;
            }
            Pick::Unbind(a) => {
                keymap::unbind(profile, a);
                st.note = Some(format!("{} has no shortcut now", results[i].title));
            }
            Pick::Reset(a) => {
                keymap::reset(profile, a);
                let label = keymap::label(profile, a);
                st.note = Some(if label.is_empty() {
                    format!("{} is back to no shortcut", results[i].title)
                } else {
                    format!("{} is back to {label}", results[i].title)
                });
            }
            Pick::Copy => ctx.copy_text(results[i].title.clone()),
        }
    }
    // Enter while recording (or deciding a clash) isn't "run".
    let enter = enter && st.recording.is_none() && st.clash.is_none();
    if enter && !results.is_empty() {
        chosen = Some(st.selected);
    }
    if let Some(i) = chosen {
        let e = &results[i];
        return Outcome::Chosen(e.command.clone(), e.key.clone());
    }
    // A click anywhere else closes it.
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed())
        && ctx
            .input(|i| i.pointer.interact_pos())
            .is_some_and(|p| !area.response.rect.contains(p));
    if clicked_outside {
        st.closing = true;
        return Outcome::Closing;
    }
    Outcome::Open
}

/// Take the clashing keys: the other commands lose them, and each gets
/// to pick new keys in turn.
fn clash_take(st: &mut PaletteState, profile: Profile, c: &Clash) {
    let before = st
        .chain
        .as_ref()
        .map(|(b, _, _)| b.clone())
        .unwrap_or_else(keymap::remaps);
    let mut waiting = st.chain.take().map(|(_, w, _)| w).unwrap_or_default();
    keymap::remap(profile, c.action, c.key, c.mods);
    waiting.extend(c.others.iter().copied());
    st.clash = None;
    st.chain = Some((before, waiting, keymap::combo_label(c.key, c.mods)));
    next_in_chain(st, profile);
}

/// Don't take those keys: try different keys for the same command.
fn clash_cancel(st: &mut PaletteState) {
    if let Some(c) = st.clash.take() {
        st.recording = Some((c.action, c.title));
        st.note = None;
    }
}

/// The next command that lost its shortcut picks a new one, or the chain
/// ends.
fn next_in_chain(st: &mut PaletteState, _profile: Profile) {
    let Some((before, mut waiting, lost)) = st.chain.take() else {
        return;
    };
    if waiting.is_empty() {
        if st.note.is_none() {
            st.note = Some("No shortcuts clash now".into());
        }
        return;
    }
    let next = waiting.remove(0);
    st.recording = Some((next, next.name().to_owned()));
    st.chain = Some((before, waiting, lost));
}

/// Put every shortcut back as it was before this chain of changes.
fn undo_chain(st: &mut PaletteState) {
    if let Some((before, _, _)) = st.chain.take() {
        keymap::set_remaps(before);
    }
    st.recording = None;
    st.clash = None;
    st.note = Some("All shortcut changes undone".into());
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn clashing_shortcuts_chain_until_nothing_clashes() {
        let _remaps = keymap::TEST_REMAPS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use egui_kittest::kittest::Queryable;
        let profile = Profile::Symphony;
        let (mine, theirs) = (Action::SetPlayStart, Action::ZoomIn);
        keymap::reset(profile, mine);
        keymap::reset(profile, theirs);
        let zoom_key = keymap::binding(profile, theirs).expect("Zoom in has a key");
        let entries = vec![Entry::new(
            "Playback starts here",
            search::Kind::Action,
            Command::Action(mine),
            "playback.start_here",
        )];
        let mut state = PaletteState::new(Pos2::new(400.0, 40.0), None);
        state.query = "playback starts".into();
        let mut h = Harness::builder()
            .with_size(Vec2::new(900.0, 400.0))
            .with_context_setup(|ctx| {
                theme::apply(ctx);
                theme::set_motion(ctx, false);
            })
            .build_ui_state(
                |ui, (state, entries)| {
                    super::ui(ui.ctx(), state, entries, &|_| 0, Profile::Symphony);
                },
                (state, entries),
            );
        h.run_steps(3);
        // Ask for Zoom in's keys: it asks first.
        h.state_mut()
            .0
            .record_for_review(mine, "Playback starts here");
        h.run_steps(1);
        h.key_press_modifiers(zoom_key.mods, zoom_key.key);
        h.run_steps(2);
        assert!(h.state().0.clash.is_some(), "asks before taking them");
        assert_eq!(
            keymap::binding(profile, theirs).map(|b| b.key),
            Some(zoom_key.key),
            "nothing changed yet"
        );
        h.get_by_label("Use it for Playback starts here, then re-pick for Zoom in")
            .click();
        h.run_steps(2);
        assert_eq!(
            keymap::binding(profile, mine).map(|b| b.key),
            Some(zoom_key.key)
        );
        assert_eq!(
            h.state().0.recording.as_ref().map(|r| r.0),
            Some(theirs),
            "Zoom in re-picks"
        );
        // F9 is free: the chain ends.
        h.key_press(egui::Key::F9);
        h.run_steps(2);
        assert_eq!(
            keymap::binding(profile, theirs).map(|b| b.key),
            Some(egui::Key::F9)
        );
        assert!(h.state().0.chain.is_none() && h.state().0.recording.is_none());

        // Again, then Undo all mid-chain puts everything back.
        let before = keymap::remaps();
        h.state_mut()
            .0
            .record_for_review(mine, "Playback starts here");
        h.run_steps(1);
        h.key_press(egui::Key::F9);
        h.run_steps(2);
        h.get_by_label("Use it for Playback starts here, then re-pick for Zoom in")
            .click();
        h.run_steps(2);
        h.get_by_label("Undo all").click();
        h.run_steps(2);
        assert_eq!(keymap::remaps(), before);
        keymap::reset(profile, mine);
        keymap::reset(profile, theirs);
    }

    #[test]
    fn right_click_a_result_to_give_it_a_shortcut() {
        let _remaps = keymap::TEST_REMAPS
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use egui_kittest::kittest::Queryable;
        let profile = Profile::Symphony;
        keymap::reset(profile, Action::MirrorView);
        let entries = vec![Entry::new(
            "Mirror view",
            search::Kind::Action,
            Command::Action(Action::MirrorView),
            "view.mirror",
        )];
        let mut state = PaletteState::new(Pos2::new(400.0, 40.0), None);
        state.query = "mirror".into();
        let mut harness = Harness::builder()
            .with_size(Vec2::new(800.0, 400.0))
            .with_context_setup(|ctx| {
                theme::apply(ctx);
                theme::set_motion(ctx, false);
            })
            .build_ui_state(
                |ui, (state, entries)| {
                    super::ui(ui.ctx(), state, entries, &|_| 0, Profile::Symphony);
                },
                (state, entries),
            );
        harness.run_steps(3);
        harness.get_by_label("Mirror view").click_secondary();
        harness.run_steps(2);
        harness.get_by_label("Add shortcut…").click();
        harness.run_steps(2);
        harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::K);
        harness.run_steps(2);
        let b = keymap::binding(profile, Action::MirrorView).expect("now bound");
        assert_eq!(b.key, egui::Key::K);
        assert!(b.mods.alt);
        assert!(keymap::is_remapped(profile, Action::MirrorView));
        keymap::reset(profile, Action::MirrorView);
        assert!(keymap::binding(profile, Action::MirrorView).is_none());
    }

    #[test]
    fn blank_search_expands_after_half_a_second() {
        use egui_kittest::kittest::Queryable;
        let state = PaletteState::new(Pos2::new(390.0, 54.0), Some(View::Timeline));
        let entries = vec![
            Entry::new(
                "Context result",
                search::Kind::Action,
                Command::GoToFrame(2),
                "context",
            )
            .ctx(&[View::Timeline]),
            Entry::new(
                "Frequent result",
                search::Kind::Action,
                Command::GoToFrame(3),
                "frequent",
            ),
            Entry::new(
                "Other result",
                search::Kind::Action,
                Command::GoToFrame(4),
                "other",
            ),
        ];
        let mut harness = Harness::builder()
            .with_step_dt(0.1)
            .with_size(Vec2::new(800.0, 620.0))
            .with_context_setup(|ctx| {
                theme::apply(ctx);
                theme::set_motion(ctx, false);
            })
            .build_ui_state(
                |ui, (state, entries)| {
                    super::ui(
                        ui.ctx(),
                        state,
                        entries,
                        &|key| if key == "frequent" { 5 } else { 0 },
                        Profile::Symphony,
                    );
                },
                (state, entries),
            );
        harness.run_steps(2);
        assert!(harness.query_by_label("Context result").is_none());
        assert!(harness.query_by_label("Frequent result").is_none());
        harness.run_steps(6);
        harness.get_by_label("Context result");
        harness.get_by_label("Frequent result");
        assert!(harness.query_by_label("Other result").is_none());
    }

    #[test]
    fn search_scrolls_and_reaches_results_beyond_the_first_page() {
        use egui_kittest::kittest::Queryable;
        let mut state = PaletteState::new(Pos2::new(390.0, 54.0), Some(View::Timeline));
        state.query = "Drawing".into();
        let entries: Vec<_> = (1..=32)
            .map(|index| {
                Entry::new(
                    &format!("Drawing entry {index:02}"),
                    search::Kind::Action,
                    Command::GoToFrame(index),
                    &format!("drawing.{index}"),
                )
            })
            .collect();
        let mut harness = Harness::builder()
            .with_size(Vec2::new(800.0, 620.0))
            .with_context_setup(|ctx| {
                theme::apply(ctx);
                theme::set_motion(ctx, false);
            })
            .build_ui_state(
                |ui, (state, entries)| {
                    super::ui(ui.ctx(), state, entries, &|_| 0, Profile::Harmony);
                },
                (state, entries),
            );
        harness.run_steps(3);
        harness.get_by_label("Drawing entry 10");
        harness.state_mut().0.query = "qzxv impossible match".into();
        harness.run_steps(3);
        harness.state_mut().0.query = "Drawing".into();
        harness.run_steps(3);
        harness.get_by_label("Drawing entry 10");
        for _ in 0..20 {
            harness.input_mut().events.push(egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
            harness.input_mut().events.push(egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run_steps(1);
        }
        assert_eq!(harness.state().0.selected, 20);
        let before = harness.state().0.scroll_offset;
        assert!(before > 0.0);
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(Pos2::new(380.0, 205.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -120.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        harness.run_steps(5);
        assert!(harness.state().0.scroll_offset > before);
    }
}
