//! User key mappings (`:map` family and config keymaps).

use std::collections::HashMap;

use crate::key::{Key, keys_to_notation, parse_keys};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MapMode {
    Normal,
    Insert,
    Visual,
    Operator,
}

impl MapMode {
    pub fn letter(&self) -> char {
        match self {
            MapMode::Normal => 'n',
            MapMode::Insert => 'i',
            MapMode::Visual => 'v',
            MapMode::Operator => 'o',
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mapping {
    pub lhs: Vec<Key>,
    pub rhs: Vec<Key>,
    pub noremap: bool,
}

pub enum MapMatch {
    None,
    /// Keys are a strict prefix of at least one mapping.
    Prefix,
    /// Exact match; `longer` says whether a longer mapping could still match.
    Exact(Mapping, bool),
}

#[derive(Default)]
pub struct Keymaps {
    maps: HashMap<MapMode, Vec<Mapping>>,
}

impl Keymaps {
    pub fn add(&mut self, mode: MapMode, lhs: &str, rhs: &str, noremap: bool, leader: &str) {
        let lhs = parse_keys(lhs, leader);
        let rhs = parse_keys(rhs, leader);
        if lhs.is_empty() {
            return;
        }
        let v = self.maps.entry(mode).or_default();
        v.retain(|m| m.lhs != lhs);
        v.push(Mapping { lhs, rhs, noremap });
    }

    pub fn remove(&mut self, mode: MapMode, lhs: &str, leader: &str) -> bool {
        let lhs = parse_keys(lhs, leader);
        let v = self.maps.entry(mode).or_default();
        let before = v.len();
        v.retain(|m| m.lhs != lhs);
        v.len() != before
    }

    pub fn lookup(&self, mode: MapMode, keys: &[Key]) -> MapMatch {
        let Some(v) = self.maps.get(&mode) else { return MapMatch::None };
        let mut exact = None;
        let mut longer = false;
        for m in v {
            if m.lhs == keys {
                exact = Some(m.clone());
            } else if m.lhs.len() > keys.len() && m.lhs.starts_with(keys) {
                longer = true;
            }
        }
        match exact {
            Some(m) => MapMatch::Exact(m, longer),
            None if longer => MapMatch::Prefix,
            None => MapMatch::None,
        }
    }

    pub fn list(&self, modes: &[MapMode]) -> Vec<String> {
        let mut out = Vec::new();
        for mode in modes {
            if let Some(v) = self.maps.get(mode) {
                for m in v {
                    out.push(format!(
                        "{}  {:<12} {}{}",
                        mode.letter(),
                        keys_to_notation(&m.lhs),
                        if m.noremap { "* " } else { "  " },
                        keys_to_notation(&m.rhs)
                    ));
                }
            }
        }
        out.sort();
        out
    }
}
