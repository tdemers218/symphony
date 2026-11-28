//! Builds the manual's website from docs/manual:
//!
//! ```sh
//! cargo run -p symphony-manual --features site --bin manual-site -- <out>
//! ```
//!
//! writes `<out>/manual/`: a page per Markdown file, a stylesheet, and a
//! search index. The Pages workflow (.github/workflows/pages.yml) puts it
//! next to the presentation page.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use symphony_manual::pulldown_cmark::{html, CowStr, Event, Parser, Tag, TagEnd};
use symphony_manual::{plain_sections, resolve, Link, Page, Part, PAGES, SOURCE_ROOT};

const STYLE: &str = include_str!("site/style.css");
const SCRIPT: &str = include_str!("site/manual.js");

fn main() {
    let Some(out) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: manual-site <output folder>");
        std::process::exit(2);
    };
    if let Err(e) = build(&out.join("manual")) {
        eprintln!("manual-site: {e}");
        std::process::exit(1);
    }
}

fn build(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for page in PAGES {
        std::fs::write(dir.join(file_name(page.slug)), render(page))?;
    }
    std::fs::write(dir.join("style.css"), STYLE)?;
    std::fs::write(dir.join("manual.js"), SCRIPT)?;
    std::fs::write(dir.join("search.js"), search_index())?;
    println!("manual-site: {} pages in {}", PAGES.len(), dir.display());
    Ok(())
}

fn file_name(slug: &str) -> String {
    format!("{slug}.html")
}

fn href(slug: &str, anchor: Option<&str>) -> String {
    let file = if slug == "index" {
        "./".to_owned()
    } else {
        file_name(slug)
    };
    match anchor {
        Some(a) => format!("{file}#{a}"),
        None => file,
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// The page's Markdown as HTML: links between pages point at the .html
/// files, and headings get the anchors the app and GitHub use.
fn body(page: &'static Page) -> String {
    let mut anchors = page.sections().into_iter().map(|s| s.anchor);
    let events = Parser::new_ext(page.markdown, symphony_manual::options())
        .map(|event| match event {
            Event::Start(Tag::Heading {
                level,
                id: None,
                classes,
                attrs,
            }) if level != symphony_manual::pulldown_cmark::HeadingLevel::H1 => {
                Event::Start(Tag::Heading {
                    level,
                    id: anchors.next().map(CowStr::from),
                    classes,
                    attrs,
                })
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest = match resolve(&dest_url, page.slug) {
                    Link::Page { slug, anchor } => href(slug, anchor.as_deref()),
                    Link::Web(url) => url,
                    Link::Broken(url) => url,
                };
                Event::Start(Tag::Link {
                    link_type,
                    dest_url: dest.into(),
                    title,
                    id,
                })
            }
            // The manual has no raw HTML; the only HTML is the note at the top
            // of generated pages, which isn't for readers.
            Event::Html(_) | Event::InlineHtml(_) => Event::Text("".into()),
            other => other,
        })
        // Tables scroll sideways inside a frame on a phone, never the page.
        .flat_map(|event| match event {
            Event::Start(Tag::Table(_)) => {
                vec![Event::Html("<div class=\"table\">".into()), event]
            }
            Event::End(TagEnd::Table) => vec![event, Event::Html("</div>".into())],
            other => vec![other],
        });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

fn nav(current: &Page) -> String {
    let mut out = String::new();
    for part in Part::ALL {
        let _ = write!(out, "<p class=\"part\">{}</p><ul>", escape(part.title()));
        for page in PAGES.iter().filter(|p| p.part == part) {
            let here = page.slug == current.slug;
            let _ = write!(
                out,
                "<li><a href=\"{}\"{}>{}</a>",
                href(page.slug, None),
                if here { " aria-current=\"page\"" } else { "" },
                escape(page.title())
            );
            if here {
                let sections: Vec<_> = page
                    .sections()
                    .into_iter()
                    .filter(|s| s.level == 2)
                    .collect();
                if !sections.is_empty() {
                    out.push_str("<ul class=\"sections\">");
                    for s in sections {
                        let _ = write!(
                            out,
                            "<li><a href=\"#{}\">{}</a></li>",
                            s.anchor,
                            escape(&s.title)
                        );
                    }
                    out.push_str("</ul>");
                }
            }
            out.push_str("</li>");
        }
        out.push_str("</ul>");
    }
    out
}

fn render(page: &'static Page) -> String {
    let (prev, next) = page.neighbours();
    let mut pager = String::new();
    if let Some(p) = prev {
        let _ = write!(
            pager,
            "<a class=\"prev\" href=\"{}\"><span>Previous</span>{}</a>",
            href(p.slug, None),
            escape(p.title())
        );
    }
    if let Some(n) = next {
        let _ = write!(
            pager,
            "<a class=\"next\" href=\"{}\"><span>Next</span>{}</a>",
            href(n.slug, None),
            escape(n.title())
        );
    }
    let title = if page.slug == "index" {
        "Symphony Manual".to_owned()
    } else {
        format!("{} · Symphony Manual", page.title())
    };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="{description}">
<title>{title}</title>
<link rel="icon" href="../presentation/symphony.png" type="image/png">
<link rel="stylesheet" href="style.css">
<script src="search.js" defer></script>
<script src="manual.js" defer></script>
</head>
<body>
<header class="top">
  <a class="brand" href="./">Symphony <span>Manual</span></a>
  <button class="menu" type="button" aria-expanded="false" aria-controls="toc">Contents</button>
  <div class="search">
    <input id="search" type="search" placeholder="Search the manual" autocomplete="off" aria-label="Search the manual">
    <ol id="results" hidden></ol>
  </div>
</header>
<div class="layout">
<nav id="toc" aria-label="Manual pages">{nav}</nav>
<main>
<article>
{body}
</article>
<nav class="pager" aria-label="Previous and next page">{pager}</nav>
<footer>
<a href="{source}{slug}.md">Edit this page on GitHub</a> ·
<a href="../">Symphony home</a> ·
<a href="https://github.com/tdemers218/symphony/releases">Download</a>
<p>Symphony is free software under the GPL-3.0-or-later. Not affiliated with Toon Boom.</p>
</footer>
</main>
</div>
</body>
</html>
"##,
        description = escape(&first_sentence(page)),
        title = escape(&title),
        nav = nav(page),
        body = body(page),
        pager = pager,
        source = SOURCE_ROOT,
        slug = page.slug,
    )
}

fn first_sentence(page: &Page) -> String {
    let text = plain_sections(page)
        .into_iter()
        .next()
        .map(|(_, t)| t)
        .unwrap_or_default();
    let text = text.trim();
    match text.find(". ") {
        Some(i) => text[..=i].to_owned(),
        None => text.chars().take(160).collect(),
    }
}

/// Every section as {page, section, url, text}, for the search box.
fn search_index() -> String {
    let mut out = String::from("window.MANUAL_INDEX = [\n");
    for page in PAGES {
        for (section, text) in plain_sections(page) {
            let (name, url) = match &section {
                Some(s) => (s.title.as_str(), href(page.slug, Some(&s.anchor))),
                None => ("", href(page.slug, None)),
            };
            let _ = writeln!(
                out,
                "{{p:{},s:{},u:{},t:{}}},",
                js(page.title()),
                js(name),
                js(&url),
                js(text
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .as_str())
            );
        }
    }
    out.push_str("];\n");
    out
}

/// A JavaScript string literal.
fn js(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '<' => out.push_str("\\u003c"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
