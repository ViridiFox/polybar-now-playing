//! Rust rewrite of [Now playing python script](https://github.com/d093w1z/polybar-now-playing)

mod config;

use config::Config;

use std::{cmp::Ordering, collections::HashMap, fs::File, io::Write, path::Path};

use anyhow::Context;
use futures::stream::StreamExt;
use signal_hook::consts::signal::*;
use signal_hook_tokio::Signals;
use zbus::{
    dbus_proxy,
    fdo::{self, DBusProxy},
    names::OwnedBusName,
    zvariant::Value,
    Connection,
};

const CONFIG_PATH: &str = "/home/viridi/.config/polybar/scripts/now_playing.yaml";

/// gets the player name from the reverse domain name
fn get_name(player_name: impl AsRef<str>) -> String {
    player_name
        .as_ref()
        .split('.')
        .skip(3)
        .collect::<Vec<&str>>()
        .join(".")
}

fn visual_len(string: impl AsRef<str>) -> usize {
    unicode_width::UnicodeWidthStr::width_cjk(string.as_ref())
}

fn make_visual_len(text: impl AsRef<str>, visual_desired_length: usize) -> String {
    let mut visual_length = 0;
    let mut altered_text = String::new();

    for char in text.as_ref().chars() {
        let width = unicode_width::UnicodeWidthChar::width_cjk(char).unwrap_or(0);
        if visual_length + width < visual_desired_length {
            visual_length += width;
            altered_text += &char.to_string();
        } else {
            break;
        }
    }

    if visual_length < visual_desired_length {
        altered_text += &" ".repeat(visual_desired_length - visual_length)
    }

    altered_text
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::Array(arr) => value_to_string(&arr.get()[0]),
        Value::U8(x) => x.to_string(),
        Value::U16(x) => x.to_string(),
        Value::U32(x) => x.to_string(),
        Value::U64(x) => x.to_string(),
        Value::I16(x) => x.to_string(),
        Value::I32(x) => x.to_string(),
        Value::I64(x) => x.to_string(),
        Value::Bool(x) => x.to_string(),
        Value::F64(x) => x.to_string(),
        Value::Str(x) => x.to_string(),
        v => unimplemented!("unsupported conversion to string for {:?}", v),
    }
}

// 'org.mpris.MediaPlayer2.Player', 'PlaybackStatus', dbus_interface='org.freedesktop.DBus.Properties'
#[dbus_proxy(
    interface = "org.mpris.MediaPlayer2.Player",
    default_path = "/org/mpris/MediaPlayer2"
)]
trait MprisPlayer {
    #[dbus_proxy(property)]
    fn PlaybackStatus(&self) -> fdo::Result<String>;

    #[dbus_proxy(property)]
    fn Metadata(&self) -> fdo::Result<HashMap<String, Value>>;
}

struct State<'a> {
    config: Config,
    current_player: usize,
    player_names: Vec<OwnedBusName>,
    message: String,
    display_text: String,
    display_prefix: char,
    display_suffix: String,
    status_paused: bool,
    last_player_name: String,
    dbus_conn: Connection,
    dbus_proxy: DBusProxy<'a>,
}

// useful alias
const STRNONE: Option<&str> = None;

impl<'a> State<'a> {
    async fn new(config: Config) -> anyhow::Result<State<'a>> {
        let dbus_conn = Connection::session().await?;

        let mut s = State {
            config,
            current_player: 0,
            player_names: Vec::new(),
            message: String::new(),
            display_text: String::new(),
            display_prefix: ' ',
            display_suffix: String::new(),
            status_paused: false,
            last_player_name: String::new(),
            dbus_proxy: DBusProxy::new(&dbus_conn).await?,
            dbus_conn,
        };

        s.update_players().await?;

        Ok(s)
    }

    fn get_name_by_index(&self, index: usize) -> Option<String> {
        Some(get_name(self.player_names.get(index)?.as_str()))
    }

    fn update_prefix_suffix(
        &mut self,
        player_name: Option<impl AsRef<str> + Clone>,
        status: Option<impl AsRef<str>>,
    ) {
        let mut player_option = String::new();

        if let Some(player_name) = player_name.clone() {
            player_option = format!("-p {}", player_name.as_ref());
        }

        let prev_button = format!(
            "%{{A:playerctl {player_option} previous :}}{}%{{A}}",
            self.config.control_chars.previous
        );
        let play_button = format!(
            "%{{A:playerctl {player_option} play :}}{}%{{A}}",
            self.config.control_chars.play
        );
        let pause_button = format!(
            "%{{A:playerctl {player_option} pause :}}{}%{{A}}",
            self.config.control_chars.pause
        );
        let next_button = format!(
            "%{{A:playerctl {player_option} next :}}{}%{{A}}",
            self.config.control_chars.next
        );

        let mut suffix = format!(" {prev_button}");

        if status.is_some() && status.unwrap().as_ref() == "Playing" {
            suffix += &format!(" {pause_button}");
            self.status_paused = false;
        } else {
            suffix += &format!(" {play_button}");
            self.status_paused = true;
        }

        suffix += &format!(" {next_button}");
        self.display_suffix = suffix;

        self.display_prefix = if let Some(player_name) = player_name {
            let player_name = player_name.as_ref();
            self.config
                .display_player_prefixes
                .specific
                .iter()
                .find(|(key, _)| key.contains(player_name))
                .map(|(_, val)| val)
                .copied()
                .unwrap_or(self.config.display_player_prefixes.default)
        } else {
            self.config.display_player_prefixes.default
        };
    }

    async fn update_players(&mut self) -> anyhow::Result<()> {
        self.player_names = self
            .dbus_proxy
            .list_names()
            .await?
            .into_iter()
            .filter(|name| {
                name.as_str()
                    .to_ascii_lowercase()
                    .starts_with("org.mpris.mediaplayer2.")
            })
            .collect();

        if Some(&self.last_player_name) != self.get_name_by_index(self.current_player).as_ref() {
            for (i, player) in self.player_names.iter().enumerate() {
                if get_name(player.as_str()) == self.last_player_name {
                    self.current_player = i;
                }
            }
        }

        Ok(())
    }

    // e.g. handle_event
    async fn next_player(&mut self) -> anyhow::Result<()> {
        self.update_players().await?;

        if self.player_names.is_empty() {
            return Ok(());
        }

        self.current_player = (self.current_player + 1) % self.player_names.len();
        self.last_player_name = self
            .get_name_by_index(self.current_player)
            .ok_or(anyhow::anyhow!("invalid index"))?;

        Ok(())
    }

    async fn update_message(&mut self) -> anyhow::Result<()> {
        let new_message = if self.player_names.is_empty() {
            self.update_prefix_suffix(STRNONE, STRNONE);
            "No player available".into()
        } else {
            let name = self
                .get_name_by_index(self.current_player)
                .ok_or(anyhow::anyhow!("invalid index"))?;

            let player_name = &self.player_names[self.current_player];
            let player = MprisPlayerProxy::builder(&self.dbus_conn)
                .destination(player_name)?
                .build()
                .await?;

            let status = player.PlaybackStatus().await?;
            let metadata: HashMap<String, Value> = player.Metadata().await?;

            let mut metadata_string_list: Vec<String> = Vec::new();
            for field in &self.config.metadata_fields {
                if let Some(res) = metadata.get(field) {
                    let str = value_to_string(res);
                    let str = str.trim();

                    if !str.is_empty() {
                        metadata_string_list.push(str.to_string());
                    }
                }
            }

            let mut metadata_string: String =
                metadata_string_list.join(&format!(" {} ", self.config.metadata_seperator));
            if visual_len(&metadata_string) > self.config.message_display_len {
                metadata_string = format!(" {metadata_string}  ");
            }
            self.update_prefix_suffix(Some(&name), Some(status));

            self.last_player_name = name;

            metadata_string
        };

        if new_message != self.message {
            self.message = new_message;
            self.display_text = self.message.clone();
        }

        Ok(())
    }

    fn scroll(&mut self) {
        if !self.status_paused {
            match visual_len(&self.display_text).cmp(&self.config.message_display_len) {
                Ordering::Greater => {
                    let mut text = self.display_text.chars();
                    let first = text
                        .next()
                        .map(|s| s.to_string())
                        .unwrap_or_else(String::new);
                    self.display_text = text.collect::<String>() + &first;
                }
                Ordering::Less => {
                    self.display_text +=
                        &" ".repeat(self.config.message_display_len - self.display_text.len());
                }
                Ordering::Equal => {}
            }
        }
    }

    fn print_text(&mut self) {
        if self.config.hide_output && self.player_names.is_empty() {
            println!();
            return;
        }

        self.scroll();
        println!(
            "{} %{{T{}}}{}%{{T-}}{}",
            self.display_prefix,
            self.config.font_index,
            make_visual_len(&self.display_text, self.config.message_display_len),
            self.display_suffix
        );
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config_file_path = Path::new(CONFIG_PATH);

    if !config_file_path.exists() {
        File::create(CONFIG_PATH)
            .with_context(|| format!("failed to create config file ({CONFIG_PATH})"))?
            .write_all(Config::default_str().as_bytes())
            .context("failed to write to config file")?;
    }

    let config = Config::load(
        File::open(config_file_path)
            .with_context(|| format!("failed to open config file ({CONFIG_PATH})"))?,
    )?;

    let mut signals =
        Signals::new(&[SIGUSR1, SIGTERM]).context("failed registering signal handlers")?;

    let mut state = State::new(config).await?;
    let mut interval = tokio::time::interval(std::time::Duration::from_secs_f32(
        state.config.update_delay,
    ));
    interval.tick().await;
    let handle = signals.handle();

    loop {
        tokio::select! {
            _ = interval.tick() => {
                state.update_players().await?;
                state.update_message().await?;
                state.print_text();
            },
            signal = signals.next() => {
                if let Some(signal) = signal {
                match signal {
                    SIGUSR1 => {
                        state.next_player().await?;
                    },
                    _ => {
                        break;
                    }
                }
                }

            }
        }
    }

    handle.close();

    Ok(())
}
