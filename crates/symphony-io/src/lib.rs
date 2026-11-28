//! Saving and opening projects (ENGINEERING E7).
//!
//! A project is a folder, `Name.symphony/`:
//!
//! ```text
//! Name.symphony/
//!   project.json          manifest: format version, scenes
//!   scenes/main/scene.json
//!   .backups/             the last 5 saves of each scene file
//! ```
//!
//! Drawings are stored inside scene.json for now. Binary drawing files
//! (`drawings/<id>.svd`) come later, as a format migration (E16).
//!
//! Every write is atomic (E7.3): write a temporary file, flush it to disk,
//! then rename it over the old one, so a crash mid-save never damages the
//! previous save.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use symphony_core::doc::Scene;

/// Bump when the on-disk format changes, and add a migration (E7.2).
pub const FORMAT_VERSION: u32 = 1;
pub const EXTENSION: &str = "symphony";
const KEEP_BACKUPS: usize = 5;

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    format_version: u32,
    app_version: String,
    scenes: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SceneFile {
    format_version: u32,
    scene: Scene,
}

/// Errors in plain words, ready for the user (DESIGN D10.3).
#[derive(Debug)]
pub struct IoError(pub String);

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for IoError {}

fn err(what: &str, path: &Path, e: impl std::fmt::Display) -> IoError {
    IoError(format!("Couldn't {what} {} ({e}).", path.display()))
}

/// Make sure a chosen path ends in `.symphony`.
pub fn with_extension(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some(EXTENSION) {
        path.to_path_buf()
    } else {
        let mut p = path.as_os_str().to_owned();
        p.push(".");
        p.push(EXTENSION);
        PathBuf::from(p)
    }
}

/// The project's display name: the folder name without `.symphony`.
pub fn project_name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

/// Write `bytes` to `path` atomically (E7.3).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let dir = path
        .parent()
        .ok_or_else(|| err("write", path, "no folder"))?;
    fs::create_dir_all(dir).map_err(|e| err("create", dir, e))?;
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp).map_err(|e| err("write", &tmp, e))?;
    f.write_all(bytes).map_err(|e| err("write", &tmp, e))?;
    f.sync_all().map_err(|e| err("write", &tmp, e))?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| err("save", path, e))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Copy the current file into `.backups/` and keep only the newest few (E7.4).
fn backup(project: &Path, file: &Path) {
    if !file.exists() {
        return;
    }
    let dir = project.join(".backups");
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let _ = fs::copy(file, dir.join(format!("{stem}-{stamp:024}.json")));
    let mut olds: Vec<PathBuf> = fs::read_dir(&dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(&stem))
                })
                .collect()
        })
        .unwrap_or_default();
    olds.sort();
    while olds.len() > KEEP_BACKUPS {
        let _ = fs::remove_file(olds.remove(0));
    }
}

/// Save `scene` as the project at `path` (a `.symphony` folder).
pub fn save(scene: &Scene, path: &Path) -> Result<(), IoError> {
    let path = with_extension(path);
    // Imported files travel with the project: copy any the project's
    // media folder doesn't have yet (from wherever the scene found them).
    let media_dir = path.join("media");
    for key in scene.media.keys() {
        let m = &scene.media[key];
        let dest = media_dir.join(&m.file);
        if dest.is_file() {
            continue;
        }
        if let Some(src) = scene.media_path(*key) {
            fs::create_dir_all(&media_dir).map_err(|e| err("save", &media_dir, e))?;
            fs::copy(&src, &dest).map_err(|e| err("save", &dest, e))?;
        }
    }
    let scene_path = path.join("scenes").join("main").join("scene.json");
    backup(&path, &scene_path);
    let file = SceneFile {
        format_version: FORMAT_VERSION,
        scene: scene.clone(),
    };
    let bytes = serde_json::to_vec(&file).map_err(|e| err("save", &scene_path, e))?;
    write_atomic(&scene_path, &bytes)?;
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").into(),
        scenes: vec!["main".into()],
    };
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| err("save", &path, e))?;
    write_atomic(&path.join("project.json"), &bytes)
}

/// Open the project at `path`. Bad files give a readable error, never a
/// panic (E7.6).
pub fn open(path: &Path) -> Result<Scene, IoError> {
    let manifest_path = path.join("project.json");
    let bytes = read_project_file(path, &manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| {
        IoError(format!(
            "{} isn't a Symphony project, or it's damaged ({e}).",
            path.display()
        ))
    })?;
    if manifest.format_version > FORMAT_VERSION {
        return Err(IoError(format!(
            "{} was saved by a newer Symphony ({}). Update Symphony to open it.",
            path.display(),
            manifest.app_version
        )));
    }
    let scene_name = manifest
        .scenes
        .first()
        .ok_or_else(|| IoError("The project has no scenes.".into()))?;
    if scene_name.contains(['/', '\\']) || scene_name.contains("..") {
        return Err(IoError(
            "The project file points outside its folder, so it wasn't opened.".into(),
        ));
    }
    let scene_path = path.join("scenes").join(scene_name).join("scene.json");
    let mut scene = read_scene(path, &scene_path)?;
    // Media files live beside the scene, in the project's media folder.
    scene.media_dirs = vec![path.join("media")];
    Ok(scene)
}

/// Resolve links before reading so a project can only reference its own files.
fn read_project_file(project: &Path, path: &Path) -> Result<Vec<u8>, IoError> {
    let root = project
        .canonicalize()
        .map_err(|e| err("open", project, e))?;
    let file = path.canonicalize().map_err(|e| err("open", path, e))?;
    if !file.starts_with(root) || !file.is_file() {
        return Err(IoError(
            "The project needs a regular file inside its folder.".into(),
        ));
    }
    fs::read(&file).map_err(|e| err("open", path, e))
}

fn read_scene(project: &Path, path: &Path) -> Result<Scene, IoError> {
    let bytes = read_project_file(project, path)?;
    let file: SceneFile = serde_json::from_slice(&bytes).map_err(|e| err("read", path, e))?;
    if file.format_version > FORMAT_VERSION {
        return Err(IoError(
            "The scene needs a newer version of Symphony.".into(),
        ));
    }
    validate(file.scene)
}

/// Reject scenes whose references don't line up, instead of crashing later.
fn validate(mut scene: Scene) -> Result<Scene, IoError> {
    // A media file name is a plain name inside the media folder.
    scene.media.retain(|_, m| {
        !m.file.is_empty() && !m.file.contains(['/', '\\']) && !m.file.starts_with('.')
    });
    // A damaged brush texture only loses its texture: its strokes draw
    // with a plain round tip instead of refusing the whole scene.
    scene.textures.retain(|_, t| t.is_valid());
    let bad = |what: &str| Err(IoError(format!("The scene is damaged: {what}.")));
    if scene.graph.node(scene.composite).is_none() || scene.graph.node(scene.display).is_none() {
        return bad("its output nodes are missing");
    }
    for l in &scene.layers {
        if scene.xsheet.column(l.column).is_none()
            || scene.graph.node(l.node).is_none()
            || scene.graph.node(l.peg).is_none()
        {
            return bad("a layer is missing its column or nodes");
        }
    }
    Ok(scene)
}

/// Crash recovery (E7.5): autosaves live in the app data folder, one file
/// per open scene, until the scene is saved normally.
pub struct Recovery {
    dir: PathBuf,
}

impl Recovery {
    pub fn new(dir: PathBuf) -> Self {
        Recovery { dir }
    }

    fn file(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.json"))
    }

    pub fn write(&self, key: &str, scene: &Scene, origin: Option<&Path>) -> Result<(), IoError> {
        #[derive(Serialize)]
        struct Rec<'a> {
            format_version: u32,
            origin: Option<&'a Path>,
            saved_at: u64,
            scene: &'a Scene,
        }
        let rec = Rec {
            format_version: FORMAT_VERSION,
            origin,
            saved_at: now_secs(),
            scene,
        };
        let bytes = serde_json::to_vec(&rec).map_err(|e| err("autosave", &self.dir, e))?;
        write_atomic(&self.file(key), &bytes)
    }

    pub fn clear(&self, key: &str) {
        let _ = fs::remove_file(self.file(key));
    }

    /// Autosaves left behind by a crash: (key, original path, scene).
    pub fn pending(&self) -> Vec<(String, Option<PathBuf>, Scene)> {
        #[derive(Deserialize)]
        struct Rec {
            format_version: u32,
            origin: Option<PathBuf>,
            scene: Scene,
        }
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                if !e.file_type().ok()?.is_file() || p.extension()?.to_str()? != "json" {
                    return None;
                }
                let key = p.file_stem()?.to_string_lossy().into_owned();
                let rec: Rec = serde_json::from_slice(&fs::read(&p).ok()?).ok()?;
                (rec.format_version <= FORMAT_VERSION).then_some(())?;
                Some((key, rec.origin, validate(rec.scene).ok()?))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphony_core::doc::Settings;
    use symphony_core::history::{Edit, History};

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("symphony-io-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        p
    }

    fn sample() -> Scene {
        let mut s = Scene::new(Settings::default());
        let snap = s.make_layer("Drawing", 0);
        History::default().apply(&mut s, "Add layer", vec![Edit::AddLayer(Box::new(snap))]);
        s
    }

    #[test]
    fn save_and_open_round_trip() {
        let dir = temp("roundtrip");
        let path = dir.join("Walk");
        let mut s = sample();
        s.bookmarks.push(symphony_core::doc::Bookmark {
            id: s.ids.fresh(),
            name: "Hold".into(),
            start: 2,
            end: 5,
            colour: Some([140, 180, 230]),
            notes: "Ease into the next pose".into(),
        });
        save(&s, &path).unwrap();
        let p = with_extension(&path);
        assert!(p.join("project.json").is_file());
        let back = open(&p).unwrap();
        assert_eq!(back.layers, s.layers);
        assert_eq!(back.bookmarks, s.bookmarks);
        assert_eq!(project_name(&p), "Walk");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A Smart Chart saves and reopens with exactly the same motion.
    #[test]
    fn a_chart_reopens_with_the_same_motion() {
        use symphony_core::chart::{Chart, ChartCurve, Role};
        use symphony_core::graph::NodeKind;
        let dir = temp("chart");
        let path = dir.join("Arc");
        let mut s = sample();
        let mut chart = Chart::default();
        chart.set_point(1, [0.0, 0.0], Role::Key);
        chart.set_point(12, [120.0, -70.0], Role::Key);
        chart.set_point(24, [240.0, 10.0], Role::Key);
        chart.points[0].timing.ease_out = 0.6;
        chart.set_rotation(24, 400.0);
        let id = s.graph.add("Hand", NodeKind::Chart(Box::new(chart)));
        save(&s, &path).unwrap();
        let back = open(&with_extension(&path)).unwrap();
        let curve = |scene: &Scene| match &scene.graph.node(id).unwrap().kind {
            NodeKind::Chart(c) => ChartCurve::new(c),
            other => panic!("not a chart: {other:?}"),
        };
        let (a, b) = (curve(&s), curve(&back));
        for f in 0..60 {
            assert_eq!(a.sample(f as f32 * 0.5), b.sample(f as f32 * 0.5));
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn imported_media_is_copied_into_the_project_and_found_again() {
        let dir = temp("media");
        let cache = dir.join("cache");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("abc.png"), b"png bytes").unwrap();
        let mut s = Scene::new(Settings::default());
        s.media.insert(
            7,
            symphony_core::doc::Media {
                kind: symphony_core::doc::MediaKind::Image,
                file: "abc.png".into(),
                name: "Background".into(),
                size: [4, 3],
                seconds: 0.0,
            },
        );
        // A hostile name is dropped on open.
        s.media.insert(
            8,
            symphony_core::doc::Media {
                kind: symphony_core::doc::MediaKind::Image,
                file: "../escape.png".into(),
                name: "x".into(),
                size: [1, 1],
                seconds: 0.0,
            },
        );
        s.media_dirs = vec![cache.clone()];
        let project = dir.join("Walk.symphony");
        save(&s, &project).unwrap();
        assert!(project.join("media").join("abc.png").is_file(), "copied in");
        let back = open(&project).unwrap();
        assert_eq!(
            back.media_path(7),
            Some(project.join("media").join("abc.png"))
        );
        assert!(!back.media.contains_key(&8));
    }

    #[test]
    fn textures_survive_saving_and_bad_ones_are_dropped() {
        let dir = temp("textures");
        let path = dir.join("Tex");
        let mut s = sample();
        let good = symphony_core::stroke::Texture {
            width: 2,
            height: 2,
            pixels: vec![0, 64, 128, 255],
        };
        s.textures.insert(7, good.clone());
        s.textures.insert(
            8,
            symphony_core::stroke::Texture {
                width: 9,
                height: 9,
                pixels: vec![1],
            },
        );
        save(&s, &path).unwrap();
        let back = open(&with_extension(&path)).unwrap();
        assert_eq!(back.textures.get(&7), Some(&good));
        assert!(!back.textures.contains_key(&8));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_five_backups() {
        let dir = temp("backups");
        let path = dir.join("B.symphony");
        let s = sample();
        for _ in 0..8 {
            save(&s, &path).unwrap();
        }
        let n = fs::read_dir(path.join(".backups")).unwrap().count();
        assert_eq!(n, KEEP_BACKUPS);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_newer_formats_and_garbage() {
        let dir = temp("newer");
        let path = dir.join("N.symphony");
        save(&sample(), &path).unwrap();
        let manifest = r#"{"format_version": 999, "app_version": "9.0.0", "scenes": ["main"]}"#;
        fs::write(path.join("project.json"), manifest).unwrap();
        assert!(open(&path).unwrap_err().0.contains("newer Symphony"));
        fs::write(path.join("project.json"), b"nonsense").unwrap();
        assert!(open(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_paths_outside_the_project() {
        let dir = temp("escape");
        let path = dir.join("E.symphony");
        save(&sample(), &path).unwrap();
        let manifest = r#"{"format_version": 1, "app_version": "0", "scenes": ["../../etc"]}"#;
        fs::write(path.join("project.json"), manifest).unwrap();
        assert!(open(&path).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn damaged_scenes_are_rejected_and_good_autosaves_still_recover() {
        let dir = temp("damaged-scenes");
        let project = dir.join("Test.symphony");
        let recovery_dir = dir.join("recovery");
        let recovery = Recovery::new(recovery_dir.clone());
        let scene = sample();
        save(&scene, &project).unwrap();
        recovery.write("good", &scene, None).unwrap();
        let file = project.join("scenes/main/scene.json");
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let mut missing_node = original.clone();
        missing_node["scene"]["composite"] = serde_json::json!(u64::MAX);
        let mut future = original.clone();
        future["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        let mut truncated = serde_json::to_vec(&original).unwrap();
        truncated.pop();
        for bytes in [
            truncated,
            b"[]".to_vec(),
            br#"{"format_version":1e9999,"scene":{}}"#.to_vec(),
            serde_json::to_vec(&missing_node).unwrap(),
            serde_json::to_vec(&future).unwrap(),
        ] {
            fs::write(&file, &bytes).unwrap();
            fs::write(recovery_dir.join("damaged.json"), &bytes).unwrap();
            assert!(open(&project).is_err());
            let pending = recovery.pending();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].0, "good");
        }
        fs::copy(
            recovery_dir.join("good.json"),
            recovery_dir.join("partial.tmp"),
        )
        .unwrap();
        assert_eq!(recovery.pending().len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_scene_link_outside_project() {
        let dir = temp("linked-scene");
        let project = dir.join("Test.symphony");
        save(&sample(), &project).unwrap();
        let file = project.join("scenes/main/scene.json");
        let external = dir.join("external.json");
        fs::rename(&file, &external).unwrap();
        std::os::unix::fs::symlink(external, file).unwrap();
        assert!(open(&project).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recovery_round_trip() {
        let dir = temp("recovery");
        let r = Recovery::new(dir.clone());
        r.write("abc", &sample(), None).unwrap();
        assert_eq!(r.pending().len(), 1);
        r.clear("abc");
        assert!(r.pending().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}

pub mod harmony;
