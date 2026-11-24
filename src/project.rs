use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub name: String,
    pub canvas: Canvas,
    #[serde(default = "default_clear_color")]
    pub clear_color: [f64; 4],
    #[serde(default)]
    pub assets: Vec<AssetDefinition>,
    #[serde(default)]
    pub nodes: Vec<NodeDefinition>,
    #[serde(default)]
    pub scripts: Option<ScriptConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub scale_mode: ScaleMode,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScaleMode {
    Stretch,
    Contain,
    #[default]
    Cover,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetDefinition {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: AssetType,
    pub path: PathBuf,
    /// Number of animation frames arranged horizontally.
    #[serde(default = "default_frame_columns")]
    pub columns: u32,
    /// Number of animation frames arranged vertically.
    #[serde(default = "default_frame_rows")]
    pub rows: u32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetType {
    Image,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDefinition {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: NodeType,
    /// Sprite nodes reference an image asset. Text and group nodes do not.
    #[serde(default)]
    pub asset: Option<String>,
    /// Parent node whose local coordinate system this node uses.
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub position: [f32; 2],
    /// For groups this defines the rotation pivot; [0, 0] means the origin.
    #[serde(default)]
    pub size: [f32; 2],
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub z_index: i32,
    #[serde(default)]
    pub frame: u32,
    /// Initial text for text nodes. Text can be changed from Rhai with set_text().
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_text_color")]
    pub color: [f32; 4],
    #[serde(default)]
    pub text_align: TextAlign,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Sprite,
    Group,
    Text,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptConfig {
    pub entry: PathBuf,
    #[serde(default = "default_tick_rate")]
    pub tick_rate: u32,
}

fn default_clear_color() -> [f64; 4] {
    [0.025, 0.035, 0.075, 1.0]
}

fn default_opacity() -> f32 {
    1.0
}

fn default_visible() -> bool {
    true
}

fn default_frame_columns() -> u32 {
    1
}

fn default_frame_rows() -> u32 {
    1
}

fn default_tick_rate() -> u32 {
    30
}

fn default_font_family() -> String {
    "sans-serif".to_owned()
}

fn default_font_size() -> f32 {
    32.0
}

fn default_text_color() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

impl Project {
    pub fn load(path: &Path) -> Result<Self> {
        let manifest_path = if path.is_dir() {
            path.join("manifest.yaml")
        } else {
            path.to_path_buf()
        };

        let manifest_path = manifest_path
            .canonicalize()
            .with_context(|| format!("Не найден файл манифеста: {}", manifest_path.display()))?;
        let root = manifest_path
            .parent()
            .context("У манифеста отсутствует родительская директория")?
            .to_path_buf();

        let source = fs::read_to_string(&manifest_path)
            .with_context(|| format!("Не удалось прочитать {}", manifest_path.display()))?;
        let manifest: Manifest = serde_saphyr::from_str(&source)
            .with_context(|| format!("Некорректный YAML в {}", manifest_path.display()))?;

        let project = Self { root, manifest };
        project.validate()?;
        Ok(project)
    }

    fn validate_project_path(&self, path: &Path, description: &str) -> Result<PathBuf> {
        ensure!(!path.as_os_str().is_empty(), "Для {description} не указан путь");
        ensure!(
            path.components().all(|part| matches!(part, Component::Normal(_))),
            "Путь {description} должен быть относительным и не содержать '..'"
        );

        let full_path = self.root.join(path);
        let resolved_path = full_path
            .canonicalize()
            .with_context(|| format!("Не найден файл {description}: {}", full_path.display()))?;
        ensure!(
            resolved_path.starts_with(&self.root),
            "Путь {description} выходит за пределы директории проекта"
        );
        ensure!(resolved_path.is_file(), "{description} не является файлом");
        Ok(resolved_path)
    }

    fn validate(&self) -> Result<()> {
        ensure!(self.manifest.version == 1, "Неподдерживаемая версия manifest: {}", self.manifest.version);
        ensure!(!self.manifest.name.trim().is_empty(), "Поле name не должно быть пустым");
        ensure!(self.manifest.canvas.width > 0, "canvas.width должен быть больше нуля");
        ensure!(self.manifest.canvas.height > 0, "canvas.height должен быть больше нуля");
        ensure!(self.manifest.canvas.width <= 16384, "canvas.width слишком велик");
        ensure!(self.manifest.canvas.height <= 16384, "canvas.height слишком велик");

        for (index, component) in self.manifest.clear_color.iter().enumerate() {
            ensure!(component.is_finite() && (0.0..=1.0).contains(component), "clear_color[{index}] должен находиться в диапазоне 0..1");
        }

        let mut asset_ids = HashSet::new();
        for asset in &self.manifest.assets {
            ensure!(!asset.id.trim().is_empty(), "ID ресурса не должен быть пустым");
            ensure!(!asset.id.starts_with("__text:"), "ID ресурса '{}' использует зарезервированный префикс __text:", asset.id);
            ensure!(asset_ids.insert(asset.id.as_str()), "Повторяющийся ID ресурса: {}", asset.id);
            ensure!(asset.columns > 0 && asset.rows > 0, "Количество кадров ресурса '{}' должно быть больше нуля", asset.id);
            ensure!(asset.columns.checked_mul(asset.rows).is_some(), "Слишком много кадров у ресурса '{}'", asset.id);

            let resolved_path = self.validate_project_path(&asset.path, &format!("ресурс '{}'", asset.id))?;
            let image = image::image_dimensions(&resolved_path)
                .with_context(|| format!("Не удалось прочитать размеры изображения '{}'", asset.id))?;
            ensure!(image.0 % asset.columns == 0, "Ширина изображения '{}' должна делиться на columns ({})", asset.id, asset.columns);
            ensure!(image.1 % asset.rows == 0, "Высота изображения '{}' должна делиться на rows ({})", asset.id, asset.rows);
        }

        let mut node_ids = HashSet::new();
        for node in &self.manifest.nodes {
            ensure!(!node.id.trim().is_empty(), "ID объекта не должен быть пустым");
            ensure!(node_ids.insert(node.id.as_str()), "Повторяющийся ID объекта: {}", node.id);
        }

        let nodes_by_id: std::collections::HashMap<&str, &NodeDefinition> = self
            .manifest
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect();

        for node in &self.manifest.nodes {
            match node.kind {
                NodeType::Sprite => {
                    ensure!(node.text.is_none(), "У спрайта '{}' не должно быть text", node.id);
                    let asset_id = node.asset.as_deref()
                        .with_context(|| format!("У спрайта '{}' не указан asset", node.id))?;
                    let asset = self.manifest.assets.iter().find(|asset| asset.id == asset_id)
                        .with_context(|| format!("Объект '{}' ссылается на неизвестный ресурс '{}'", node.id, asset_id))?;
                    ensure!(node.frame < asset.columns * asset.rows, "Кадр {} объекта '{}' выходит за пределы ресурса '{}'", node.frame, node.id, asset_id);
                    ensure!(node.size.iter().all(|value| value.is_finite() && *value > 0.0), "size спрайта '{}' должен содержать положительные числа", node.id);
                }
                NodeType::Group => {
                    ensure!(node.asset.is_none(), "У группы '{}' не должно быть asset", node.id);
                    ensure!(node.text.is_none(), "У группы '{}' не должно быть text", node.id);
                    ensure!(node.size.iter().all(|value| value.is_finite() && *value >= 0.0), "size группы '{}' должен содержать неотрицательные числа", node.id);
                }
                NodeType::Text => {
                    ensure!(node.asset.is_none(), "У текстового объекта '{}' не должно быть asset", node.id);
                    ensure!(node.text.is_some(), "У текстового объекта '{}' не указано text", node.id);
                    ensure!(node.text.as_ref().is_some_and(|text| text.chars().count() <= 4096), "Текст объекта '{}' слишком длинный", node.id);
                    ensure!(node.size.iter().all(|value| value.is_finite() && *value > 0.0), "size текстового объекта '{}' должен содержать положительные числа", node.id);
                    ensure!(node.size[0] <= 8192.0 && node.size[1] <= 1024.0, "Текстовая область '{}' слишком велика", node.id);
                    ensure!(node.font_size.is_finite() && (1.0..=512.0).contains(&node.font_size), "font_size объекта '{}' должен находиться в диапазоне 1..512", node.id);
                    ensure!(node.color.iter().all(|value| value.is_finite() && (0.0..=1.0).contains(value)), "color объекта '{}' должен содержать RGBA в диапазоне 0..1", node.id);
                    ensure!(!node.font_family.trim().is_empty(), "font_family объекта '{}' не должен быть пустым", node.id);
                }
            }

            ensure!(node.position.iter().all(|value| value.is_finite()), "Некорректная position у объекта '{}'", node.id);
            ensure!(node.rotation.is_finite(), "Некорректный rotation у объекта '{}'", node.id);
            ensure!(node.opacity.is_finite() && (0.0..=1.0).contains(&node.opacity), "opacity объекта '{}' должен находиться в диапазоне 0..1", node.id);

            if let Some(parent_id) = node.parent.as_deref() {
                ensure!(nodes_by_id.contains_key(parent_id), "Родитель '{}' объекта '{}' не существует", parent_id, node.id);
                ensure!(parent_id != node.id.as_str(), "Объект '{}' не может быть родителем самому себе", node.id);
            }
        }

        // Parent links must form a forest. Detect cycles before the renderer
        // recursively resolves inherited transforms.
        for node in &self.manifest.nodes {
            let mut visited = HashSet::new();
            let mut current = Some(node.id.as_str());
            while let Some(id) = current {
                ensure!(visited.insert(id), "Обнаружен цикл в иерархии объектов около '{}'", id);
                current = nodes_by_id.get(id).and_then(|entry| entry.parent.as_deref());
            }
        }

        if let Some(scripts) = &self.manifest.scripts {
            ensure!((1..=240).contains(&scripts.tick_rate), "scripts.tick_rate должен находиться в диапазоне 1..240");
            self.validate_project_path(&scripts.entry, "точку входа скриптов")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scale_mode_is_cover() {
        let parsed: ScaleMode = serde_saphyr::from_str("cover").unwrap();
        assert!(matches!(parsed, ScaleMode::Cover));
    }

    #[test]
    fn default_script_tick_rate_is_thirty() {
        let parsed: ScriptConfig = serde_saphyr::from_str("entry: scripts/main.rhai").unwrap();
        assert_eq!(parsed.tick_rate, 30);
    }

    #[test]
    fn text_nodes_deserialize_with_system_font_defaults() {
        let parsed: NodeDefinition = serde_saphyr::from_str(
            "id: clock_text\ntype: text\ntext: \"12:34:56\"\nsize: [460, 110]",
        ).unwrap();
        assert!(matches!(parsed.kind, NodeType::Text));
        assert_eq!(parsed.text.as_deref(), Some("12:34:56"));
        assert_eq!(parsed.font_family, "sans-serif");
        assert_eq!(parsed.font_size, 32.0);
        assert_eq!(parsed.color, [0.0, 0.0, 0.0, 1.0]);
        assert!(matches!(parsed.text_align, TextAlign::Left));
    }

    #[test]
    fn text_nodes_support_right_alignment() {
        let parsed: NodeDefinition = serde_saphyr::from_str(
            "id: value\ntype: text\ntext: \"42%\"\nsize: [120, 56]\ntext_align: right",
        ).unwrap();
        assert!(matches!(parsed.text_align, TextAlign::Right));
    }

    #[test]
    fn group_nodes_can_omit_asset_and_size() {
        let parsed: NodeDefinition = serde_saphyr::from_str(
            "id: clock
type: group
position: [100, 50]",
        ).unwrap();
        assert!(matches!(parsed.kind, NodeType::Group));
        assert!(parsed.asset.is_none());
        assert_eq!(parsed.position, [100.0, 50.0]);
        assert_eq!(parsed.size, [0.0, 0.0]);
    }
}
