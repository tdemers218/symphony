//! Symphony's user manual. The pages are Markdown files in `docs/manual/`,
//! compiled into the app so the Manual view works offline, and turned into
//! the website by `src/bin/manual-site.rs`. Both read the same text, so the
//! app and the web never disagree.
//!
//! Links between pages are written as `smart-charts.md#draw-the-path`, which
//! also works when the files are read on GitHub. [`resolve`] turns them into
//! a page and an anchor.

pub use pulldown_cmark;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Where the website version lives.
pub const WEB_ROOT: &str = "https://tdemers218.github.io/symphony/manual/";

/// Where the page sources live on GitHub, for "Edit this page".
pub const SOURCE_ROOT: &str = "https://github.com/tdemers218/symphony/blob/main/docs/manual/";

/// The groups of the table of contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Start,
    Make,
    Reference,
    Ahead,
}

impl Part {
    pub const ALL: [Part; 4] = [Part::Start, Part::Make, Part::Reference, Part::Ahead];

    pub fn title(self) -> &'static str {
        match self {
            Part::Start => "Get started",
            Part::Make => "Make a shot",
            Part::Reference => "Reference",
            Part::Ahead => "What's next",
        }
    }
}

/// One page of the manual.
#[derive(Debug)]
pub struct Page {
    /// File name without `.md`; also the web page's name.
    pub slug: &'static str,
    pub part: Part,
    pub markdown: &'static str,
}

macro_rules! page {
    ($slug:literal, $part:ident) => {
        Page {
            slug: $slug,
            part: Part::$part,
            markdown: include_str!(concat!("../../../docs/manual/", $slug, ".md")),
        }
    };
}

/// Every page, in reading order.
pub const PAGES: &[Page] = &[
    page!("index", Start),
    page!("install", Start),
    page!("first-animation", Start),
    page!("workspace", Start),
    page!("drawing", Make),
    page!("colour", Make),
    page!("timing", Make),
    page!("animating", Make),
    page!("rigging", Make),
    page!("smart-charts", Make),
    page!("nodes", Make),
    page!("import-export", Make),
    page!("shortcuts", Reference),
    page!("harmony", Reference),
    page!("projects", Reference),
    page!("tablets", Reference),
    page!("planned", Ahead),
];

/// The Markdown features the manual uses.
pub fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
}

/// The page called `slug`.
pub fn find(slug: &str) -> Option<&'static Page> {
    PAGES.iter().find(|p| p.slug == slug)
}

/// A heading on a page, with the anchor that links to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub level: u8,
    pub title: String,
    pub anchor: String,
}

impl Page {
    /// The page's `# Title`.
    pub fn title(&self) -> &'static str {
        self.markdown
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .unwrap_or(self.slug)
            .trim()
    }

    /// Every heading below the title, in order.
    pub fn sections(&self) -> Vec<Section> {
        let mut out = Vec::new();
        let mut current: Option<(u8, String)> = None;
        for event in Parser::new_ext(self.markdown, options()) {
            match event {
                Event::Start(Tag::Heading { level, .. }) => {
                    current = Some((level_number(level), String::new()));
                }
                Event::Text(t) | Event::Code(t) => {
                    if let Some((_, title)) = current.as_mut() {
                        title.push_str(&t);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((level, title)) = current.take() {
                        if level > 1 {
                            out.push(Section {
                                level,
                                anchor: anchor(&title),
                                title,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Where this page is on the web.
    pub fn web_url(&self, anchor: Option<&str>) -> String {
        let file = if self.slug == "index" {
            String::new()
        } else {
            format!("{}.html", self.slug)
        };
        match anchor {
            Some(a) if !a.is_empty() => format!("{WEB_ROOT}{file}#{a}"),
            _ => format!("{WEB_ROOT}{file}"),
        }
    }

    /// The pages before and after this one.
    pub fn neighbours(&self) -> (Option<&'static Page>, Option<&'static Page>) {
        let i = PAGES.iter().position(|p| p.slug == self.slug);
        match i {
            Some(i) => (
                i.checked_sub(1).and_then(|j| PAGES.get(j)),
                PAGES.get(i + 1),
            ),
            None => (None, None),
        }
    }
}

pub fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// A heading's anchor, the way GitHub makes them: lower case, spaces to
/// hyphens, punctuation dropped. So `smart-charts.md#draw-the-path` works on
/// GitHub, on the website and in the app.
pub fn anchor(title: &str) -> String {
    title
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// Where a link in the manual goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// Another place in the manual.
    Page {
        slug: &'static str,
        anchor: Option<String>,
    },
    /// Somewhere on the web.
    Web(String),
    /// A link to a manual page that doesn't exist (a bug; the tests catch it).
    Broken(String),
}

/// Resolve a link written on page `from`.
pub fn resolve(href: &str, from: &'static str) -> Link {
    if href.starts_with("https://") || href.starts_with("http://") || href.starts_with("mailto:") {
        return Link::Web(href.to_owned());
    }
    let (path, anchor) = match href.split_once('#') {
        Some((p, a)) => (p, Some(a.to_owned())),
        None => (href, None),
    };
    if path.is_empty() {
        return Link::Page { slug: from, anchor };
    }
    let slug = path.trim_start_matches("./").trim_end_matches(".md");
    match find(slug) {
        Some(p) => Link::Page {
            slug: p.slug,
            anchor,
        },
        // Anything else in the repository (DESIGN.md, the plans) opens on
        // GitHub.
        None if path.starts_with("../") => Link::Web(format!(
            "https://github.com/tdemers218/symphony/blob/main/docs/{}",
            path.trim_start_matches("../")
        )),
        None => Link::Broken(href.to_owned()),
    }
}

/// A page's text without Markdown, one entry per section (the part before
/// the first heading is the page's own), for search.
pub fn plain_sections(page: &Page) -> Vec<(Option<Section>, String)> {
    let sections = page.sections();
    let mut out: Vec<(Option<Section>, String)> = vec![(None, String::new())];
    let mut next = sections.into_iter();
    let mut in_heading = false;
    for event in Parser::new_ext(page.markdown, options()) {
        match event {
            Event::Start(Tag::Heading { level, .. }) if level != HeadingLevel::H1 => {
                in_heading = true;
                out.push((next.next(), String::new()));
            }
            Event::Start(Tag::Heading { .. }) => in_heading = true,
            Event::End(TagEnd::Heading(_)) => in_heading = false,
            Event::Text(t) | Event::Code(t) if !in_heading => {
                if let Some((_, text)) = out.last_mut() {
                    text.push_str(&t);
                }
            }
            Event::SoftBreak | Event::HardBreak | Event::End(_) => {
                if let Some((_, text)) = out.last_mut() {
                    if !text.ends_with(' ') {
                        text.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_a_title_and_unique_anchors() {
        for page in PAGES {
            assert!(
                page.markdown.starts_with("# "),
                "{} must start with its # title",
                page.slug
            );
            let mut seen = std::collections::HashSet::new();
            for s in page.sections() {
                assert!(
                    seen.insert(s.anchor.clone()),
                    "{}: two headings make the anchor #{}",
                    page.slug,
                    s.anchor
                );
            }
        }
    }

    #[test]
    fn every_link_between_pages_lands_somewhere() {
        for page in PAGES {
            for event in Parser::new_ext(page.markdown, options()) {
                let Event::Start(Tag::Link { dest_url, .. }) = event else {
                    continue;
                };
                match resolve(&dest_url, page.slug) {
                    Link::Web(_) => {}
                    Link::Broken(href) => panic!("{}: no page for {href}", page.slug),
                    Link::Page { slug, anchor } => {
                        if let Some(a) = anchor {
                            let target = find(slug).expect("resolved");
                            assert!(
                                target.sections().iter().any(|s| s.anchor == a),
                                "{}: {slug} has no heading #{a}",
                                page.slug
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn anchors_match_github() {
        assert_eq!(anchor("Draw the path"), "draw-the-path");
        assert_eq!(
            anchor("Turn and scale (the needle)"),
            "turn-and-scale-the-needle"
        );
        assert_eq!(anchor("Keys & pegs"), "keys--pegs");
    }

    #[test]
    fn links_resolve_to_pages_and_the_web() {
        assert_eq!(
            resolve("smart-charts.md#draw-the-path", "index"),
            Link::Page {
                slug: "smart-charts",
                anchor: Some("draw-the-path".into())
            }
        );
        assert_eq!(
            resolve("#top", "timing"),
            Link::Page {
                slug: "timing",
                anchor: Some("top".into())
            }
        );
        assert!(matches!(resolve("../AI-POLICY.md", "index"), Link::Web(_)));
        assert!(matches!(resolve("nope.md", "index"), Link::Broken(_)));
    }

    #[test]
    fn pages_know_their_web_address() {
        let charts = find("smart-charts").expect("page");
        assert_eq!(
            charts.web_url(Some("draw-the-path")),
            format!("{WEB_ROOT}smart-charts.html#draw-the-path")
        );
        assert_eq!(find("index").expect("page").web_url(None), WEB_ROOT);
    }
}
