use std::collections::HashMap;
use std::io::Read;

use anyhow::Context;
use serde::{Deserialize, Serialize};

const DEFAULT_CONFIG: &str = include_str!("../default_config.yaml");

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// length of media info string. If the length of the string exceeds this value,
    /// the text will scroll.
    pub message_display_len: usize,

    /// font index of polybar. This value should be 1 higher than the font value specified
    /// in the polybar config
    pub font_index: u32,

    /// update speed of the text in seconds
    pub update_delay: f32,

    pub control_chars: ControlChars,

    /// icons to display as prefix for specific players
    pub display_player_prefixes: DisplayPlayerPrefixes,

    /// metadata fields based on mpris specification.
    /// See [mpris specification](https://www.freedesktop.org/wiki/Specifications/mpris-spec/metadata/) for more details
    pub metadata_fields: Vec<String>,

    pub metadata_seperator: String,

    /// hide text when no player is available
    pub hide_output: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ControlChars {
    pub play: char,
    pub pause: char,
    pub previous: char,
    pub next: char,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DisplayPlayerPrefixes {
    pub default: char,
    pub specific: HashMap<String, char>,
}

impl Config {
    pub fn load(config: impl Read) -> anyhow::Result<Config> {
        serde_yaml::from_reader(config).context("failed to parse config")
    }

    pub fn default_str() -> &'static str {
        DEFAULT_CONFIG
    }
}
