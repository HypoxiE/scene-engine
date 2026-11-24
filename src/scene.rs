use crate::project::{Manifest, NodeType, ScaleMode, TextAlign};
use bytemuck::{Pod, Zeroable};
use std::{collections::HashMap, ops::Range};

#[derive(Clone, Debug)]
pub struct SceneNode {
    pub id: String,
    pub kind: NodeType,
    pub asset: Option<String>,
    pub parent: Option<String>,
    /// Position in the parent's local coordinate system, or scene coordinates for root nodes.
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub rotation: f32,
    pub opacity: f32,
    pub visible: bool,
    pub z_index: i32,
    pub frame: u32,
    pub text: Option<String>,
    pub font_family: String,
    pub font_size: f32,
    pub color: [f32; 4],
    pub text_align: TextAlign,
    order: usize,
}

#[derive(Clone, Copy, Debug)]
struct FrameGrid {
    columns: u32,
    rows: u32,
}

#[derive(Debug)]
pub struct Scene {
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub scale_mode: ScaleMode,
    pub nodes: Vec<SceneNode>,
    frame_grids: HashMap<String, FrameGrid>,
}

impl Scene {
    pub fn from_manifest(manifest: &Manifest) -> Self {
        let frame_grids = manifest
            .assets
            .iter()
            .map(|asset| {
                (
                    asset.id.clone(),
                    FrameGrid {
                        columns: asset.columns,
                        rows: asset.rows,
                    },
                )
            })
            .collect();

        let nodes = manifest
            .nodes
            .iter()
            .enumerate()
            .map(|(order, node)| SceneNode {
                id: node.id.clone(),
                kind: node.kind,
                asset: node.asset.clone(),
                parent: node.parent.clone(),
                position: node.position,
                size: node.size,
                rotation: node.rotation,
                opacity: node.opacity,
                visible: node.visible,
                z_index: node.z_index,
                frame: node.frame,
                text: node.text.clone(),
                font_family: node.font_family.clone(),
                font_size: node.font_size,
                color: node.color,
                text_align: node.text_align,
                order,
            })
            .collect();

        Self {
            canvas_width: manifest.canvas.width,
            canvas_height: manifest.canvas.height,
            scale_mode: manifest.canvas.scale_mode,
            nodes,
            frame_grids,
        }
    }

    fn node_mut(&mut self, id: &str) -> Option<&mut SceneNode> {
        self.nodes.iter_mut().find(|node| node.id == id)
    }

    /// Sets a root node's scene position or a child's position relative to its parent.
    pub fn set_position(&mut self, id: &str, x: f32, y: f32) -> bool {
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        node.position = [x, y];
        true
    }

    /// Moves a node in its parent's local coordinate system.
    pub fn move_by(&mut self, id: &str, dx: f32, dy: f32) -> bool {
        if !dx.is_finite() || !dy.is_finite() {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        let next_x = node.position[0] + dx;
        let next_y = node.position[1] + dy;
        if !next_x.is_finite() || !next_y.is_finite() {
            return false;
        }
        node.position = [next_x, next_y];
        true
    }

    pub fn set_size(&mut self, id: &str, width: f32, height: f32) -> bool {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        if matches!(node.kind, NodeType::Group) {
            return false;
        }
        if matches!(node.kind, NodeType::Text) && (width > 8192.0 || height > 1024.0) {
            return false;
        }
        node.size = [width, height];
        true
    }

    pub fn set_rotation(&mut self, id: &str, angle: f32) -> bool {
        if !angle.is_finite() {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        node.rotation = angle;
        true
    }

    pub fn set_opacity(&mut self, id: &str, opacity: f32) -> bool {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        node.opacity = opacity;
        true
    }

    pub fn set_visible(&mut self, id: &str, visible: bool) -> bool {
        let Some(node) = self.node_mut(id) else { return false };
        node.visible = visible;
        true
    }

    pub fn set_z_index(&mut self, id: &str, z_index: i32) -> bool {
        let Some(node) = self.node_mut(id) else { return false };
        node.z_index = z_index;
        true
    }

    pub fn set_text(&mut self, id: &str, text: String) -> bool {
        if text.chars().count() > 4096 {
            return false;
        }
        let Some(node) = self.node_mut(id) else { return false };
        if !matches!(node.kind, NodeType::Text) {
            return false;
        }
        node.text = Some(text);
        true
    }

    pub fn set_frame(&mut self, id: &str, frame: u32) -> bool {
        let Some(node_index) = self.nodes.iter().position(|node| node.id == id) else {
            return false;
        };
        let Some(asset_id) = self.nodes[node_index].asset.as_deref() else {
            return false;
        };
        let Some(grid) = self.frame_grids.get(asset_id) else { return false };
        if frame >= grid.columns * grid.rows {
            return false;
        }
        self.nodes[node_index].frame = frame;
        true
    }

    fn frame_uv(&self, node: &SceneNode) -> [f32; 4] {
        let Some(asset_id) = node.asset.as_deref() else {
            return [0.0, 0.0, 1.0, 1.0];
        };
        let grid = self.frame_grids.get(asset_id).copied().unwrap_or(FrameGrid {
            columns: 1,
            rows: 1,
        });
        let column = node.frame % grid.columns;
        let row = node.frame / grid.columns;
        let cell_width = 1.0 / grid.columns as f32;
        let cell_height = 1.0 / grid.rows as f32;

        // Inset UVs slightly so linear texture filtering does not sample a neighboring frame.
        let inset_x = if grid.columns > 1 {
            (0.8 / (grid.columns as f32 * 64.0)).min(cell_width * 0.08)
        } else {
            0.0
        };
        let inset_y = if grid.rows > 1 {
            (0.8 / (grid.rows as f32 * 64.0)).min(cell_height * 0.08)
        } else {
            0.0
        };
        let u0 = column as f32 * cell_width + inset_x;
        let v0 = row as f32 * cell_height + inset_y;
        let u1 = (column + 1) as f32 * cell_width - inset_x;
        let v1 = (row + 1) as f32 * cell_height - inset_y;
        [u0, v0, u1, v1]
    }
}

/// A 2D affine transform: (a*x + c*y + tx, b*x + d*y + ty).
#[derive(Clone, Copy, Debug)]
struct Affine2 {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    tx: f32,
    ty: f32,
}

impl Affine2 {
    const IDENTITY: Self = Self {
        a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0,
    };

    fn translation(x: f32, y: f32) -> Self {
        Self { tx: x, ty: y, ..Self::IDENTITY }
    }

    fn rotation(angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self { a: cos, b: sin, c: -sin, d: cos, tx: 0.0, ty: 0.0 }
    }

    /// Returns a transform that applies `other` first and `self` second.
    fn multiply(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            tx: self.a * other.tx + self.c * other.ty + self.tx,
            ty: self.b * other.tx + self.d * other.ty + self.ty,
        }
    }

    fn transform_point(self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.tx, self.b * x + self.d * y + self.ty)
    }
}

#[derive(Clone, Copy, Debug)]
struct EffectiveNodeTransform {
    matrix: Affine2,
    opacity: f32,
    visible: bool,
}

impl Default for EffectiveNodeTransform {
    fn default() -> Self {
        Self { matrix: Affine2::IDENTITY, opacity: 1.0, visible: true }
    }
}

fn local_transform(node: &SceneNode) -> Affine2 {
    let [x, y] = node.position;
    let [width, height] = node.size;
    let center_x = width * 0.5;
    let center_y = height * 0.5;

    // Sprites rotate around their center. A zero-sized group rotates around its origin.
    Affine2::translation(x, y)
        .multiply(Affine2::translation(center_x, center_y))
        .multiply(Affine2::rotation(node.rotation))
        .multiply(Affine2::translation(-center_x, -center_y))
}

fn resolve_node_transform(
    index: usize,
    scene: &Scene,
    indices: &HashMap<String, usize>,
    cache: &mut [Option<EffectiveNodeTransform>],
    visiting: &mut [bool],
) -> EffectiveNodeTransform {
    if let Some(transform) = cache[index] {
        return transform;
    }
    // Project validation rejects cycles. This defensive fallback avoids recursion
    // overflow if Scene is ever constructed from an unvalidated Manifest.
    if visiting[index] {
        return EffectiveNodeTransform::default();
    }
    visiting[index] = true;

    let node = &scene.nodes[index];
    let parent_id = node.parent.clone();
    let own_transform = local_transform(node);
    let own_opacity = node.opacity;
    let own_visible = node.visible;

    let parent_transform = parent_id
        .as_ref()
        .and_then(|parent_id| indices.get(parent_id))
        .map(|&parent_index| resolve_node_transform(parent_index, scene, indices, cache, visiting))
        .unwrap_or_default();

    let resolved = EffectiveNodeTransform {
        matrix: parent_transform.matrix.multiply(own_transform),
        opacity: parent_transform.opacity * own_opacity,
        visible: parent_transform.visible && own_visible,
    };
    visiting[index] = false;
    cache[index] = Some(resolved);
    resolved
}

fn resolve_node_transforms(scene: &Scene) -> Vec<EffectiveNodeTransform> {
    let indices: HashMap<String, usize> = scene
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.clone(), index))
        .collect();
    let mut cache = vec![None; scene.nodes.len()];
    let mut visiting = vec![false; scene.nodes.len()];

    for index in 0..scene.nodes.len() {
        resolve_node_transform(index, scene, &indices, &mut cache, &mut visiting);
    }

    cache.into_iter().map(|entry| entry.unwrap_or_default()).collect()
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub opacity: f32,
}

#[derive(Debug)]
pub struct DrawCommand {
    pub asset_id: String,
    pub vertices: Range<u32>,
}

#[derive(Debug)]
pub struct SceneGeometry {
    pub vertices: Vec<Vertex>,
    pub commands: Vec<DrawCommand>,
}

impl SceneGeometry {
    pub fn build(scene: &Scene, viewport_width: u32, viewport_height: u32) -> Self {
        let width = viewport_width.max(1) as f32;
        let height = viewport_height.max(1) as f32;
        let canvas_width = scene.canvas_width as f32;
        let canvas_height = scene.canvas_height as f32;

        let (scale_x, scale_y, offset_x, offset_y) = match scene.scale_mode {
            ScaleMode::Stretch => (width / canvas_width, height / canvas_height, 0.0, 0.0),
            ScaleMode::Contain => {
                let scale = (width / canvas_width).min(height / canvas_height);
                (scale, scale, (width - canvas_width * scale) * 0.5, (height - canvas_height * scale) * 0.5)
            }
            ScaleMode::Cover => {
                let scale = (width / canvas_width).max(height / canvas_height);
                (scale, scale, (width - canvas_width * scale) * 0.5, (height - canvas_height * scale) * 0.5)
            }
        };

        let effective_transforms = resolve_node_transforms(scene);
        let mut ordered_indices: Vec<usize> = (0..scene.nodes.len()).collect();
        ordered_indices.sort_by_key(|&index| {
            let node = &scene.nodes[index];
            (node.z_index, node.order)
        });

        let mut vertices = Vec::with_capacity(ordered_indices.len() * 6);
        let mut commands = Vec::with_capacity(ordered_indices.len());

        for index in ordered_indices {
            let node = &scene.nodes[index];
            if matches!(node.kind, NodeType::Sprite | NodeType::Text) {
                append_node_quad(
                    node,
                    scene,
                    effective_transforms[index],
                    (scale_x, scale_y, offset_x, offset_y),
                    (width, height),
                    &mut vertices,
                    &mut commands,
                );
            }
        }

        Self { vertices, commands }
    }
}

fn append_node_quad(
    node: &SceneNode,
    scene: &Scene,
    effective: EffectiveNodeTransform,
    viewport_transform: (f32, f32, f32, f32),
    viewport: (f32, f32),
    vertices: &mut Vec<Vertex>,
    commands: &mut Vec<DrawCommand>,
) {
    let texture_id = match node.kind {
        NodeType::Sprite => match node.asset.as_ref() { Some(asset_id) => asset_id.clone(), None => return },
        NodeType::Text => format!("__text:{}", node.id),
        NodeType::Group => return,
    };
    let (scale_x, scale_y, offset_x, offset_y) = viewport_transform;
    let (width, height) = viewport;
    let start = vertices.len() as u32;
    let [sprite_width, sprite_height] = node.size;
    let corners = [
        effective.matrix.transform_point(0.0, 0.0),
        effective.matrix.transform_point(sprite_width, 0.0),
        effective.matrix.transform_point(sprite_width, sprite_height),
        effective.matrix.transform_point(0.0, sprite_height),
    ];
    let [u0, v0, u1, v1] = if matches!(node.kind, NodeType::Sprite) {
        scene.frame_uv(node)
    } else {
        [0.0, 0.0, 1.0, 1.0]
    };
    let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];
    let indices = [0_usize, 1, 2, 0, 2, 3];
    let opacity = if effective.visible { effective.opacity } else { 0.0 };

    for index in indices {
        let (scene_x, scene_y) = corners[index];
        let screen_x = scene_x * scale_x + offset_x;
        let screen_y = scene_y * scale_y + offset_y;
        let ndc_x = (screen_x / width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_y / height) * 2.0;
        vertices.push(Vertex {
            position: [ndc_x, ndc_y],
            uv: uvs[index],
            opacity,
        });
    }

    commands.push(DrawCommand {
        asset_id: texture_id,
        vertices: start..vertices.len() as u32,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{AssetDefinition, AssetType, Canvas, Manifest, NodeDefinition, NodeType};

    fn sprite(id: &str, position: [f32; 2], parent: Option<&str>) -> NodeDefinition {
        NodeDefinition {
            id: id.into(),
            kind: NodeType::Sprite,
            asset: Some("a".into()),
            parent: parent.map(str::to_owned),
            position,
            size: [10.0, 10.0],
            rotation: 0.0,
            opacity: 1.0,
            visible: true,
            z_index: 0,
            frame: 0,
            text: None,
            font_family: "sans-serif".into(),
            font_size: 32.0,
            color: [0.0, 0.0, 0.0, 1.0],
            text_align: TextAlign::Left,
        }
    }

    fn test_manifest(nodes: Vec<NodeDefinition>) -> Manifest {
        Manifest {
            version: 1,
            name: "test".into(),
            canvas: Canvas { width: 100, height: 100, scale_mode: ScaleMode::Stretch },
            clear_color: [0.0, 0.0, 0.0, 1.0],
            assets: vec![AssetDefinition {
                id: "a".into(), kind: AssetType::Image, path: "a.png".into(), columns: 1, rows: 1,
            }],
            nodes,
            scripts: None,
        }
    }

    #[test]
    fn invisible_nodes_keep_geometry_for_runtime_visibility_changes() {
        let mut node = sprite("sprite", [0.0, 0.0], None);
        node.visible = false;
        let scene = Scene::from_manifest(&test_manifest(vec![node]));
        let geometry = SceneGeometry::build(&scene, 100, 100);
        assert_eq!(geometry.vertices.len(), 6);
        assert_eq!(geometry.commands.len(), 1);
        assert!(geometry.vertices.iter().all(|vertex| vertex.opacity == 0.0));
    }

    #[test]
    fn runtime_can_update_text_node_content() {
        let mut text_node = sprite("clock_text", [0.0, 0.0], None);
        text_node.kind = NodeType::Text;
        text_node.asset = None;
        text_node.text = Some("00:00:00".into());
        let mut scene = Scene::from_manifest(&test_manifest(vec![text_node]));

        assert!(scene.set_text("clock_text", "12:34:56".into()));
        assert_eq!(scene.nodes[0].text.as_deref(), Some("12:34:56"));
        let geometry = SceneGeometry::build(&scene, 100, 100);
        assert_eq!(geometry.commands[0].asset_id, "__text:clock_text");
        assert!(!scene.set_text("missing", "nope".into()));
    }

    #[test]
    fn runtime_can_change_sprite_frame() {
        let mut manifest = test_manifest(vec![sprite("sprite", [0.0, 0.0], None)]);
        manifest.assets[0].columns = 10;
        let mut scene = Scene::from_manifest(&manifest);
        assert!(scene.set_frame("sprite", 9));
        assert!(!scene.set_frame("sprite", 10));
    }

    #[test]
    fn child_position_is_relative_to_parent_and_inherits_parent_movement() {
        let parent = sprite("clock", [100.0, 50.0], None);
        let child = sprite("hour_hand", [12.0, 18.0], Some("clock"));
        let mut scene = Scene::from_manifest(&test_manifest(vec![parent, child]));

        let child_index = scene.nodes.iter().position(|node| node.id == "hour_hand").unwrap();
        let mut transforms = resolve_node_transforms(&scene);
        assert_eq!(transforms[child_index].matrix.transform_point(0.0, 0.0), (112.0, 68.0));

        assert!(scene.set_position("clock", 300.0, 200.0));
        transforms = resolve_node_transforms(&scene);
        assert_eq!(transforms[child_index].matrix.transform_point(0.0, 0.0), (312.0, 218.0));
        assert_eq!(scene.nodes[child_index].position, [12.0, 18.0]);
    }

    #[test]
    fn children_inherit_visibility_and_opacity() {
        let mut parent = sprite("clock", [0.0, 0.0], None);
        parent.visible = false;
        parent.opacity = 0.5;
        let child = sprite("hand", [0.0, 0.0], Some("clock"));
        let scene = Scene::from_manifest(&test_manifest(vec![parent, child]));
        let transforms = resolve_node_transforms(&scene);
        assert!(!transforms[1].visible);
        assert!((transforms[1].opacity - 0.5).abs() < f32::EPSILON);
    }
}
