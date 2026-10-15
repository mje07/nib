//! `~/.config/mdvim/config.toml` loading.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::editor::Editor;
use crate::editor::keymap::MapMode;
use crate::theme::StyleSpec;

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    pub leader: Option<String>,
    #[serde(default)]
    pub options: HashMap<String, toml::Value>,
    #[serde(default)]
    pub keymaps: Keymaps,
    #[serde(default)]
    pub theme: HashMap<String, StyleSpec>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Keymaps {
    #[serde(default)]
    pub normal: HashMap<String, String>,
    #[serde(default)]
    pub insert: HashMap<String, String>,
    #[serde(default)]
    pub visual: HashMap<String, String>,
    #[serde(default)]
    pub operator: HashMap<String, String>,
}

pub fn default_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("mdvim").join("config.toml"))
}

/// Load and apply the config. Returns a list of non-fatal problems.
pub fn load(editor: &mut Editor, path: &Path) -> Vec<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return vec![],
        Err(e) => return vec![format!("config: {}: {e}", path.display())],
    };
    match toml::from_str::<ConfigFile>(&text) {
        Ok(cfg) => apply(editor, cfg),
        Err(e) => vec![format!("config: {}: {}", path.display(), e.message())],
    }
}

pub fn apply(editor: &mut Editor, cfg: ConfigFile) -> Vec<String> {
    let mut errs = Vec::new();
    if let Some(l) = cfg.leader {
        editor.leader = l;
    }
    for (name, val) in cfg.options {
        let res = match val {
            toml::Value::Boolean(b) => editor.options.set_bool(&name, b),
            toml::Value::Integer(n) if n >= 0 => editor.options.set_num(&name, n as usize),
            other => Err(format!("invalid value for {name}: {other}")),
        };
        if let Err(e) = res {
            errs.push(format!("config: {e}"));
        }
    }
    let maps = [
        (MapMode::Normal, cfg.keymaps.normal),
        (MapMode::Insert, cfg.keymaps.insert),
        (MapMode::Visual, cfg.keymaps.visual),
        (MapMode::Operator, cfg.keymaps.operator),
    ];
    for (mode, m) in maps {
        for (lhs, rhs) in m {
            editor.keymaps.add(mode, &lhs, &rhs, true, &editor.leader.clone());
        }
    }
    for (group, spec) in cfg.theme {
        match spec.to_style() {
            Ok(st) => editor.theme.set(&group, st),
            Err(e) => errs.push(format!("config: theme.{group}: {e}")),
        }
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_applies_cleanly() {
        let cfg: ConfigFile = toml::from_str(include_str!("../examples/config.toml")).unwrap();
        let mut ed = Editor::new(80, 24);
        let errs = apply(&mut ed, cfg);
        assert!(errs.is_empty(), "{errs:?}");
        assert!(ed.options.number);
        // The insert-mode "jk" mapping works.
        for k in crate::key::parse_keys("ihijk", " ") {
            ed.handle_key(k);
        }
        assert_eq!(ed.mode, crate::editor::Mode::Normal);
        assert_eq!(ed.buf().contents(), "hi");
    }
}
