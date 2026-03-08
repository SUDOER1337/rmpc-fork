use std::sync::Arc;

use rmpc_mpd::mpd_client::AlbumArtOrder;
use rmpc_shared::paths::utils::tilde_expand;
use serde::{Deserialize, Serialize};
use strum::Display;

use super::Size;
use crate::shared::terminal::ImageBackend;

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
#[serde(default)]
pub struct AlbumArtConfigFile {
    pub method: ImageMethodFile,
    pub effect: AlbumArtEffectFile,
    pub order: AlbumArtOrderFile,
    pub max_size_px: Size,
    pub disabled_protocols: Vec<String>,
    pub vertical_align: VerticalAlignFile,
    pub horizontal_align: HorizontalAlignFile,
    pub rotation_speed_dps: f32,
    pub rotation_fps: u8,
    pub custom_loader: Option<Vec<String>>,
}

impl Default for AlbumArtConfigFile {
    fn default() -> Self {
        Self {
            method: ImageMethodFile::default(),
            effect: AlbumArtEffectFile::default(),
            order: AlbumArtOrderFile::default(),
            max_size_px: Size::default(),
            disabled_protocols: vec!["http://".to_string(), "https://".to_string()],
            vertical_align: VerticalAlignFile::default(),
            horizontal_align: HorizontalAlignFile::default(),
            rotation_speed_dps: 8.0,
            rotation_fps: 6,
            custom_loader: None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct AlbumArtConfig {
    pub method: ImageMethod,
    pub effect: AlbumArtEffect,
    pub order: AlbumArtOrder,
    pub max_size_px: Size,
    pub disabled_protocols: Vec<String>,
    pub vertical_align: VerticalAlign,
    pub horizontal_align: HorizontalAlign,
    pub rotation_speed_dps: f32,
    pub rotation_fps: u8,
    pub custom_loader: Option<Arc<Vec<String>>>,
}

#[derive(Default, Display, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum AlbumArtEffectFile {
    #[default]
    Static,
    Rotate,
}

#[derive(Default, Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlbumArtEffect {
    #[default]
    Static,
    Rotate,
}

#[derive(Default, Display, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlignFile {
    Left,
    #[default]
    Center,
    Right,
}
#[derive(Default, Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Default, Display, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum AlbumArtOrderFile {
    #[default]
    EmbeddedFirst,
    FileFirst,
    EmbeddedOnly,
    FileOnly,
}

#[derive(Default, Display, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignFile {
    Top,
    #[default]
    Center,
    Bottom,
}
#[derive(Default, Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Default, Display, Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ImageMethodFile {
    Kitty,
    UeberzugWayland,
    UeberzugX11,
    Iterm2,
    Sixel,
    Block,
    None,
    #[default]
    Auto,
}

#[derive(Default, Display, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMethod {
    Kitty,
    UeberzugWayland,
    UeberzugX11,
    Iterm2,
    Sixel,
    None,
    #[default]
    Block,
}

impl From<AlbumArtConfigFile> for AlbumArtConfig {
    fn from(value: AlbumArtConfigFile) -> Self {
        let size = value.max_size_px;
        AlbumArtConfig {
            method: ImageMethod::default(),
            effect: value.effect.into(),
            order: match value.order {
                AlbumArtOrderFile::EmbeddedFirst => AlbumArtOrder::EmbeddedFirst,
                AlbumArtOrderFile::FileFirst => AlbumArtOrder::FileFirst,
                AlbumArtOrderFile::EmbeddedOnly => AlbumArtOrder::EmbeddedOnly,
                AlbumArtOrderFile::FileOnly => AlbumArtOrder::FileOnly,
            },
            max_size_px: Size {
                width: if size.width == 0 { u16::MAX } else { size.width },
                height: if size.height == 0 { u16::MAX } else { size.height },
            },
            disabled_protocols: value.disabled_protocols,
            vertical_align: value.vertical_align.into(),
            horizontal_align: value.horizontal_align.into(),
            rotation_speed_dps: value.rotation_speed_dps.clamp(0.1, 60.0),
            rotation_fps: value.rotation_fps.clamp(1, 12),
            custom_loader: value.custom_loader.map(|arr| {
                Arc::new(arr.into_iter().map(|v| tilde_expand(&v).into_owned()).collect())
            }),
        }
    }
}

impl From<AlbumArtEffectFile> for AlbumArtEffect {
    fn from(value: AlbumArtEffectFile) -> Self {
        match value {
            AlbumArtEffectFile::Static => AlbumArtEffect::Static,
            AlbumArtEffectFile::Rotate => AlbumArtEffect::Rotate,
        }
    }
}

impl From<VerticalAlignFile> for VerticalAlign {
    fn from(value: VerticalAlignFile) -> Self {
        match value {
            VerticalAlignFile::Top => VerticalAlign::Top,
            VerticalAlignFile::Center => VerticalAlign::Center,
            VerticalAlignFile::Bottom => VerticalAlign::Bottom,
        }
    }
}

impl From<HorizontalAlignFile> for HorizontalAlign {
    fn from(value: HorizontalAlignFile) -> Self {
        match value {
            HorizontalAlignFile::Left => HorizontalAlign::Left,
            HorizontalAlignFile::Center => HorizontalAlign::Center,
            HorizontalAlignFile::Right => HorizontalAlign::Right,
        }
    }
}

impl From<ImageBackend> for ImageMethod {
    fn from(value: ImageBackend) -> Self {
        match value {
            ImageBackend::Kitty => ImageMethod::Kitty,
            ImageBackend::Iterm2 => ImageMethod::Iterm2,
            ImageBackend::Sixel => ImageMethod::Sixel,
            ImageBackend::UeberzugWayland => ImageMethod::UeberzugWayland,
            ImageBackend::UeberzugX11 => ImageMethod::UeberzugX11,
            ImageBackend::Block => ImageMethod::Block,
        }
    }
}

#[cfg(test)]
mod tests {
    use ron::de::from_str;

    use super::{AlbumArtConfig, AlbumArtConfigFile, AlbumArtEffect, AlbumArtEffectFile};

    #[test]
    fn old_configs_deserialize_without_rotation_fields() {
        let config: AlbumArtConfigFile = from_str(
            "(method: Auto, order: EmbeddedFirst, max_size_px: (width: 1200, height: 1200), disabled_protocols: [\"http://\"], vertical_align: Center, horizontal_align: Center)",
        )
        .expect("legacy config should deserialize");

        assert_eq!(config.effect, AlbumArtEffectFile::Static);
        assert_eq!(config.rotation_speed_dps, 8.0);
        assert_eq!(config.rotation_fps, 6);
    }

    #[test]
    fn rotation_values_are_clamped() {
        let config: AlbumArtConfig = AlbumArtConfigFile {
            effect: AlbumArtEffectFile::Rotate,
            rotation_speed_dps: 99.0,
            rotation_fps: 0,
            ..AlbumArtConfigFile::default()
        }
        .into();

        assert_eq!(config.effect, AlbumArtEffect::Rotate);
        assert_eq!(config.rotation_speed_dps, 60.0);
        assert_eq!(config.rotation_fps, 1);
    }
}
