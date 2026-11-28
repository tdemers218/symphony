//! The Manual view (Help → Manual): the user manual in docs/manual, built
//! into the app so it works offline. The same Markdown makes the website
//! (crates/symphony-manual), so the two never disagree.
//!
//! A page is parsed into blocks once and kept (ENGINEERING E6.1: the UI
//! thread only paints); painting lays out one galley per paragraph, and a
//! link is found from the character under the pointer.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{
    self, text::LayoutJob, Align, Color32, FontFamily, FontId, Rect, Sense, Stroke, TextFormat,
};
use symphony_manual::pulldown_cmark::{Event, Parser, Tag, TagEnd};
use symphony_manual::{Link, Page, Part, PAGES};
use symphony_ui::{icons, icons::Icon, theme};

/// Below this width the page list folds into a menu.
const SIDEBAR_FROM: f32 = 620.0;
const SIDEBAR: f32 = 180.0;
/// Lines longer than this are hard to read.
const MEASURE: f32 = 680.0;

/// What the Manual view shows, and where it has been.
pub struct ManualState {
    page: &'static str,
    /// A heading to scroll to on the next paint.
    pending: Option<String>,
    /// Pages to go back to.
    back: Vec<&'static str>,
    parsed: HashMap<&'static str, Arc<Vec<Block>>>,
}

impl Default for ManualState {
    fn default() -> Self {
        Self {
            page: "index",
            pending: None,
            back: Vec::new(),
            parsed: HashMap::new(),
        }
    }
}

impl ManualState {
    /// Show `slug`, at the heading `anchor` if given.
    pub fn open(&mut self, slug: &'static str, anchor: Option<String>) {
        if slug != self.page {
            self.back.push(self.page);
        }
        self.page = slug;
        self.pending = Some(anchor.unwrap_or_default());
    }

    fn blocks(&mut self, page: &'static Page) -> Arc<Vec<Block>> {
        self.parsed
            .entry(page.slug)
            .or_insert_with(|| Arc::new(parse(page.markdown)))
            .clone()
    }
}

// ---- Parsing ------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
struct Span {
    text: String,
    strong: bool,
    emphasis: bool,
    code: bool,
    link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Block {
    Heading {
        level: u8,
        spans: Vec<Span>,
        anchor: String,
    },
    Paragraph(Vec<Span>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Code(String),
    Table {
        head: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Quote(Vec<Block>),
    Rule,
}

enum Frame {
    Blocks(Vec<Block>),
    List {
        start: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Item(Vec<Block>),
    Quote(Vec<Block>),
    Table {
        head: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
        row: Vec<Vec<Span>>,
    },
}

#[derive(Default)]
struct Inline {
    spans: Vec<Span>,
    strong: u8,
    emphasis: u8,
    links: Vec<String>,
}

impl Inline {
    fn push(&mut self, text: &str, code: bool) {
        let span = Span {
            text: text.to_owned(),
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
            code,
            link: self.links.last().cloned(),
        };
        // Merge runs of the same style, so a paragraph is a few sections.
        if let Some(last) = self.spans.last_mut() {
            if !code
                && !last.code
                && last.strong == span.strong
                && last.emphasis == span.emphasis
                && last.link == span.link
            {
                last.text.push_str(text);
                return;
            }
        }
        self.spans.push(span);
    }

    fn take(&mut self) -> Vec<Span> {
        std::mem::take(&mut self.spans)
    }
}

fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn push_block(stack: &mut [Frame], block: Block) {
    // Blocks only ever land in these; Markdown puts nothing straight
    // inside a list or table.
    if let Some(Frame::Blocks(b) | Frame::Item(b) | Frame::Quote(b)) = stack.last_mut() {
        b.push(block);
    }
}

fn parse(markdown: &str) -> Vec<Block> {
    let mut stack = vec![Frame::Blocks(Vec::new())];
    let mut inline = Inline::default();
    let mut code: Option<String> = None;
    let mut in_image = false;
    let mut in_html = false;
    for event in Parser::new_ext(markdown, symphony_manual::options()) {
        match event {
            Event::Start(Tag::Paragraph | Tag::Heading { .. }) => {
                inline.take();
            }
            Event::End(TagEnd::Paragraph) => {
                let spans = inline.take();
                push_block(&mut stack, Block::Paragraph(spans));
            }
            Event::End(TagEnd::Heading(level)) => {
                let spans = inline.take();
                let anchor = symphony_manual::anchor(&plain(&spans));
                push_block(
                    &mut stack,
                    Block::Heading {
                        level: symphony_manual::level_number(level),
                        spans,
                        anchor,
                    },
                );
            }
            Event::Start(Tag::List(start)) => stack.push(Frame::List {
                start,
                items: Vec::new(),
            }),
            Event::End(TagEnd::List(_)) => {
                if let Some(Frame::List { start, items }) = stack.pop() {
                    push_block(&mut stack, Block::List { start, items });
                }
            }
            Event::Start(Tag::Item) => stack.push(Frame::Item(Vec::new())),
            Event::End(TagEnd::Item) => {
                // Tight list items hold their text without a paragraph.
                let spans = inline.take();
                if let Some(Frame::Item(mut blocks)) = stack.pop() {
                    if !spans.is_empty() {
                        blocks.insert(0, Block::Paragraph(spans));
                    }
                    if let Some(Frame::List { items, .. }) = stack.last_mut() {
                        items.push(blocks);
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => stack.push(Frame::Quote(Vec::new())),
            Event::End(TagEnd::BlockQuote(_)) => {
                if let Some(Frame::Quote(blocks)) = stack.pop() {
                    push_block(&mut stack, Block::Quote(blocks));
                }
            }
            Event::Start(Tag::CodeBlock(_)) => code = Some(String::new()),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(text) = code.take() {
                    push_block(&mut stack, Block::Code(text.trim_end().to_owned()));
                }
            }
            Event::Start(Tag::Table(_)) => stack.push(Frame::Table {
                head: Vec::new(),
                rows: Vec::new(),
                row: Vec::new(),
            }),
            Event::Start(Tag::TableCell) => {
                inline.take();
            }
            Event::End(TagEnd::TableCell) => {
                let spans = inline.take();
                if let Some(Frame::Table { row, .. }) = stack.last_mut() {
                    row.push(spans);
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(Frame::Table { head, row, .. }) = stack.last_mut() {
                    *head = std::mem::take(row);
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(Frame::Table { rows, row, .. }) = stack.last_mut() {
                    rows.push(std::mem::take(row));
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(Frame::Table { head, rows, .. }) = stack.pop() {
                    push_block(&mut stack, Block::Table { head, rows });
                }
            }
            Event::Start(Tag::Strong) => inline.strong += 1,
            Event::End(TagEnd::Strong) => inline.strong = inline.strong.saturating_sub(1),
            Event::Start(Tag::Emphasis) => inline.emphasis += 1,
            Event::End(TagEnd::Emphasis) => inline.emphasis = inline.emphasis.saturating_sub(1),
            Event::Start(Tag::Link { dest_url, .. }) => inline.links.push(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                inline.links.pop();
            }
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::Start(Tag::HtmlBlock) => in_html = true,
            Event::End(TagEnd::HtmlBlock) => in_html = false,
            Event::Text(t) => {
                if let Some(c) = code.as_mut() {
                    c.push_str(&t);
                } else if !in_image && !in_html {
                    inline.push(&t, false);
                }
            }
            Event::Code(t) => inline.push(&t, true),
            Event::SoftBreak => inline.push(" ", false),
            Event::HardBreak => inline.push("\n", false),
            Event::Rule => push_block(&mut stack, Block::Rule),
            _ => {}
        }
    }
    match stack.into_iter().next() {
        Some(Frame::Blocks(b)) => b,
        _ => Vec::new(),
    }
}

// ---- Painting -----------------------------------------------------------

/// What a click on the page asked for.
enum Go {
    Link(String),
}

fn text_colour() -> Color32 {
    theme::TEXT_SECONDARY
}

fn heading_font(level: u8) -> FontId {
    let size = match level {
        1 => 26.0,
        2 => 19.0,
        _ => 15.0,
    };
    FontId::new(size, FontFamily::Name("Bricolage".into()))
}

fn job(
    spans: &[Span],
    font: FontId,
    colour: Color32,
    width: f32,
) -> (LayoutJob, Vec<(usize, Option<String>)>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    // Character offset where each section starts, and its link.
    let mut links = Vec::new();
    let mut chars = 0;
    for s in spans {
        let mut format = TextFormat {
            font_id: if s.code {
                FontId::monospace(font.size - 1.0)
            } else {
                font.clone()
            },
            color: if s.strong || s.code {
                theme::TEXT_PRIMARY
            } else {
                colour
            },
            italics: s.emphasis,
            ..Default::default()
        };
        if s.code {
            format.background = theme::BG_RAISED;
        }
        if s.link.is_some() {
            format.color = theme::ACCENT_SELECT;
            format.underline = Stroke::new(1.0, theme::ACCENT_SELECT.gamma_multiply(0.5));
        }
        links.push((chars, s.link.clone()));
        chars += s.text.chars().count();
        job.append(&s.text, 0.0, format);
    }
    (job, links)
}

/// One paragraph (or heading, or cell). Returns a link if one was clicked.
fn rich(
    ui: &mut egui::Ui,
    spans: &[Span],
    font: FontId,
    colour: Color32,
    width: f32,
) -> (Rect, Option<Go>) {
    let (job, links) = job(spans, font, colour, width);
    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    let link_at = |pos: egui::Pos2| -> Option<String> {
        if !galley.rect.translate(rect.min.to_vec2()).contains(pos) {
            return None;
        }
        let at = galley.cursor_from_pos(pos - rect.min).index.0;
        links
            .iter()
            .rev()
            .find(|(start, _)| *start <= at)
            .and_then(|(_, l)| l.clone())
    };
    let mut go = None;
    if let Some(pos) = response.hover_pos() {
        if let Some(link) = link_at(pos) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked() {
                go = Some(Go::Link(link));
            }
        }
    }
    ui.painter().galley(rect.min, galley, colour);
    (rect, go)
}

struct Painter<'a> {
    pending: &'a mut Option<String>,
    go: Option<Go>,
}

impl Painter<'_> {
    fn blocks(&mut self, ui: &mut egui::Ui, blocks: &[Block], width: f32) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                ui.add_space(8.0);
            }
            self.block(ui, b, width);
        }
    }

    fn take(&mut self, go: Option<Go>) {
        if go.is_some() {
            self.go = go;
        }
    }

    fn block(&mut self, ui: &mut egui::Ui, block: &Block, width: f32) {
        match block {
            Block::Heading {
                level,
                spans,
                anchor,
            } => {
                if *level == 2 {
                    ui.add_space(10.0);
                    let (r, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), Sense::hover());
                    ui.painter().hline(
                        r.x_range(),
                        r.center().y,
                        Stroke::new(1.0, theme::LINE_SUBTLE),
                    );
                    ui.add_space(6.0);
                }
                let (rect, go) = rich(ui, spans, heading_font(*level), theme::TEXT_PRIMARY, width);
                self.take(go);
                if self.pending.as_deref() == Some(anchor.as_str()) {
                    ui.scroll_to_rect(rect.expand(8.0), Some(Align::TOP));
                    *self.pending = None;
                }
            }
            Block::Paragraph(spans) => {
                let (_, go) = rich(ui, spans, theme::body(), text_colour(), width);
                self.take(go);
            }
            Block::List { start, items } => {
                for (n, item) in items.iter().enumerate() {
                    ui.horizontal_top(|ui| {
                        let marker = match start {
                            Some(s) => format!("{}.", s + n as u64),
                            None => "•".to_owned(),
                        };
                        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), Sense::hover());
                        ui.painter().text(
                            r.left_top() + egui::vec2(2.0, 0.0),
                            egui::Align2::LEFT_TOP,
                            marker,
                            theme::body(),
                            theme::TEXT_DISABLED,
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            self.blocks(ui, item, (width - 24.0).max(80.0));
                        });
                    });
                    ui.add_space(3.0);
                }
            }
            Block::Code(text) => {
                egui::Frame::new()
                    .fill(theme::BG_INPUT)
                    .stroke(Stroke::new(1.0, theme::LINE_SUBTLE))
                    .corner_radius(theme::RADIUS_CARD)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(width - 18.0);
                        ui.label(
                            egui::RichText::new(text)
                                .font(theme::numbers())
                                .color(theme::TEXT_PRIMARY),
                        );
                    });
            }
            Block::Quote(blocks) => {
                egui::Frame::new()
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| self.blocks(ui, blocks, width - 22.0));
            }
            Block::Rule => {
                ui.separator();
            }
            Block::Table { head, rows } => self.table(ui, head, rows, width),
        }
    }

    fn table(
        &mut self,
        ui: &mut egui::Ui,
        head: &[Vec<Span>],
        rows: &[Vec<Vec<Span>>],
        width: f32,
    ) {
        let columns = head.len().max(1);
        // Share the width by how much each column holds.
        let mut weight = vec![4.0f32; columns];
        for row in std::iter::once(head).chain(rows.iter().map(Vec::as_slice)) {
            for (c, cell) in row.iter().enumerate().take(columns) {
                let n = plain(cell).chars().count() as f32;
                weight[c] = weight[c].max(n.min(48.0));
            }
        }
        let gap = 14.0;
        let usable = (width - gap * (columns as f32 - 1.0) - 16.0).max(60.0 * columns as f32);
        let total: f32 = weight.iter().sum();
        let widths: Vec<f32> = weight
            .iter()
            .map(|w| (usable * w / total).max(48.0))
            .collect();
        egui::Frame::new()
            .stroke(Stroke::new(1.0, theme::LINE_SUBTLE))
            .corner_radius(theme::RADIUS_CARD)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(columns)
                    .spacing(egui::vec2(gap, 6.0))
                    .striped(true)
                    .show(ui, |ui| {
                        for (c, cell) in head.iter().enumerate() {
                            let mut strong = cell.clone();
                            for s in &mut strong {
                                s.strong = true;
                            }
                            let (_, go) = rich(
                                ui,
                                &strong,
                                theme::dense(),
                                theme::TEXT_PRIMARY,
                                widths[c.min(columns - 1)],
                            );
                            self.take(go);
                        }
                        ui.end_row();
                        for row in rows {
                            for (c, cell) in row.iter().enumerate().take(columns) {
                                let (_, go) =
                                    rich(ui, cell, theme::body(), text_colour(), widths[c]);
                                self.take(go);
                            }
                            ui.end_row();
                        }
                    });
            });
    }
}

fn page_list(ui: &mut egui::Ui, state: &mut ManualState) {
    for part in Part::ALL {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new(part.title().to_uppercase())
                .font(theme::dense())
                .color(theme::TEXT_DISABLED),
        );
        for page in PAGES.iter().filter(|p| p.part == part) {
            let here = page.slug == state.page;
            if ui.selectable_label(here, page.title()).clicked() && !here {
                state.open(page.slug, None);
            }
        }
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut ManualState) {
    let Some(page) = symphony_manual::find(state.page).or_else(|| PAGES.first()) else {
        return;
    };
    let wide = ui.available_width() >= SIDEBAR_FROM;

    // Bar: back, the page menu when narrow, and the web version.
    ui.horizontal(|ui| {
        let back = !state.back.is_empty();
        if icons::button(ui, Icon::PrevFrame, false, back, "Back").clicked() {
            if let Some(slug) = state.back.pop() {
                state.page = slug;
                state.pending = Some(String::new());
            }
        }
        if !wide {
            ui.menu_button(page.title(), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        page_list(ui, state);
                    });
            });
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.hyperlink_to("Open on the web", page.web_url(None))
                .on_hover_text(page.web_url(None));
        });
    });
    ui.separator();

    let blocks = state.blocks(page);
    let mut go = None;
    ui.horizontal_top(|ui| {
        if wide {
            ui.vertical(|ui| {
                ui.set_width(SIDEBAR);
                egui::ScrollArea::vertical()
                    .id_salt("manual-pages")
                    .show(ui, |ui| page_list(ui, state));
            });
            ui.separator();
        }
        let scroll = egui::ScrollArea::vertical()
            .id_salt(("manual-page", page.slug))
            .auto_shrink([false, false]);
        let scroll = if state.pending.as_deref() == Some("") {
            state.pending = None;
            scroll.vertical_scroll_offset(0.0)
        } else {
            scroll
        };
        scroll.show(ui, |ui| {
            ui.vertical(|ui| {
                let width = (ui.available_width() - 12.0).clamp(160.0, MEASURE);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.add_space(8.0);
                let mut painter = Painter {
                    pending: &mut state.pending,
                    go: None,
                };
                painter.blocks(ui, &blocks, width);
                go = painter.go;
                ui.add_space(24.0);
                let (prev, next) = page.neighbours();
                ui.horizontal(|ui| {
                    if let Some(p) = prev {
                        if ui.button(format!("‹ {}", p.title())).clicked() {
                            go = Some(Go::Link(format!("{}.md", p.slug)));
                        }
                    }
                    if let Some(n) = next {
                        if ui.button(format!("{} ›", n.title())).clicked() {
                            go = Some(Go::Link(format!("{}.md", n.slug)));
                        }
                    }
                });
                ui.add_space(16.0);
            })
        });
    });

    if let Some(Go::Link(href)) = go {
        match symphony_manual::resolve(&href, page.slug) {
            Link::Page { slug, anchor } => {
                if slug == page.slug {
                    state.pending = anchor;
                } else {
                    state.open(slug, anchor);
                }
            }
            Link::Web(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            Link::Broken(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_parses_into_blocks_with_its_headings() {
        for page in PAGES {
            let blocks = parse(page.markdown);
            assert!(
                matches!(blocks.first(), Some(Block::Heading { level: 1, .. })),
                "{} starts with its title",
                page.slug
            );
            let anchors: Vec<String> = blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Heading { level, anchor, .. } if *level > 1 => Some(anchor.clone()),
                    _ => None,
                })
                .collect();
            let want: Vec<String> = page.sections().into_iter().map(|s| s.anchor).collect();
            assert_eq!(anchors, want, "{}: headings the app scrolls to", page.slug);
        }
    }

    #[test]
    fn markdown_keeps_links_code_lists_and_tables() {
        let blocks = parse(
            "# T\n\nSee [Timing](timing.md#keys) and `K`.\n\n- one\n- **two**\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n<!-- hidden -->\n",
        );
        let Block::Paragraph(spans) = &blocks[1] else {
            panic!("paragraph: {blocks:?}");
        };
        assert_eq!(spans[1].link.as_deref(), Some("timing.md#keys"));
        assert!(spans.iter().any(|s| s.code && s.text == "K"));
        assert!(matches!(&blocks[2], Block::List { items, .. } if items.len() == 2));
        assert!(
            matches!(&blocks[3], Block::Table { head, rows } if head.len() == 2 && rows.len() == 1)
        );
        assert_eq!(blocks.len(), 4, "the HTML comment isn't shown");
    }
}
