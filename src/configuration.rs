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

#[derive(Debug, Deserialize)]
pub struct OutputConfiguration {
    #[serde(default)]
    pub width: Option<i32>,
    #[serde(default)]
    pub height: Option<i32>,

    #[serde(default)]
    pub message_layout: Option<String>,

    #[serde(default)]
    pub font_size: Option<f32>,
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

    #[serde(deserialize_with = "deserialize_anchor")]
    #[serde(default)]
    pub anchor: Option<Anchor>,
    #[serde(default)]
    pub direction: Option<GrowthDirection>,

    #[serde(deserialize_with = "deserialize_layer")]
    #[serde(default)]
    pub layer: Option<Layer>,

    #[serde(default)]
    pub margins: Option<Margin>,
}

#[macro_export]
macro_rules! with_other {
    ( $self:expr,$other:expr,$($field:ident) + ) => {{ $( $self.$field = $self.$field.or($other.$field); )+ }};
}
#[macro_export]
macro_rules! with_other_owned {
    ( $self:expr,$other:expr,$($field:ident) + ) => {{ $( $self.$field = $self.$field.as_ref().or($other.$field.as_ref()).map(|i| i.clone()); )+ }};
}
impl OutputConfiguration {
    pub fn complete_missing(&mut self, other: &OutputConfiguration) {
        with_other!(self, other, width height);
        with_other_owned!(self, other, message_layout);
        with_other!(self, other, font_size text_color);
        with_other!(self, other, background_color);
        with_other_owned!(self, other, icon_theme);
        with_other!(self, other, border_color border_size border_radius);
        with_other!(self, other, anchor direction layer margins);
    }

    pub fn get_message_layout(&self, mut render_fragment: impl FnMut(&str, f32, Color) -> ()) {
        let layout = self.message_layout.as_ref().unwrap();
        for chunk in layout.split(['<', '>']) {
            if chunk.len() == 0 {
                continue;
            }

            match chunk {
                fragment => {
                    let mut font_size: Option<f32> = None;
                    let mut text_color: Option<Color> = None;

                    let fragment_split = fragment.split(['=', ' ']).collect::<Vec<&str>>();
                    let mut fragment_index = 0;

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
                            ["color", value, ..] => {
                                fragment_index += 2;

                                let u32_value = u32::from_str_radix(
                                    value.strip_prefix("0x").unwrap_or(value),
                                    16,
                                );
                                if let Err(error) = u32_value {
                                    eprintln!(
                                        "Failed to parse 'color' parameter in 'message_layout': {}",
                                        error.to_string()
                                    );
                                    continue;
                                }
                                let u32_value = u32_value.unwrap();

                                use serde::de::value::{Error, U32Deserializer};
                                match deserialize_color::<U32Deserializer<Error>>(
                                    u32_value.into_deserializer(),
                                ) {
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
                    render_fragment(
                        fragment.as_str(),
                        font_size.unwrap_or(self.font_size.unwrap()),
                        text_color.unwrap_or(self.text_color.unwrap()),
                    );
                }
            }
        }
    }
}

impl Default for OutputConfiguration {
    fn default() -> Self {
        OutputConfiguration {
            width: Some(260),
            height: Some(125),
            message_layout: Some("<summary> from <app_name>\n<body>".to_owned()),
            font_size: Some(14.0),
            text_color: Some(Color::rgba(0xFF, 0xFF, 0xFF, 0xFF)),
            background_color: Some(Color::rgba(0x00, 0x00, 0x00, 0xFF)),
            icon_theme: Some("Adwaita".to_owned()), // FIXME: Maybe we should instead leave it as None by default?
            border_color: Some(Color::rgba(0x00, 0x00, 0x00, 0xFF)),
            border_size: Some(0),
            border_radius: Some(4),
            anchor: Some(Anchor::Right | Anchor::Bottom),
            direction: Some(GrowthDirection::default()),
            layer: Some(Layer::Top),
            margins: Some(Margin::default()),
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
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
pub enum EventResponse {
    #[serde(rename = "close-notification")]
    CloseNotification,
    #[serde(rename = "nothing")]
    Nothing,
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

fn deserialize_arc<'de, D, T>(deserializer: D) -> Result<Arc<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Deserialize::deserialize(deserializer).map(|input: T| Arc::new(input))
}

#[derive(Debug, Default, Deserialize)]
pub struct Configuration {
    #[serde(flatten)]
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_arc")]
    default_output_config: Arc<OutputConfiguration>,

    #[serde(default)]
    events: HashMap<EventTrigger, EventResponse>,

    #[serde(default)]
    #[serde(deserialize_with = "deserialize_outputs")]
    outputs: HashMap<String, Arc<OutputConfiguration>>,
}

const ENV_VARIABLES: [&'static str; 2] = ["XDG_CONFIG_HOME", "HOME"];

const DEFAULT_PATHS: [&'static str; 3] = [
    "$XDG_CONFIG_HOME/bell/config.toml",
    "$HOME/.config/bell/config.toml",
    "/usr/local/share/bell/config.toml",
];

use std::io::{Error, ErrorKind};
impl Configuration {
    pub fn from_default_paths() -> Result<Configuration, Vec<(ErrorKind, String)>> {
        let mut failed_paths = Vec::<(ErrorKind, String)>::new();

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
                    failed_paths.push((error.kind(), error.to_string()));
                }
            }
        }

        Err(failed_paths)
    }

    pub fn from_file(path: &std::path::Path) -> Result<Configuration, Error> {
        if !path.is_file() {
            let path_string = path.to_str().unwrap().to_owned();
            return Err(Error::new(ErrorKind::NotFound, path_string));
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

    pub fn get_event_handler(&self) -> impl Fn(&EventTrigger) -> EventResponse + use<> {
        let event_translator = self.events.clone();
        move |trigger: &EventTrigger| {
            *event_translator
                .get(trigger)
                .unwrap_or(&EventResponse::Nothing)
        }
    }

    fn populate_outputs_with_default(&mut self) {
        Arc::get_mut(&mut self.default_output_config)
            .unwrap()
            .complete_missing(&OutputConfiguration::default());
        for output_configuration in self.outputs.values_mut() {
            Arc::get_mut(output_configuration)
                .unwrap()
                .complete_missing(&self.default_output_config);
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
        output_spec.background_color.unwrap(),
        Color::rgba(0xAD, 0xBE, 0xEF, 0xDE)
    );

    assert_eq!(
        output_spec.border_color.unwrap(),
        Color::rgba(0xEF, 0xDE, 0xAD, 0xBE)
    );
    assert_eq!(output_spec.border_size.unwrap(), 2);

    assert_eq!(output_spec.anchor.unwrap(), Anchor::Left | Anchor::Top);
    assert_eq!(output_spec.direction.unwrap(), GrowthDirection::Down);

    assert_eq!(output_spec.layer.unwrap(), Layer::Overlay);

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

    assert_eq!(output_spec.border_size.unwrap(), 2);

    // Test that values not specified in neither the global nor the
    // per-output configuration still give their default value.
    assert!(output_spec.layer.is_some());
    assert_eq!(output_spec.layer.unwrap(), Layer::Top);
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
    let event_handler = configuration.get_event_handler();

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

    let render = |text: &str, font_size: f32, text_color: Color| {
        assert_eq!(
            font_size,
            configuration.default_output_config.font_size.unwrap()
        );
        assert_eq!(
            text_color,
            configuration.default_output_config.text_color.unwrap()
        );

        match text {
            "app_name" => app_name_called = true,
            "summary" => summary_called = true,
            "body" => body_called = true,
            "\n" => fragment_called = true,
            _ => {
                unreachable!()
            }
        }
    };

    output_config.get_message_layout(render);

    assert!(app_name_called);
    assert!(summary_called);
    assert!(body_called);
    assert!(fragment_called);
}

#[test]
fn test_message_layout_customized() {
    let file_contents = r#"
        message_layout = "<color=0xDEADBEEF font_size=13.5 this is a custom text>\n<font_size=18 summary>"
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

    let render = |text: &str, font_size: f32, text_color: Color| match text {
        "summary" => {
            summary_called = true;
            assert_eq!(font_size, 18.0);
        }
        "this is a custom text" => {
            fragment_called = true;
            assert_eq!(font_size, 13.5);
            assert_eq!(text_color, Color::rgba(0xAD, 0xBE, 0xEF, 0xDE));
        }
        "\n" => {}
        _ => {
            unreachable!()
        }
    };

    output_config.get_message_layout(render);

    assert!(summary_called);
    assert!(fragment_called);
}
