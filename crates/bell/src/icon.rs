use std::collections::HashMap;
use std::io::{Error as IOError, ErrorKind as IOErrorKind};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::LazyLock;

const UNPOPULATED_ICON_THEME_SEARCH_PATHS: [&'static str; 3] =
    ["$HOME/.icons", "$XDG_DATA_DIRS/icons", "/usr/share/pixmaps"];

static POPULATED_ICON_THEME_SEARCH_PATHS: LazyLock<Vec<PathBuf>> =
    LazyLock::new(|| populate_icon_theme_search_paths());

#[derive(Copy, Clone)]
pub enum IconFileType {
    SVG,
    PNG,
    XPM,
}
pub struct IconFileTypeIterator(IconFileType);

impl IconFileType {
    const fn iter() -> IconFileTypeIterator {
        IconFileTypeIterator(IconFileType::SVG)
    }

    pub fn try_from_extension(extension: &str) -> Option<Self> {
        match extension {
            "svg" => Some(IconFileType::SVG),
            "png" => Some(IconFileType::PNG),
            "xpm" => Some(IconFileType::XPM),
            _ => None,
        }
    }
}

impl Into<&str> for IconFileType {
    fn into(self) -> &'static str {
        match self {
            IconFileType::SVG => ".svg",
            IconFileType::PNG => ".png",
            IconFileType::XPM => ".xpm",
        }
    }
}

impl Iterator for IconFileTypeIterator {
    type Item = IconFileType;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0 {
            IconFileType::SVG => {
                self.0 = IconFileType::PNG;
            }
            IconFileType::PNG => {
                self.0 = IconFileType::XPM;
            }
            IconFileType::XPM => {
                return None;
            }
        }

        Some(self.0)
    }
}

#[derive(Copy, Clone)]
pub struct IconSize {
    pub size: usize,
    pub scale: usize,
}

impl IconSize {
    pub fn scaled_size(&self) -> usize {
        self.size * self.scale
    }
}

impl Default for IconSize {
    fn default() -> IconSize {
        IconSize { size: 0, scale: 0 }
    }
}

pub struct IconFileInformation {
    pub path: PathBuf,
    pub file_type: IconFileType,

    pub icon_size: Option<IconSize>,
}

impl IconFileInformation {
    pub fn new(path: PathBuf, file_type: IconFileType, icon_size: Option<IconSize>) -> Self {
        IconFileInformation {
            path,
            file_type,
            icon_size,
        }
    }
}

// https://specifications.freedesktop.org/notification/latest/icons-and-images.html#icons-and-images-formats
pub fn retrieve_app_icon(
    app_icon: &str,
    icon_theme: Option<&str>,
    preferred_size: IconSize,
) -> Result<IconFileInformation, IOError> {
    if app_icon.starts_with("file://") {
        // URI (file:// is the only URI schema supported right now)
        let (_, file_extension) = app_icon
            .rsplit_once('.')
            .ok_or(IOError::from(IOErrorKind::InvalidInput))?;
        let file_type = IconFileType::try_from_extension(file_extension)
            .ok_or(IOError::from(IOErrorKind::Unsupported))?;

        return Ok(IconFileInformation::new(
            std::path::PathBuf::from_str(&app_icon).unwrap(),
            file_type,
            None,
        ));
    }

    // A name in a freedesktop.org-compliant icon theme.
    let icon_theme_path = match search_for_icon_theme(icon_theme) {
        Some(icon_theme_path) => icon_theme_path,
        None => {
            let missing_error = IOError::new(
                IOErrorKind::NotFound,
                format!(
                    "Icon theme '{}' was not found. Searched these paths: {}",
                    icon_theme.unwrap_or("<none>"),
                    join_search_paths(" | "),
                ),
            );

            if icon_theme.is_some() {
                search_for_icon_theme(None).ok_or(missing_error)?
            } else {
                Err(missing_error)?
            }
        }
    };

    if let Ok(icon_information) =
        search_for_icon_in_icon_theme(app_icon, preferred_size, &icon_theme_path)
    {
        return Ok(icon_information);
    }

    // TODO: Look in parent themes if not found.
    // TODO: Try to use fallback icon if all fails.

    Err(IOError::from(IOErrorKind::NotFound))
}

macro_rules! find_value {
    ($value:ident $source:expr) => {
        $source
            .find($value)
            .ok_or(IOError::from(IOErrorKind::InvalidData))?
    };
    ($value:literal $source:expr) => {
        $source
            .find($value)
            .ok_or(IOError::from(IOErrorKind::InvalidData))?
    };
    ($value:literal $default:literal $error:literal $source:expr) => {
        $source.find($value).unwrap_or($default)
    };
    ($value:literal $default:literal $source:expr) => {
        $source.find($value).unwrap_or($default)
    };
}

fn get_field_from_ini_file<'a>(source: &'a str, field: &str) -> Result<&'a str, IOError> {
    let start_index = find_value!(field source) + field.len() + 1;
    assert!(
        start_index < source.len(),
        "Overflowing INI file... The file may be malformed."
    );

    let end_index = start_index + find_value!('\n' source[start_index..]);

    Ok(&source[start_index..end_index])
}

fn get_section_from_ini_file(
    source: &str,
    section: &str,
) -> Result<HashMap<String, String>, IOError> {
    let start_index = find_value!(section source) + section.len() + 3;
    assert!(
        start_index < source.len(),
        "Overflowing INI file... The file may be malformed."
    );

    let mut section_items = HashMap::new();
    for line in source[start_index..].lines() {
        let mut split_line = line.split('=');

        match (split_line.next(), split_line.next()) {
            (Some(key), Some(value)) => {
                section_items.insert(key.to_string(), value.to_string());
            }
            _ => {}
        }
    }

    Ok(section_items)
}

// https://specifications.freedesktop.org/icon-theme/latest/#icon_lookup
fn search_for_icon_in_icon_theme(
    icon_name: &str,
    nominal_size: IconSize,
    icon_theme_path: &PathBuf,
) -> Result<IconFileInformation, IOError> {
    let index_file_path = icon_theme_path.join("./index.theme");
    let index_file_contents = std::fs::read_to_string(&index_file_path)?;

    let mut directories = get_field_from_ini_file(&index_file_contents, "Directories")?;
    directories = directories.trim_matches([' ', ',']);

    let mut found_icon_path: Option<PathBuf> = None;
    let mut found_file_type: Option<IconFileType> = None;
    let mut found_icon_size: Option<IconSize> = None;

    for directory in directories.split(',') {
        let path = icon_theme_path.join(directory);
        if !path.is_dir() {
            continue;
        }

        if let Ok(section_data) = get_section_from_ini_file(&index_file_contents, directory) {
            let ret = search_for_icon_in_directory(&path, icon_name, section_data, nominal_size);
            if let Some((found_path, found_type, found_size)) = ret {
                found_icon_path = Some(found_path);
                found_file_type = Some(found_type);
                found_icon_size = Some(found_size);

                break;
            }
        }
    }

    if let (Some(path), Some(file_type)) = (found_icon_path, found_file_type) {
        Ok(IconFileInformation::new(path, file_type, found_icon_size))
    } else {
        Err(IOError::from(IOErrorKind::NotFound))
    }
}

macro_rules! parse_usize {
    ($key_name:literal) => {
        |value: &String| usize::from_str(value).expect("Failed to parse '$key_name' field.")
    };
}

fn search_for_icon_in_directory(
    folder_path: &PathBuf,
    icon_name: &str,
    section_data: HashMap<String, String>,
    nominal_size: IconSize,
) -> Option<(PathBuf, IconFileType, IconSize)> {
    let mut closest_valid_icon = (
        usize::MAX,
        PathBuf::new(),
        IconFileType::SVG,
        IconSize::default(),
    );

    for file_type in IconFileType::iter() {
        let file_extension: &str = file_type.into();
        let attempt_path = folder_path.join(format!("{}{}", icon_name, file_extension));

        if !attempt_path.is_file() {
            continue;
        }

        let (icon_size_distance, actual_icon_size) =
            get_size_distance_for_icon(&section_data, nominal_size);

        if icon_size_distance == 0 {
            // Found an exact match; return it immediately.
            return Some((attempt_path, file_type, nominal_size));
        }

        if icon_size_distance < closest_valid_icon.0 {
            // Found a closer match; keep it around.
            closest_valid_icon = (
                icon_size_distance,
                attempt_path,
                file_type,
                actual_icon_size,
            );
        }
    }

    if closest_valid_icon.0 != usize::MAX {
        // Found a non-exact but valid match; return it.
        return Some((
            closest_valid_icon.1,
            closest_valid_icon.2,
            closest_valid_icon.3,
        ));
    }

    // No match found.
    None
}

// Implements 'DirectorySizeDistance' and 'DirectoryMatchesSize' (indirect) from reference pseudo-code.
fn get_size_distance_for_icon(
    section_data: &HashMap<String, String>,
    nominal_size: IconSize,
) -> (usize, IconSize) {
    let nominal_scaled_size = nominal_size.scaled_size();

    let icon_scale = section_data.get("Scale").map_or(1, parse_usize!("Scale"));
    let icon_size = section_data.get("Size").map(parse_usize!("Size")).unwrap();
    let icon_type = section_data
        .get("Type")
        .map_or("Threshold", |value: &String| value.as_str());

    match icon_type {
        "Fixed" => (
            nominal_scaled_size.abs_diff(icon_size * icon_scale),
            IconSize {
                size: icon_size,
                scale: icon_scale,
            },
        ),
        "Scalable" => {
            let min_size = section_data
                .get("MinSize")
                .map_or(icon_size, parse_usize!("MinSize"));
            let max_size = section_data
                .get("MaxSize")
                .map_or(icon_size, parse_usize!("MaxSize"));

            let min_scaled_size = min_size * icon_scale;
            let max_scaled_size = max_size * icon_scale;

            if nominal_scaled_size < min_scaled_size {
                return (
                    nominal_scaled_size.abs_diff(min_scaled_size),
                    IconSize {
                        size: min_size,
                        scale: icon_scale,
                    },
                );
            }

            if nominal_scaled_size > max_scaled_size {
                return (
                    nominal_scaled_size.abs_diff(max_scaled_size),
                    IconSize {
                        size: max_size,
                        scale: icon_scale,
                    },
                );
            }

            (
                0,
                IconSize {
                    size: icon_size,
                    scale: icon_scale,
                },
            )
        }
        "Threshold" => {
            let threshold = section_data
                .get("Threshold")
                .map_or(2, parse_usize!("Threshold"));

            let min_scaled_size = (icon_size - threshold) * icon_scale;
            let max_scaled_size = (icon_size + threshold) * icon_scale;

            if nominal_scaled_size < min_scaled_size {
                return (
                    nominal_scaled_size.abs_diff(min_scaled_size),
                    IconSize {
                        size: icon_size - threshold,
                        scale: icon_scale,
                    },
                );
            }

            if nominal_scaled_size > max_scaled_size {
                return (
                    nominal_scaled_size.abs_diff(max_scaled_size),
                    IconSize {
                        size: icon_size + threshold,
                        scale: icon_scale,
                    },
                );
            }

            (
                0,
                IconSize {
                    size: icon_size,
                    scale: icon_scale,
                },
            )
        }
        _ => {
            unreachable!()
        }
    }
}

// https://specifications.freedesktop.org/icon-theme/latest/#directory_layout
fn search_for_icon_theme(icon_theme: Option<&str>) -> Option<PathBuf> {
    // Implementations are required to look in the "hicolor" theme if an icon was not found in the current theme.
    let icon_theme = icon_theme.unwrap_or("hicolor");
    let search_paths = &POPULATED_ICON_THEME_SEARCH_PATHS;

    for search_path in search_paths.iter() {
        // FIXME: Differentiate between internal theme name and user-facing name?
        let theme_folder = search_path.join(icon_theme);
        if !theme_folder.is_dir() {
            continue;
        }

        // The first index.theme found while searching the base directories in order is used.
        let theme_index_file = theme_folder.join("./index.theme");
        if !theme_index_file.is_file() {
            continue;
        }

        return Some(theme_folder.canonicalize().unwrap());
    }

    None
}

fn populate_icon_theme_search_paths() -> Vec<PathBuf> {
    let mut search_paths = Vec::new();

    for search_path in UNPOPULATED_ICON_THEME_SEARCH_PATHS {
        if search_path.starts_with("$HOME") {
            let remainder = search_path.replace("$HOME", ".");

            let mut path = std::env::home_dir().unwrap_or_default();
            path.push(remainder);

            if path.exists() {
                search_paths.push(path.canonicalize().unwrap());
            }

            continue;
        }

        if search_path.starts_with("$XDG_DATA_DIRS") {
            let remainder = search_path.replace("$XDG_DATA_DIRS", ".");

            let env_variable = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
            for data_directory in std::env::split_paths(&env_variable) {
                let path = data_directory.join(&remainder);

                if path.exists() {
                    search_paths.push(path.canonicalize().unwrap());
                }
            }

            continue;
        }

        let path = PathBuf::from_str(search_path).unwrap_or_default();
        if path.exists() {
            search_paths.push(path.canonicalize().unwrap());
        }
    }

    search_paths
}

fn join_search_paths(separator: &str) -> String {
    POPULATED_ICON_THEME_SEARCH_PATHS
        .iter()
        .map(|path| path.to_str().unwrap())
        .collect::<Vec<&str>>()
        .join(separator)
}
