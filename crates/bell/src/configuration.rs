use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;
use serde::de::{IntoDeserializer, MapAccess, Visitor};
use toml;

use crate::render::Color;
use crate::wayland::{Anchor, Layer};

#[macro_export]
macro_rules! with_change {
    ( $self:expr,$side:ident ) => {{
        let mut ret = $self.clone();
        ret.$side = $side;
        ret
    }};
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct Margin {
    #[serde(default)]
    pub top: i32,
    #[serde(default)]
    pub right: i32,
    #[serde(default)]
    pub bottom: i32,
    #[serde(default)]
    pub left: i32,
}

impl Margin {
    pub fn with_top(&self, top: i32) -> Margin {
        with_change!(self, top)
    }
    pub fn with_right(&self, right: i32) -> Margin {
        with_change!(self, right)
    }
    pub fn with_bottom(&self, bottom: i32) -> Margin {
        with_change!(self, bottom)
    }
    pub fn with_left(&self, left: i32) -> Margin {
        with_change!(self, left)
    }
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
pub enum GrowthDirection {
    Up,
    Right,
    Down,
    Left,
}

impl Default for GrowthDirection {
    fn default() -> GrowthDirection {
        GrowthDirection::Up
    }
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Option<Color>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(|input: u32| Some(Color(input)))
}

fn deserialize_anchor<'de, D>(deserializer: D) -> Result<Option<Anchor>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    fn for_each_flag(flag: &str) -> Anchor {
        Anchor::from_name(flag.trim()).unwrap_or(Anchor::empty())
    }

    fn handle_input(input: String) -> Option<Anchor> {
        let anchors = input.split('|').map(for_each_flag);
        anchors.reduce(|acc, anchor| acc | anchor)
    }

    Deserialize::deserialize(deserializer).map(handle_input)
}

const VARIANTS: [&'static str; 4] = ["Background", "Bottom", "Top", "Overlay"];
struct LayerVisitor;
impl<'de> Visitor<'de> for LayerVisitor {
    type Value = Option<Layer>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a Layer option (Background, Bottom, Top, or Overlay)")
    }

    fn visit_str<E>(self, data: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match data {
            "Background" => Ok(Some(Layer::Background)),
            "Bottom" => Ok(Some(Layer::Bottom)),
            "Top" => Ok(Some(Layer::Top)),
            "Overlay" => Ok(Some(Layer::Overlay)),
            _ => Err(E::unknown_variant(data, &VARIANTS)),
        }
    }
}
fn deserialize_layer<'de, D>(deserializer: D) -> Result<Option<Layer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_str(LayerVisitor)
}

struct OutputsVisitor;
impl<'de> Visitor<'de> for OutputsVisitor {
    type Value = HashMap<String, Arc<OutputConfiguration>>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a map of output names to pointers to configuration")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = HashMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, Arc::new(value));
        }

        Ok(map)
    }
}
fn deserialize_outputs<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Arc<OutputConfiguration>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_map(OutputsVisitor)
}

struct UrgencyVisitor;
impl<'de> Visitor<'de> for UrgencyVisitor {
    type Value = HashMap<String, Arc<UrgencyConfiguration>>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a map of output names to pointers to configuration")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = HashMap::with_capacity(access.size_hint().unwrap_or(0));

        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, Arc::new(value));
        }

        Ok(map)
    }
}
fn deserialize_urgency<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, Arc<UrgencyConfiguration>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_map(UrgencyVisitor)
}

fn deserialize_arc<'de, D, T>(deserializer: D) -> Result<Arc<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(|input: T| Arc::new(input))
}

fn default_sound() -> String {
    String::from("/usr/share/sounds/freedesktop/stereo/message-new-instant.oga")
}

#[derive(Debug, Deserialize)]
pub struct UrgencyConfiguration {
    #[serde(default)]
    pub message_layout: Option<String>,

    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(deserialize_with = "deserialize_color")]
    #[serde(default)]
    pub text_color: Option<Color>,

    #[serde(deserialize_with = "deserialize_color")]
    #[serde(default)]
    pub background_color: Option<Color>,

    #[serde(default)]
    pub icon_theme: Option<String>,

    #[serde(deserialize_with = "deserialize_color")]
    #[serde(default)]
    pub border_color: Option<Color>,
    #[serde(default)]
    pub border_size: Option<usize>,
    #[serde(default)]
    pub border_radius: Option<usize>,

    #[serde(deserialize_with = "deserialize_layer")]
    #[serde(default)]
    pub layer: Option<Layer>,
}

impl Default for UrgencyConfiguration {
    fn default() -> Self {
        UrgencyConfiguration {
            message_layout: Some("<summary> from <app_name>\n<body>".to_owned()),
            font_family: None,
            font_size: Some(14.0),
            text_color: Some(Color::rgba(0xFF, 0xFF, 0xFF, 0xFF)),
            background_color: Some(Color::rgba(0x00, 0x00, 0x00, 0xFF)),
            icon_theme: Some("Adwaita".to_owned()), // FIXME: Maybe we should instead leave it as None by default?
            border_color: Some(Color::rgba(0x00, 0x00, 0x00, 0xFF)),
            border_size: Some(0),
            border_radius: Some(4),
            layer: Some(Layer::Top),
        }
    }
}

struct EnableOptionVisitor;
impl<'de> Visitor<'de> for EnableOptionVisitor {
    type Value = Option<EnableOption>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a bool or a EnableOption option (Enabled, Disabled)")
    }

    fn visit_str<E>(self, data: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        match data.to_lowercase().as_str() {
            "enabled" => Ok(Some(EnableOption::Enabled)),
            "when-active" => Ok(Some(EnableOption::WhenActive)),
            "disabled" => Ok(Some(EnableOption::Disabled)),
            _ => Err(E::unknown_variant(
                data,
                &["enabled", "when-active", "disabled"],
            )),
        }
    }

    fn visit_bool<E>(self, data: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(Some(EnableOption::from(data)))
    }
}
fn deserialize_enable_option<'de, D>(deserializer: D) -> Result<Option<EnableOption>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_any(EnableOptionVisitor)
}

#[derive(Copy, Clone, Debug, Deserialize, Eq, PartialEq)]
pub enum EnableOption {
    Enabled,
    #[serde(rename = "when-active")]
    WhenActive,
    Disabled,
}

impl Default for EnableOption {
    fn default() -> Self {
        EnableOption::Disabled
    }
}

impl Into<bool> for EnableOption {
    fn into(self) -> bool {
        match self {
            Self::Enabled => true,
            Self::WhenActive => true,
            Self::Disabled => false,
        }
    }
}

impl From<bool> for EnableOption {
    fn from(value: bool) -> Self {
        if value { Self::Enabled } else { Self::Disabled }
    }
}

#[derive(Debug, Deserialize)]
pub struct OutputConfiguration {
    #[serde(deserialize_with = "deserialize_enable_option")]
    #[serde(default)]
    pub enabled: Option<EnableOption>,

    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,

    #[serde(deserialize_with = "deserialize_anchor")]
    #[serde(default)]
    pub anchor: Option<Anchor>,
    #[serde(default)]
    pub direction: Option<GrowthDirection>,

    #[serde(default)]
    pub margins: Option<Margin>,

    #[serde(flatten)]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_arc")]
    pub default_urgency: Arc<UrgencyConfiguration>,

    #[serde(default)]
    #[serde(rename = "urgency")]
    #[serde(deserialize_with = "deserialize_urgency")]
    pub urgencies: HashMap<String, Arc<UrgencyConfiguration>>,
}

impl Default for OutputConfiguration {
    fn default() -> Self {
        OutputConfiguration {
            enabled: None,
            width: Some(260),
            height: Some(125),
            anchor: Some(Anchor::Right | Anchor::Bottom),
            direction: Some(GrowthDirection::default()),
            margins: Some(Margin::default()),
            default_urgency: Arc::new(UrgencyConfiguration::default()),
            urgencies: HashMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Hash)]
pub enum EventTrigger {
    #[serde(rename = "left-click")]
    OnLeftClick,
    #[serde(rename = "right-click")]
    OnRightClick,
    #[serde(rename = "middle-click")]
    OnMiddleClick,
    #[serde(rename = "on-notification-received")]
    OnNotificationReceived,
    #[serde(rename = "on-notification-closed")]
    OnNotificationClosed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub enum EventResponse {
    #[serde(rename = "close-notification")]
    CloseNotification,
    #[serde(rename = "exec")]
    ExecuteCommand(String),
    #[serde(rename = "play-sound")]
    PlaySound(String),
    #[serde(rename = "invoke-action")]
    InvokeAction,
    #[serde(rename = "nothing")]
    Nothing,
}

impl Default for EventResponse {
    fn default() -> Self {
        EventResponse::Nothing
    }
}

fn default_true() -> bool {
    true
}

fn default_idle_time() -> u32 {
    30_000
}

#[derive(Debug, Default, Deserialize)]
pub struct Configuration {
    #[serde(flatten)]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_arc")]
    default_output_config: Arc<OutputConfiguration>,

    #[serde(default = "default_sound")]
    pub default_sound: String,

    #[serde(default = "default_true")]
    #[serde(rename = "persist-when-idle")]
    pub persist_when_idle: bool,
    #[serde(default = "default_idle_time")]
    #[serde(rename = "idle-time")]
    pub idle_time: u32,

    #[serde(default)]
    events: HashMap<EventTrigger, EventResponse>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_outputs")]
    outputs: HashMap<String, Arc<OutputConfiguration>>,
}

#[derive(Copy, Clone, Debug, Default)]
pub struct TextOptions {
    pub font_size: f32,
    pub line_height: f32,
    pub text_color: u32,

    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[macro_export]
macro_rules! with_other {
    ( $self:expr,$other:expr,$($field:ident) + ) => {{ $( $self.$field = $self.$field.or($other.$field); )+ }};
}
#[macro_export]
macro_rules! with_other_owned {
    ( $self:expr,$other:expr,$($field:ident) + ) => {{ $( $self.$field = $self.$field.as_ref().or($other.$field.as_ref()).map(|i| i.clone()); )+ }};
}
impl UrgencyConfiguration {
    pub fn complete_missing(&mut self, other: &UrgencyConfiguration) {
        with_other_owned!(self, other, message_layout);
        with_other_owned!(self, other, font_family);
        with_other!(self, other, font_size text_color);
        with_other!(self, other, background_color);
        with_other_owned!(self, other, icon_theme);
        with_other!(self, other, border_color border_size border_radius);
        with_other!(self, other, layer);
    }

    pub fn get_message_layout<T>(
        &self,
        mut render_fragment: impl FnMut(String, TextOptions) -> T,
    ) -> Vec<T> {
        let mut spans = Vec::<T>::new();

        let layout = self.message_layout.as_ref().unwrap();
        for (fragment, options) in self.parse_layout(layout, None) {
            spans.push(render_fragment(fragment, options));
        }

        spans
    }

    pub fn parse_layout(
        &self,
        layout: &str,
        starting_text_options: Option<TextOptions>,
    ) -> Vec<(String, TextOptions)> {
        let mut parsed_fragments = Vec::new();

        let mut current_text_options = starting_text_options.unwrap_or_default();
        for chunk in layout.split(['<', '>']) {
            if chunk.len() == 0 {
                continue;
            }

            match chunk {
                "b" => current_text_options.bold = true,
                "/b" => current_text_options.bold = false,
                "i" => current_text_options.italic = true,
                "/i" => current_text_options.italic = false,
                "u" => current_text_options.underline = true,
                "/u" => current_text_options.underline = false,
                fragment => {
                    let current_line_height = current_text_options.line_height;

                    let mut parse_sections = |sections: Vec<&str>| {
                        let preprocessed_fragment = sections.join("\n");
                        if !preprocessed_fragment.is_empty() {
                            let (parsed_fragment, font_size, line_height, text_color) = self
                                .parse_layout_fragment(
                                    &preprocessed_fragment,
                                    starting_text_options,
                                );

                            current_text_options.font_size = font_size;
                            current_text_options.line_height = line_height;
                            current_text_options.text_color = text_color;

                            parsed_fragments.push((parsed_fragment, current_text_options.clone()));
                        }
                    };

                    let mut current_sections = Vec::new();

                    // Parse isolated / consecutive '\n's with a lower line height, so they don't take much space.
                    let mut fragment_sections = fragment.split('\n').enumerate().peekable();
                    while let Some((index, fragment_section)) = fragment_sections.next() {
                        let should_add_newline = {
                            if fragment_section.is_empty() {
                                // Ignore segmenting of the last empty section, since X '\n's generate
                                // X + 1 sections, but we only want to have X sections in the final output.
                                //
                                // If peek.is_some:
                                //   If is_empty:
                                //     Fragment: (...)\n.\n($ | \n(...)) | ^.\n($ | \n(...))
                                //     Always add newline; skip next section in first case
                                //   Else:
                                //     Fragment: (...)\n.\n(...) or ^.\n(abc)
                                //     Only add newline in first case
                                // Else:
                                //   Fragment: (...)\n.$ or ^.$
                                //   Never add newline
                                if let Some((_, next_section)) = fragment_sections.peek() {
                                    if next_section.is_empty() {
                                        if index != 0 {
                                            fragment_sections.next();
                                        }

                                        true
                                    } else {
                                        index != 0
                                    }
                                } else {
                                    false
                                }
                            } else {
                                // Fragment: (...)\n.(...)
                                false
                            }
                        };

                        if should_add_newline {
                            parse_sections(current_sections);
                            current_sections = Vec::new();

                            let section =
                                format!("line_height={} \n", (current_line_height / 2.0).min(12.0));
                            parse_sections(vec![&section; 1]);
                        } else {
                            current_sections.push(fragment_section);
                        }
                    }

                    parse_sections(current_sections);
                }
            }
        }

        parsed_fragments
    }

    fn parse_layout_fragment(
        &self,
        layout_fragment: &str,
        default_options: Option<TextOptions>,
    ) -> (String, f32, f32, u32) {
        let fragment_split = layout_fragment.split(['=', ' ']).collect::<Vec<&str>>();
        let mut fragment_index = 0;

        let mut font_size = default_options.map_or(self.font_size, |v| Some(v.font_size));
        let mut line_height = default_options.map_or(None, |v| Some(v.line_height));
        let mut text_color = default_options.map_or(self.text_color, |v| Some(Color(v.text_color)));

        loop {
            match fragment_split.as_slice()[fragment_index..] {
                ["font_size", value, ..] => {
                    fragment_index += 2;

                    match value.parse::<f32>() {
                        Ok(parsed_value) => font_size = Some(parsed_value),
                        Err(error) => eprintln!(
                            "Failed to parse 'font_size' parameter in 'message_layout': {}",
                            error.to_string()
                        ),
                    }
                }
                ["line_height", value, ..] => {
                    fragment_index += 2;

                    match value.parse::<f32>() {
                        Ok(parsed_value) => line_height = Some(parsed_value),
                        Err(error) => eprintln!(
                            "Failed to parse 'line_height' parameter in 'message_layout': {}",
                            error.to_string()
                        ),
                    }
                }
                ["color", value, ..] => {
                    fragment_index += 2;

                    let u32_value =
                        u32::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16);
                    if let Err(error) = u32_value {
                        eprintln!(
                            "Failed to parse 'color' parameter in 'message_layout': {}",
                            error.to_string()
                        );
                        continue;
                    }
                    let u32_value = u32_value.unwrap();

                    use serde::de::value::{Error, U32Deserializer};
                    match deserialize_color::<U32Deserializer<Error>>(u32_value.into_deserializer())
                    {
                        Ok(parsed_color) => text_color = parsed_color,
                        Err(error) => eprintln!(
                            "Failed to parse 'color' parameter in 'message_layout': {}",
                            error.to_string()
                        ),
                    }
                }
                _ => break,
            }
        }

        let fragment = fragment_split[fragment_index..].join(" ");

        let font_size = font_size.expect("Failed to parse font size for layout fragment.");
        let line_height = line_height.unwrap_or(font_size + 4.0);
        let text_color = text_color
            .expect("Failed to parse text color for layout fragment.")
            .0;

        (fragment, font_size, line_height, text_color)
    }
}
impl OutputConfiguration {
    pub fn complete_missing(&mut self, other: &OutputConfiguration) {
        with_other!(self, other, enabled);
        with_other!(self, other, width height);
        with_other!(self, other, anchor direction margins);

        Arc::get_mut(&mut self.default_urgency)
            .unwrap()
            .complete_missing(&other.default_urgency);

        for (urgency_key, urgency) in self.urgencies.iter_mut() {
            if other.urgencies.contains_key(urgency_key) {
                Arc::get_mut(urgency)
                    .unwrap()
                    .complete_missing(other.urgencies.get(urgency_key).as_ref().unwrap());
            }
        }
    }

    pub fn get_by_urgency(&self, urgency: &str) -> Arc<UrgencyConfiguration> {
        Arc::clone(self.urgencies.get(urgency).unwrap_or(&self.default_urgency))
    }
}

const ENV_VARIABLES: [&'static str; 2] = ["XDG_CONFIG_HOME", "HOME"];

const DEFAULT_PATHS: [&'static str; 3] = [
    "$XDG_CONFIG_HOME/bell/config.toml",
    "$HOME/.config/bell/config.toml",
    "/usr/local/share/bell/config.toml",
];

use std::io::{Error, ErrorKind};
impl Configuration {
    pub fn from_default_paths() -> Result<Configuration, Error> {
        let mut errors = Vec::<(std::path::PathBuf, Error)>::new();

        fn substitute_env_vars(input: &str) -> String {
            let mut ret = input.to_owned();

            for env_var in ENV_VARIABLES.iter() {
                if let Ok(value) = std::env::var(env_var) {
                    ret = ret.replace(format!("${}", env_var).as_str(), value.as_str());
                }
            }

            ret
        }

        for path_raw in DEFAULT_PATHS.iter() {
            let path_sub = substitute_env_vars(path_raw);
            let path = std::path::PathBuf::from(path_sub);

            match Configuration::from_file(path.as_path()) {
                Ok(configuration) => {
                    return Ok(configuration);
                }
                Err(error) => {
                    errors.push((path, error));
                }
            }
        }

        eprintln!("Failed to find a configuration file. Tried the following:");
        for (path, error) in errors {
            eprintln!("  {} - {}", path.display(), error)
        }

        Err(Error::from(ErrorKind::NotFound))
    }

    pub fn from_file(path: &std::path::Path) -> Result<Configuration, Error> {
        if !path.is_file() {
            let path_string = path.to_str().unwrap().to_owned();
            return Err(Error::new(
                ErrorKind::NotFound,
                format!(
                    "The specified configuration path '{}' is not a file.",
                    path_string
                ),
            ));
        }

        let file_contents = std::fs::read_to_string(path)?;

        Configuration::from_string(&file_contents)
    }

    pub fn from_string(string: &String) -> Result<Configuration, Error> {
        match toml::from_str::<Configuration>(string.as_str()) {
            Ok(mut configuration) => {
                configuration.populate_outputs_with_default();
                Ok(configuration)
            }
            Err(error) => {
                let error_span = error.span().unwrap_or_default();
                let error_section = &string.as_str()[error_span.start..error_span.end];
                Err(Error::new(
                    ErrorKind::InvalidData,
                    format!("{} in '{}'", error.message(), error_section),
                ))
            }
        }
    }

    pub fn get_output_configuration(&self, output_name: &str) -> Arc<OutputConfiguration> {
        match self.outputs.get(output_name) {
            Some(output_configuration) => Arc::clone(output_configuration),
            None => Arc::clone(&self.default_output_config),
        }
    }

    pub fn get_event_handler(&self) -> HashMap<EventTrigger, EventResponse> {
        self.events.clone()
    }

    fn populate_outputs_with_default(&mut self) {
        let global_conf = Arc::get_mut(&mut self.default_output_config).unwrap();
        global_conf.complete_missing(&OutputConfiguration::default());

        for urgency in global_conf.urgencies.values_mut() {
            Arc::get_mut(urgency)
                .unwrap()
                .complete_missing(&global_conf.default_urgency);
        }

        for output_configuration in self.outputs.values_mut() {
            let conf = Arc::get_mut(output_configuration).unwrap();
            conf.complete_missing(&self.default_output_config);

            for urgency in conf.urgencies.values_mut() {
                Arc::get_mut(urgency)
                    .unwrap()
                    .complete_missing(&conf.default_urgency);
            }
        }
    }
}

#[test]
fn test_empty_configuration() {
    let file_contents = "".to_owned();
    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    assert!(configuration.unwrap().outputs.is_empty());
}

#[test]
fn test_single_output_partial() {
    let file_contents = r#"
        [outputs."eDP-1"]
        width = 300
        height = 150
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    assert_eq!(configuration.outputs.len(), 1);
    assert!(configuration.outputs.contains_key("eDP-1"));

    let output_spec = configuration.outputs.get("eDP-1").unwrap();
    assert_eq!(output_spec.width.unwrap(), 300);
    assert_eq!(output_spec.height.unwrap(), 150);
}

#[test]
fn test_single_output_complete() {
    let file_contents = r#"
        [outputs."A-HDMI-1"]
        width = 300
        height = 150

        background_color = 0xDEADBEEF

        border_color = 0xBEEFDEAD
        border_size = 2

        anchor = "Left|Top"
        direction = "Down"

        layer = "Overlay"

        [outputs."A-HDMI-1".margins]
        left = 1
        right = 2
        top = 3
        bottom = 4
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    assert_eq!(configuration.outputs.len(), 1);
    assert!(configuration.outputs.contains_key("A-HDMI-1"));

    let output_spec = configuration.outputs.get("A-HDMI-1").unwrap();
    assert_eq!(output_spec.width.unwrap(), 300);
    assert_eq!(output_spec.height.unwrap(), 150);

    assert_eq!(
        output_spec.default_urgency.background_color.unwrap(),
        Color::rgba(0xAD, 0xBE, 0xEF, 0xDE)
    );

    assert_eq!(
        output_spec.default_urgency.border_color.unwrap(),
        Color::rgba(0xEF, 0xDE, 0xAD, 0xBE)
    );
    assert_eq!(output_spec.default_urgency.border_size.unwrap(), 2);

    assert_eq!(output_spec.anchor.unwrap(), Anchor::Left | Anchor::Top);
    assert_eq!(output_spec.direction.unwrap(), GrowthDirection::Down);

    assert_eq!(output_spec.default_urgency.layer.unwrap(), Layer::Overlay);

    assert_eq!(
        output_spec.margins.unwrap(),
        Margin {
            left: 1,
            right: 2,
            top: 3,
            bottom: 4
        }
    );
}

#[test]
fn test_multiple_outputs_partial() {
    let file_contents = r#"
        [outputs."eDP-1"]
        width = 300
        height = 150
        [outputs."A-HDMI-1"]
        width = 500
        height = 200
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    assert_eq!(configuration.outputs.len(), 2);
    assert!(configuration.outputs.contains_key("eDP-1"));
    assert!(configuration.outputs.contains_key("A-HDMI-1"));

    let output_spec = configuration.outputs.get("eDP-1").unwrap();
    assert_eq!(output_spec.width.unwrap(), 300);
    assert_eq!(output_spec.height.unwrap(), 150);

    let output_spec = configuration.outputs.get("A-HDMI-1").unwrap();
    assert_eq!(output_spec.width.unwrap(), 500);
    assert_eq!(output_spec.height.unwrap(), 200);
}

#[test]
fn test_default_output() {
    let file_contents = r#"
        width = 123
        height = 456
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();

    let output_spec = configuration.get_output_configuration("ahsiujhfiajsik");
    assert_eq!(output_spec.width.unwrap(), 123);
    assert_eq!(output_spec.height.unwrap(), 456);
}

#[test]
fn test_default_output_with_override() {
    let file_contents = r#"
        width = 123
        height = 456

        [outputs."eDP-1"]
        height = 789

        border_size = 2
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();

    let output_spec = configuration.get_output_configuration("eDP-1");
    assert_eq!(output_spec.width.unwrap(), 123);
    assert_eq!(output_spec.height.unwrap(), 789);

    assert_eq!(output_spec.default_urgency.border_size.unwrap(), 2);

    // Test that values not specified in neither the global nor the
    // per-output configuration still give their default value.
    assert!(output_spec.default_urgency.layer.is_some());
    assert_eq!(output_spec.default_urgency.layer.unwrap(), Layer::Top);
}

#[test]
fn test_event_handler() {
    let file_contents = r#"
        [events]
        left-click = "nothing"
        right-click = "close-notification"
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    let event_handler = |trigger| {
        configuration
            .get_event_handler()
            .get(trigger)
            .unwrap_or(&EventResponse::Nothing)
            .clone()
    };

    assert_eq!(
        event_handler(&EventTrigger::OnLeftClick),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnRightClick),
        EventResponse::CloseNotification
    );
    assert_eq!(
        event_handler(&EventTrigger::OnMiddleClick),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnNotificationReceived),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnNotificationClosed),
        EventResponse::Nothing
    );
}

#[test]
fn test_event_handler_custom_command() {
    let file_contents = r#"
        [events]
        left-click = { exec = "echo abc" }
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    let event_handler = |trigger| {
        configuration
            .get_event_handler()
            .get(trigger)
            .unwrap_or(&EventResponse::Nothing)
            .clone()
    };

    assert_eq!(
        event_handler(&EventTrigger::OnLeftClick),
        EventResponse::ExecuteCommand("echo abc".to_owned())
    );
    assert_eq!(
        event_handler(&EventTrigger::OnRightClick),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnMiddleClick),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnNotificationReceived),
        EventResponse::Nothing
    );
    assert_eq!(
        event_handler(&EventTrigger::OnNotificationClosed),
        EventResponse::Nothing
    );
}

#[test]
fn test_message_layout_simple() {
    let file_contents = r#"
        message_layout = "<summary>\n<app_name>\n<body>"
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    let output_config = configuration.get_output_configuration("");

    let mut app_name_called = false;
    let mut summary_called = false;
    let mut body_called = false;
    let mut fragment_called = false;

    let render = |text: String, text_options: TextOptions| {
        assert_eq!(
            text_options.font_size,
            configuration
                .default_output_config
                .default_urgency
                .font_size
                .unwrap()
        );
        assert_eq!(
            text_options.text_color,
            configuration
                .default_output_config
                .default_urgency
                .text_color
                .unwrap()
                .0
        );

        match text.as_str() {
            "app_name" => app_name_called = true,
            "summary" => summary_called = true,
            "body" => body_called = true,
            "\n" => fragment_called = true,
            _ => {
                unreachable!()
            }
        }
    };

    output_config
        .get_by_urgency("Normal")
        .get_message_layout(render);

    assert!(app_name_called);
    assert!(summary_called);
    assert!(body_called);
    assert!(fragment_called);
}

#[test]
fn test_message_layout_customized() {
    let file_contents = r#"
message_layout = """
<color=0xDEADBEEF font_size=13.5 this is a custom text>
<font_size=18 summary>
"""

[urgency.Normal]
font_size = 14
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    let output_config = configuration.get_output_configuration("");

    let mut summary_called = false;
    let mut fragment_called = false;

    let render = |text: String, text_options: TextOptions| match text.as_str() {
        "summary" => {
            summary_called = true;
            assert_eq!(text_options.font_size, 18.0);
        }
        "this is a custom text" => {
            fragment_called = true;
            assert_eq!(text_options.font_size, 13.5);
            assert_eq!(
                text_options.text_color,
                Color::rgba(0xAD, 0xBE, 0xEF, 0xDE).0
            );
        }
        "\n" => {}
        _ => {
            unreachable!()
        }
    };

    output_config
        .get_by_urgency("Normal")
        .get_message_layout(render);

    assert!(summary_called);
    assert!(fragment_called);
}

#[allow(unused_macros)]
macro_rules! format_color {
    ($value:expr) => {
        format!(
            "0x{:02X}{:02X}{:02X}{:02X}",
            $value.a(),
            $value.r(),
            $value.g(),
            $value.b()
        )
    };
}

#[allow(unused_macros)]
macro_rules! assert_eq_color {
    ($got:expr,$expected:expr) => {
        let got_str = format_color!($got);
        let expected_str = format_color!($expected);

        assert_eq!(
            $got, $expected,
            "Got: {} | Expected: {}",
            got_str, expected_str
        );
    };
}

#[test]
fn test_custom_urgency_levels() {
    let file_contents = r#"
        [outputs."eDP-1"]
        border_size = 2
        border_color = 0xFF444488

        [outputs."eDP-1".urgency.Normal]
        border_color = 0xFF6644BB

        [outputs."eDP-1".urgency.Critical]
        border_color = 0xFFFF6644
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    let output_config = configuration.get_output_configuration("eDP-1");

    println!("Urgencies: {:?}", output_config.urgencies.keys());

    let low_urgency_config = output_config.get_by_urgency("Low");
    assert_eq!(low_urgency_config.border_size, Some(2));
    assert_eq_color!(
        low_urgency_config.border_color.unwrap(),
        Color::rgba(0x44, 0x44, 0x88, 0xFF)
    );

    let normal_urgency_config = output_config.get_by_urgency("Normal");
    assert_eq!(normal_urgency_config.border_size, Some(2));
    assert_eq_color!(
        normal_urgency_config.border_color.unwrap(),
        Color::rgba(0x66, 0x44, 0xBB, 0xFF)
    );

    let critical_urgency_config = output_config.get_by_urgency("Critical");
    assert_eq!(critical_urgency_config.border_size, Some(2));
    assert_eq_color!(
        critical_urgency_config.border_color.unwrap(),
        Color::rgba(0xFF, 0x66, 0x44, 0xFF)
    );
}

#[test]
fn test_enable_output() {
    let file_contents = r#"
        enabled = true

        [outputs."eDP-1"]
        width = 300
        height = 120

        [outputs."HDMI-A-1"]
        enabled = false

        [outputs."ABC"]
        enabled = "when-active"
    "#
    .to_owned();

    let configuration = Configuration::from_string(&file_contents);

    if let Err(error) = configuration {
        panic!("{}", error.to_string());
    }

    let configuration = configuration.unwrap();
    assert_eq!(
        configuration.default_output_config.enabled,
        Some(EnableOption::Enabled)
    );
    assert_eq!(
        configuration.outputs.get("eDP-1").unwrap().enabled,
        Some(EnableOption::Enabled)
    );
    assert_eq!(
        configuration.outputs.get("HDMI-A-1").unwrap().enabled,
        Some(EnableOption::Disabled)
    );
    assert_eq!(
        configuration.outputs.get("ABC").unwrap().enabled,
        Some(EnableOption::WhenActive)
    );
}
