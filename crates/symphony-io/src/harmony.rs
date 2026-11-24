//! Import the drawing-layer structure of a standalone Harmony scene.
//! Keep the source node inventory alongside the converted layers.
use std::{collections::BTreeMap, fs::File, io::Read, path::Path};

use symphony_core::{
    doc::{Scene, Settings},
    history::{Edit, History},
};

pub const NODE_LIBRARY: &[(&str, &str)] = &[
    ("READ", "Drawing"),
    ("PEG", "Peg"),
    ("COMPOSITE", "Composite"),
    ("DISPLAY", "Display"),
    ("WRITE", "Write"),
];

pub fn import(path: &Path) -> Result<Scene, String> {
    const LIMIT: usize = 64 * 1024 * 1024;
    let file = File::open(path).map_err(|e| format!("Couldn't open the Harmony scene ({e})."))?;
    let mut bytes = Vec::new();
    file.take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("This scene exceeds the 64 MB import limit.".into());
    }
    let xml = std::str::from_utf8(&bytes).map_err(|_| "The scene is not UTF-8 XML.")?;
    parse(xml)
}

fn parse(xml: &str) -> Result<Scene, String> {
    let options = roxmltree::ParsingOptions {
        nodes_limit: 1_000_000,
        ..Default::default()
    };
    let doc = roxmltree::Document::parse_with_options(xml, options)
        .map_err(|e| format!("Couldn't read the Harmony scene XML ({e})."))?;
    let root = doc.root_element();
    if !root.has_tag_name("project") {
        return Err("Expected a Harmony project.".into());
    }
    let scene_node = root
        .descendants()
        .find(|n| n.has_tag_name("scene"))
        .ok_or("No scene was found in this file.")?;
    let mut settings = Settings::default();
    if let Some(length) = scene_node.attribute("nbframes") {
        settings.length = bounded(length, 100_000, "frame count")?;
    }
    if let Some(options) = root.children().find(|n| n.has_tag_name("options")) {
        if let Some(size) = options
            .children()
            .find(|n| n.has_tag_name("resolution"))
            .and_then(|n| n.attribute("size"))
        {
            let (w, h) = size.split_once(',').ok_or("Invalid scene resolution.")?;
            settings.width = bounded(w, 16_384, "width")?;
            settings.height = bounded(h, 16_384, "height")?;
        }
        if let Some(rate) = options
            .children()
            .find(|n| n.has_tag_name("framerate"))
            .and_then(|n| n.attribute("val"))
        {
            settings.fps = rate.parse().map_err(|_| "Invalid scene frame rate.")?;
            if !(0.01..=1000.0).contains(&settings.fps) {
                return Err("Unsupported scene frame rate.".into());
            }
        }
    }
    let columns: BTreeMap<_, _> = scene_node
        .children()
        .filter(|n| n.has_tag_name("columns"))
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("column"))
        .filter_map(|n| Some((n.attribute("name")?, n)))
        .collect();
    let mut nodes = BTreeMap::new();
    let mut layers = Vec::new();
    for module in scene_node
        .descendants()
        .filter(|n| n.has_tag_name("module"))
    {
        let kind = module.attribute("type").unwrap_or("Unknown");
        *nodes.entry(kind.to_owned()).or_default() += 1;
        if kind == "READ" {
            let name = module.attribute("name").unwrap_or("Drawing");
            let column = module
                .descendants()
                .find(|n| {
                    n.has_tag_name("element")
                        && n.parent().is_some_and(|p| p.has_tag_name("drawing"))
                })
                .and_then(|n| n.attribute("col"))
                .unwrap_or(name);
            layers.push((name, columns.get(column).copied()));
        }
    }
    if layers.is_empty() {
        return Err("No drawing layers were found to import.".into());
    }
    if layers.len() > 512 {
        return Err("The scene exceeds the 512-layer import limit.".into());
    }
    // For simple drawing-to-composite networks the leftmost input is in front.
    let display_names: std::collections::BTreeSet<_> = scene_node
        .descendants()
        .filter(|n| n.has_tag_name("module") && n.attribute("type") == Some("DISPLAY"))
        .filter_map(|n| n.attribute("name"))
        .collect();
    let links: Vec<_> = scene_node
        .descendants()
        .filter(|n| n.has_tag_name("link"))
        .collect();
    if let Some(composite) = links
        .iter()
        .find(|l| {
            l.attribute("in")
                .is_some_and(|name| display_names.contains(name))
        })
        .and_then(|l| l.attribute("out"))
    {
        let ports: BTreeMap<_, _> = links
            .iter()
            .filter(|l| l.attribute("in") == Some(composite))
            .filter_map(|l| {
                Some((
                    l.attribute("out")?,
                    l.attribute("inport").unwrap_or("0").parse::<usize>().ok()?,
                ))
            })
            .collect();
        if layers.iter().all(|(name, _)| ports.contains_key(name)) {
            layers.sort_by_key(|(name, _)| ports[name]);
        }
    }
    let mut scene = Scene::new(settings);
    let mut budget: usize = 2_000_000;
    let mut storage_budget: usize = 2_000_000;
    for (name, column) in layers {
        let mut layer = scene.make_layer(name, scene.layers.len());
        let mut extent = 0;
        let mut drawings = BTreeMap::new();
        if let Some(column) = column {
            for seq in column.children().filter(|n| n.has_tag_name("elementSeq")) {
                let value = seq.attribute("val").ok_or("Missing drawing name.")?;
                let id = *drawings.entry(value).or_insert_with(|| {
                    let id = scene.ids.fresh();
                    scene.drawings.insert(
                        id,
                        symphony_core::stroke::Drawing {
                            id,
                            column: layer.column.id,
                            name: value.into(),
                            strokes: Vec::new(),
                            fills: Vec::new(),
                            image: None,
                            text: Vec::new(),
                        },
                    );
                    id
                });
                for run in seq.attribute("exposures").unwrap_or("").split(',') {
                    let (start, end) = run.trim().split_once('-').unwrap_or((run, run));
                    let start = bounded(start, 100_000, "exposure frame")?;
                    let end = bounded(end, 100_000, "exposure frame")?;
                    if end < start {
                        return Err("Invalid exposure range.".into());
                    }
                    budget = budget
                        .checked_sub((end - start + 1) as usize)
                        .ok_or("Too many exposures to import.")?;
                    if end > extent {
                        storage_budget = storage_budget
                            .checked_sub((end - extent) as usize)
                            .ok_or("Exposure sheet is too large.")?;
                        extent = end;
                    }
                    for frame in start..=end {
                        layer.column.set(frame, Some(id));
                    }
                }
            }
        }
        History::default().apply(
            &mut scene,
            "Import layer",
            vec![Edit::AddLayer(Box::new(layer))],
        );
    }
    for link in links {
        let (Some(from), Some(to)) = (link.attribute("out"), link.attribute("in")) else {
            continue;
        };
        let mut groups: Vec<_> = link
            .ancestors()
            .filter(|n| n.has_tag_name("group") || n.has_tag_name("rootgroup"))
            .filter_map(|n| n.attribute("name"))
            .collect();
        groups.reverse();
        let port = |attr| {
            link.attribute(attr)
                .unwrap_or("0")
                .parse()
                .map_err(|_| format!("Invalid connection port on {to}."))
        };
        scene
            .harmony_connections
            .push(symphony_core::doc::SourceConnection {
                group: groups.join("/"),
                from: from.into(),
                output: port("outport")?,
                to: to.into(),
                input: port("inport")?,
            });
    }
    scene.harmony_source = Some(xml.to_owned());
    scene.harmony_nodes = nodes;
    Ok(scene)
}

fn bounded(text: &str, max: u32, field: &str) -> Result<u32, String> {
    text.trim()
        .parse::<u32>()
        .ok()
        .filter(|n| (1..=max).contains(n))
        .ok_or_else(|| format!("Unsupported {field}: {text}."))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_holds_gaps_and_composite_order() {
        let xml = r#"<project><options><resolution size="640,360"/><framerate val="12"/></options><scenes><scene nbframes="6"><columns><column name="c"><elementSeq val="ink" exposures="1-3,5"/></column></columns><rootgroup><module type="READ" name="Back"/><module type="READ" name="Front"><attrs><drawing><element col="c"/></drawing></attrs></module><module type="DISPLAY" name="Out"/><link out="Comp" in="Out"/><link out="Front" in="Comp" inport="0"/><link out="Back" in="Comp" inport="1"/></rootgroup></scene></scenes></project>"#;
        let scene = parse(xml).unwrap();
        assert_eq!(
            (
                scene.settings.width,
                scene.settings.height,
                scene.settings.length
            ),
            (640, 360, 6)
        );
        assert_eq!(scene.settings.fps, 12.0);
        assert_eq!(scene.column(0).unwrap().name, "Front");
        let id = scene.exposed(0, 1).unwrap();
        assert_eq!(scene.drawings[&id].name, "ink");
        assert_eq!(scene.exposed(0, 3), Some(id));
        assert_eq!(scene.exposed(0, 4), None);
        assert_eq!(scene.exposed(0, 5), Some(id));
        assert_eq!(scene.harmony_source.as_deref(), Some(xml));
        assert_eq!(scene.harmony_connections.len(), 3);
        assert_eq!(scene.harmony_connections[2].input, 1);
        assert!(parse(&xml.replace("1-3,5", "5-2")).is_err());
    }

    #[test]
    fn imports_nested_drawing_names_and_counts_unimplemented_nodes() {
        let scene = parse(r#"<project><scene><group><module type="READ" name="Hand &amp; arm"/><module type="BLUR"/></group></scene></project>"#).unwrap();
        assert_eq!(scene.column(0).unwrap().name, "Hand & arm");
        assert_eq!(scene.harmony_nodes["BLUR"], 1);
        assert!(parse("<scene><module").is_err());
        assert!(parse("<scene/>").is_err());
    }
}
