//! 模型格式转换：.bbmodel / 基岩版几何 / Java 版方块 / 通用 JSON
//!
//! 统一中间表示采用 Blockbench 坐标空间（与 .bbmodel、Java 模型一致）。
//! 基岩版的 X 轴与旋转方向与之相反，导入/导出时按 [-x, y, z] / [-rx, -ry, rz] 换算。
//! 贴图与动画随格式一起转换：bbmodel 内嵌贴图与动画，基岩版拆分为 .geo.json +
//! .animation.json + 贴图，Java 版为单轴旋转方块模型（不支持动画）。

use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

pub const FACE_KEYS: [&str; 6] = ["east", "west", "up", "down", "south", "north"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Original,
    Bbmodel,
    Bedrock,
    Java,
    Generic,
}

impl Format {
    pub fn parse(text: &str) -> Format {
        match text.trim().to_ascii_lowercase().as_str() {
            "bbmodel" => Format::Bbmodel,
            "bedrock" | "geo" | "geo.json" => Format::Bedrock,
            "java" => Format::Java,
            "generic" => Format::Generic,
            _ => Format::Original,
        }
    }

    pub fn is_original(self) -> bool {
        matches!(self, Format::Original)
    }

    pub fn name(self) -> &'static str {
        match self {
            Format::Original => "original",
            Format::Bbmodel => "bbmodel",
            Format::Bedrock => "bedrock",
            Format::Java => "java",
            Format::Generic => "generic",
        }
    }
}

pub struct Converted {
    pub files: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct FaceUv {
    rect: [f32; 4],
    rotation: f32,
    texture: usize,
}

#[derive(Clone, Debug, Default)]
struct Texture {
    name: String,
    data: Vec<u8>,
    width: u32,
    height: u32,
}

#[derive(Clone, Debug)]
struct Bone {
    name: String,
    parent: Option<usize>,
    pivot: [f32; 3],
    rotation: [f32; 3],
}

#[derive(Clone, Debug)]
struct Cube {
    name: String,
    bone: Option<usize>,
    from: [f32; 3],
    to: [f32; 3],
    origin: [f32; 3],
    rotation: [f32; 3],
    inflate: f32,
    mirror: bool,
    faces: [Option<FaceUv>; 6],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Channel {
    Rotation,
    Position,
    Scale,
}

impl Channel {
    fn key(self) -> &'static str {
        match self {
            Channel::Rotation => "rotation",
            Channel::Position => "position",
            Channel::Scale => "scale",
        }
    }
}

struct AnimChannel {
    bone: usize,
    channel: Channel,
    keys: Vec<(f32, [f32; 3])>,
}

struct Animation {
    name: String,
    length: f32,
    looping: bool,
    channels: Vec<AnimChannel>,
}

#[derive(Default)]
struct Doc {
    res: [f32; 2],
    bones: Vec<Bone>,
    cubes: Vec<Cube>,
    textures: Vec<Texture>,
    animations: Vec<Animation>,
    warnings: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bbmodel,
    Bedrock,
    Java,
    Generic,
}

// ===== 数值辅助 =====

fn num_value(value: &Value) -> Option<f32> {
    if let Some(v) = value.as_f64() {
        return Some(v as f32);
    }
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if let Ok(v) = trimmed.parse::<f64>() {
            return Some(v as f32);
        }
    }
    None
}

fn vec3_opt(value: Option<&Value>) -> Option<[f32; 3]> {
    let list = value?.as_array()?;
    if list.len() < 3 {
        return None;
    }
    Some([
        num_value(&list[0])?,
        num_value(&list[1])?,
        num_value(&list[2])?,
    ])
}

fn fnum(value: f32) -> Value {
    let rounded = (value as f64 * 10000.0).round() / 10000.0;
    let rounded = if rounded == -0.0 { 0.0 } else { rounded };
    match serde_json::Number::from_f64(rounded) {
        Some(number) => Value::Number(number),
        None => json!(0),
    }
}

fn v3(value: [f32; 3]) -> Value {
    json!([fnum(value[0]), fnum(value[1]), fnum(value[2])])
}

fn r4(rect: [f32; 4]) -> Value {
    json!([
        fnum(rect[0]),
        fnum(rect[1]),
        fnum(rect[2]),
        fnum(rect[3])
    ])
}

fn mirror_pos(value: [f32; 3]) -> [f32; 3] {
    [-value[0], value[1], value[2]]
}

fn mirror_rot(value: [f32; 3]) -> [f32; 3] {
    [-value[0], -value[1], value[2]]
}

fn make_uuid(seed: usize) -> String {
    let hash = seed.wrapping_mul(2654435761);
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        hash as u32,
        (hash >> 8) as u16,
        (seed & 0xfff) as u16,
        ((seed >> 4) & 0xfff) as u16,
        (seed as u64).wrapping_mul(11400714819323198485)
            & 0xffff_ffff_ffff
    )
}

fn fmt_time(time: f32) -> String {
    let value = (time as f64 * 1000.0).round() / 1000.0;
    if value == value.trunc() {
        format!("{value:.1}")
    } else {
        let text = format!("{value:.3}");
        text.trim_end_matches('0').to_string()
    }
}

fn texture_ext(name: &str) -> String {
    let ext = name
        .rsplit('.')
        .next()
        .unwrap_or("png")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => ext,
        _ => "png".to_string(),
    }
}

fn texture_mime(name: &str) -> &'static str {
    match texture_ext(name).as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/png",
    }
}

/// 读取 PNG 尺寸（仅解析 IHDR）
fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 24 || &data[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn texture_from_file(path: &Path) -> Texture {
    let data = fs::read(path).unwrap_or_default();
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let (width, height) = image_size(&data).unwrap_or((0, 0));
    Texture {
        name,
        data,
        width,
        height,
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("读取文件失败: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 JSON 失败: {e}"))
}

fn version_lt(version: &str, target: &str) -> bool {
    let parse = |text: &str| -> Vec<i64> {
        text.split('.')
            .map(|part| part.trim().parse::<i64>().unwrap_or(0))
            .collect()
    };
    let a = parse(version);
    let b = parse(target);
    for index in 0..3 {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return left < right;
        }
    }
    false
}

/// Blockbench / 基岩版共用的 Box UV 展开公式（面顺序：east/west/up/down/south/north）
fn box_rects(size: [f32; 3], uv: [f32; 2], mirror_uv: bool) -> [[f32; 4]; 6] {
    let w = (size[0] + 1e-7).floor().max(0.0);
    let h = (size[1] + 1e-7).floor().max(0.0);
    let d = (size[2] + 1e-7).floor().max(0.0);
    let (u, v) = (uv[0], uv[1]);
    let mut rects = [
        [u, v + d, u + d, v + d + h],
        [u + d + w, v + d, u + 2.0 * d + w, v + d + h],
        [u + d + w, v + d, u + d, v],
        [u + 2.0 * d + w, v, u + d + w, v + d],
        [u + 2.0 * d + w, v + d, u + 2.0 * d + 2.0 * w, v + d + h],
        [u + d, v + d, u + d + w, v + d + h],
    ];
    if mirror_uv {
        for rect in rects.iter_mut() {
            rect.swap(0, 2);
        }
        rects.swap(0, 1);
    }
    rects
}

fn detect_kind(ext: &str, json: &Value) -> Result<Kind, String> {
    if ext == "bbmodel" {
        return Ok(Kind::Bbmodel);
    }
    if ext != "json" {
        return Err("该格式暂不支持转换（仅支持 .bbmodel 与 .json）".to_string());
    }
    if json.get("meta").is_some() && json.get("elements").is_some() {
        return Ok(Kind::Generic);
    }
    let has_geo = json.get("minecraft:geometry").is_some()
        || json.get("bones").is_some()
        || json
            .as_object()
            .map(|object| object.keys().any(|key| key.starts_with("geometry.")))
            .unwrap_or(false);
    if has_geo {
        return Ok(Kind::Bedrock);
    }
    if json.get("elements").is_some() || json.get("textures").is_some() {
        return Ok(Kind::Java);
    }
    Err("无法识别的模型 JSON 结构".to_string())
}

fn resolve_geometry(json: &Value) -> Option<Value> {
    match json.get("minecraft:geometry") {
        Some(Value::Array(list)) => list.first().cloned(),
        Some(Value::Object(_)) => json.get("minecraft:geometry").cloned(),
        _ => {
            if let Some(object) = json.as_object() {
                if let Some((_, value)) = object
                    .iter()
                    .find(|(key, _)| key.starts_with("geometry."))
                {
                    return Some(value.clone());
                }
            }
            if json.get("bones").is_some() {
                return Some(json.clone());
            }
            None
        }
    }
}

// ===== 解析：.bbmodel / 通用 JSON =====

struct OutlinerCtx<'a> {
    bones: &'a mut Vec<Bone>,
    groups: &'a HashMap<String, Value>,
    cube_of_uuid: &'a HashMap<String, usize>,
    bone_of_cube: &'a mut HashMap<usize, usize>,
    uuid_bone: &'a mut HashMap<String, usize>,
    used_names: &'a mut HashSet<String>,
}

fn walk_outliner(nodes: &[Value], parent: Option<usize>, ctx: &mut OutlinerCtx, depth: usize) {
    if depth > 64 {
        return;
    }
    for node in nodes {
        match node {
            Value::String(key) => {
                if let Some(&cube) = ctx.cube_of_uuid.get(key) {
                    if let Some(bone) = parent {
                        ctx.bone_of_cube.insert(cube, bone);
                    }
                } else if let Some(group) = ctx.groups.get(key).cloned() {
                    walk_group(&group, parent, ctx, depth + 1);
                }
            }
            Value::Object(_) => walk_group(node, parent, ctx, depth + 1),
            _ => {}
        }
    }
}

fn walk_group(node: &Value, parent: Option<usize>, ctx: &mut OutlinerCtx, depth: usize) {
    if depth > 64 {
        return;
    }
    let data = if node.get("name").is_some() {
        node.clone()
    } else if let Some(uuid) = node.get("uuid").and_then(|value| value.as_str()) {
        ctx.groups
            .get(uuid)
            .cloned()
            .unwrap_or_else(|| node.clone())
    } else {
        node.clone()
    };

    let base = data
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("group")
        .to_string();
    let mut name = base.clone();
    let mut index = 1;
    while !ctx.used_names.insert(name.to_lowercase()) {
        index += 1;
        name = format!("{base}_{index}");
    }

    ctx.bones.push(Bone {
        name,
        parent,
        pivot: vec3_opt(data.get("origin")).unwrap_or([0.0, 0.0, 0.0]),
        rotation: vec3_opt(data.get("rotation")).unwrap_or([0.0, 0.0, 0.0]),
    });
    let bone_index = ctx.bones.len() - 1;
    if let Some(uuid) = data.get("uuid").and_then(|value| value.as_str()) {
        ctx.uuid_bone.insert(uuid.to_string(), bone_index);
    }

    let children = node
        .get("children")
        .or_else(|| data.get("children"))
        .cloned();
    if let Some(Value::Array(list)) = children {
        walk_outliner(&list, Some(bone_index), ctx, depth + 1);
    }
}

fn texture_index(value: &Value, textures: &[Texture]) -> usize {
    if let Some(index) = value.as_u64() {
        return index as usize;
    }
    if let Some(text) = value.as_str() {
        if let Ok(index) = text.parse::<usize>() {
            return index;
        }
        if let Some(index) = textures.iter().position(|item| item.name == text) {
            return index;
        }
    }
    0
}

fn parse_bbmodel(json: &Value, texture_paths: &[PathBuf]) -> Result<Doc, String> {
    let mut doc = Doc::default();
    let resolution = json.get("resolution");
    let mut width = resolution
        .and_then(|value| value.get("width"))
        .and_then(num_value)
        .unwrap_or(0.0);
    let mut height = resolution
        .and_then(|value| value.get("height"))
        .and_then(num_value)
        .unwrap_or(0.0);

    if let Some(list) = json.get("textures").and_then(|value| value.as_array()) {
        for (index, item) in list.iter().enumerate() {
            let name = item
                .get("name")
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("texture{index}.png"));
            let mut data = Vec::new();
            if let Some(source) = item.get("source").and_then(|value| value.as_str()) {
                if let Some((_, encoded)) = source.split_once("base64,") {
                    if let Ok(bytes) = STANDARD.decode(encoded.trim()) {
                        data = bytes;
                    }
                }
            }
            if data.is_empty() {
                if let Some(path) = texture_paths.get(index) {
                    data = fs::read(path).unwrap_or_default();
                }
            }
            if width <= 0.0 || height <= 0.0 {
                if let Some((w, h)) = image_size(&data) {
                    width = w as f32;
                    height = h as f32;
                }
            }
            let (tw, th) = image_size(&data).unwrap_or((width as u32, height as u32));
            doc.textures.push(Texture {
                name,
                data,
                width: tw,
                height: th,
            });
        }
    }
    if doc.textures.is_empty() {
        for path in texture_paths {
            doc.textures.push(texture_from_file(path));
        }
    }
    if width <= 0.0 || height <= 0.0 {
        if let Some(texture) = doc.textures.first() {
            if texture.width > 0 && texture.height > 0 {
                width = texture.width as f32;
                height = texture.height as f32;
            }
        }
    }
    if width <= 0.0 {
        width = 16.0;
    }
    if height <= 0.0 {
        height = 16.0;
    }
    doc.res = [width, height];

    let default_box_uv = json
        .get("meta")
        .and_then(|value| value.get("box_uv"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let elements = json
        .get("elements")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut cube_of_uuid: HashMap<String, usize> = HashMap::new();

    for element in &elements {
        let Some(from_raw) = vec3_opt(element.get("from")) else {
            continue;
        };
        let Some(to_raw) = vec3_opt(element.get("to")) else {
            continue;
        };
        let from = [
            from_raw[0].min(to_raw[0]),
            from_raw[1].min(to_raw[1]),
            from_raw[2].min(to_raw[2]),
        ];
        let to = [
            from_raw[0].max(to_raw[0]),
            from_raw[1].max(to_raw[1]),
            from_raw[2].max(to_raw[2]),
        ];
        let size = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
        let origin = vec3_opt(element.get("origin")).unwrap_or([
            (from[0] + to[0]) / 2.0,
            (from[1] + to[1]) / 2.0,
            (from[2] + to[2]) / 2.0,
        ]);
        let rotation = vec3_opt(element.get("rotation")).unwrap_or([0.0, 0.0, 0.0]);
        let inflate = element.get("inflate").and_then(num_value).unwrap_or(0.0);
        let mirror = element
            .get("mirror_uv")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let box_uv = element
            .get("box_uv")
            .and_then(|value| value.as_bool())
            .unwrap_or(default_box_uv);

        let mut faces: [Option<FaceUv>; 6] = std::array::from_fn(|_| None);
        let mut has_face = false;
        if let Some(face_map) = element.get("faces").and_then(|value| value.as_object()) {
            for (face_index, key) in FACE_KEYS.iter().enumerate() {
                let Some(face) = face_map.get(*key) else {
                    continue;
                };
                let texture_value = face.get("texture");
                if matches!(texture_value, None | Some(Value::Null) | Some(Value::Bool(false))) {
                    continue;
                }
                let Some(uv) = face.get("uv").and_then(|value| value.as_array()) else {
                    continue;
                };
                if uv.len() < 4 {
                    continue;
                }
                let mut rect = [0.0f32; 4];
                let mut valid = true;
                for index in 0..4 {
                    match num_value(&uv[index]) {
                        Some(value) => rect[index] = value,
                        None => valid = false,
                    }
                }
                if !valid {
                    continue;
                }
                let rotation = face.get("rotation").and_then(num_value).unwrap_or(0.0);
                let texture = texture_value
                    .map(|value| texture_index(value, &doc.textures))
                    .unwrap_or(0);
                faces[face_index] = Some(FaceUv {
                    rect,
                    rotation,
                    texture,
                });
                has_face = true;
            }
        }

        if !has_face && box_uv {
            let uv = element
                .get("uv_offset")
                .and_then(|value| value.as_array())
                .and_then(|list| {
                    Some([
                        num_value(list.first()?)?,
                        num_value(list.get(1)?)?,
                    ])
                })
                .unwrap_or([0.0, 0.0]);
            let rects = box_rects(size, uv, mirror);
            for index in 0..6 {
                faces[index] = Some(FaceUv {
                    rect: rects[index],
                    rotation: 0.0,
                    texture: 0,
                });
            }
            has_face = true;
        }
        if !has_face {
            continue;
        }

        let name = element
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("cube")
            .to_string();
        let index = doc.cubes.len();
        doc.cubes.push(Cube {
            name,
            bone: None,
            from,
            to,
            origin,
            rotation,
            inflate,
            mirror,
            faces,
        });
        if let Some(uuid) = element.get("uuid").and_then(|value| value.as_str()) {
            cube_of_uuid.insert(uuid.to_string(), index);
        }
    }

    if doc.cubes.is_empty() {
        return Err("模型中没有可转换的方块".to_string());
    }

    let mut groups: HashMap<String, Value> = HashMap::new();
    if let Some(list) = json.get("groups").and_then(|value| value.as_array()) {
        for group in list {
            if let Some(uuid) = group.get("uuid").and_then(|value| value.as_str()) {
                groups.insert(uuid.to_string(), group.clone());
            }
        }
    }

    let outliner = match json.get("outliner").and_then(|value| value.as_array()) {
        Some(list) if !list.is_empty() => list.clone(),
        _ => cube_of_uuid.keys().map(|key| json!(key)).collect::<Vec<_>>(),
    };

    let mut bone_of_cube: HashMap<usize, usize> = HashMap::new();
    let mut uuid_bone: HashMap<String, usize> = HashMap::new();
    let mut used_names: HashSet<String> = HashSet::new();
    {
        let mut ctx = OutlinerCtx {
            bones: &mut doc.bones,
            groups: &groups,
            cube_of_uuid: &cube_of_uuid,
            bone_of_cube: &mut bone_of_cube,
            uuid_bone: &mut uuid_bone,
            used_names: &mut used_names,
        };
        walk_outliner(&outliner, None, &mut ctx, 0);
    }
    for (index, cube) in doc.cubes.iter_mut().enumerate() {
        cube.bone = bone_of_cube.get(&index).copied();
    }

    parse_bbmodel_animations(json, &mut doc, &uuid_bone);
    Ok(doc)
}

fn parse_bbmodel_animations(json: &Value, doc: &mut Doc, uuid_bone: &HashMap<String, usize>) {
    let legacy = version_lt(
        json
            .get("meta")
            .and_then(|value| value.get("format_version"))
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        "5.0",
    );
    let list: Vec<Value> = match json.get("animations") {
        Some(Value::Array(list)) => list.clone(),
        Some(Value::Object(map)) => map.values().cloned().collect(),
        _ => Vec::new(),
    };
    let bone_by_name: HashMap<String, usize> = doc
        .bones
        .iter()
        .enumerate()
        .map(|(index, bone)| (bone.name.to_lowercase(), index))
        .collect();

    for item in list {
        let name = item
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("animation")
            .to_string();
        let loop_mode = item
            .get("loop")
            .and_then(|value| value.as_str())
            .unwrap_or("once");
        let looping = loop_mode == "loop" || loop_mode == "hold";
        let mut length = item
            .get("animation_length")
            .and_then(num_value)
            .or_else(|| item.get("length").and_then(num_value))
            .unwrap_or(0.0);

        let mut channels: Vec<AnimChannel> = Vec::new();
        let mut max_time: f32 = 0.0;
        if let Some(animators) = item.get("animators").and_then(|value| value.as_object()) {
            for (key, animator) in animators {
                let Some(keyframes) = animator.get("keyframes").and_then(|value| value.as_array())
                else {
                    continue;
                };
                let bone_index = uuid_bone.get(key).copied().or_else(|| {
                    animator
                        .get("name")
                        .and_then(|value| value.as_str())
                        .and_then(|name| bone_by_name.get(&name.to_lowercase()).copied())
                });
                let Some(bone_index) = bone_index else {
                    continue;
                };

                let mut buckets: [Vec<(f32, [f32; 3])>; 3] = Default::default();
                for keyframe in keyframes {
                    let channel_index = match keyframe
                        .get("channel")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                    {
                        "rotation" => 0,
                        "position" => 1,
                        "scale" => 2,
                        _ => continue,
                    };
                    let time = keyframe.get("time").and_then(num_value).unwrap_or(0.0);
                    let point = keyframe
                        .get("data_points")
                        .and_then(|value| value.as_array())
                        .and_then(|list| list.first())
                        .cloned()
                        .unwrap_or_else(|| keyframe.clone());
                    let (Some(x), Some(y), Some(z)) = (
                        point.get("x").and_then(num_value),
                        point.get("y").and_then(num_value),
                        point.get("z").and_then(num_value),
                    ) else {
                        continue;
                    };
                    let mut values = [x, y, z];
                    if legacy && channel_index == 0 {
                        values[0] = -values[0];
                        values[1] = -values[1];
                    } else if legacy && channel_index == 1 {
                        values[0] = -values[0];
                    }
                    buckets[channel_index].push((time, values));
                    max_time = max_time.max(time);
                }

                for (index, keys) in buckets.iter_mut().enumerate() {
                    if keys.is_empty() {
                        continue;
                    }
                    keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                    let channel = match index {
                        0 => Channel::Rotation,
                        1 => Channel::Position,
                        _ => Channel::Scale,
                    };
                    channels.push(AnimChannel {
                        bone: bone_index,
                        channel,
                        keys: keys.clone(),
                    });
                }
            }
        }

        if channels.is_empty() {
            continue;
        }
        if length <= 0.0 {
            length = max_time;
        }
        if length <= 0.0 {
            continue;
        }
        doc.animations.push(Animation {
            name,
            length,
            looping,
            channels,
        });
    }
}

// ===== 解析：基岩版 =====

fn parse_bedrock(
    json: &Value,
    texture_paths: &[PathBuf],
    animation_paths: &[PathBuf],
) -> Result<Doc, String> {
    let geometry = resolve_geometry(json).ok_or_else(|| "无法识别的基岩版几何 JSON".to_string())?;
    let description = geometry.get("description").cloned().unwrap_or(json!({}));
    let mut width = description
        .get("texture_width")
        .and_then(num_value)
        .or_else(|| geometry.get("texturewidth").and_then(num_value))
        .unwrap_or(0.0);
    let mut height = description
        .get("texture_height")
        .and_then(num_value)
        .or_else(|| geometry.get("textureheight").and_then(num_value))
        .unwrap_or(0.0);

    let mut doc = Doc::default();
    for path in texture_paths {
        doc.textures.push(texture_from_file(path));
    }
    if width <= 0.0 || height <= 0.0 {
        if let Some(texture) = doc.textures.first() {
            if texture.width > 0 && texture.height > 0 {
                width = texture.width as f32;
                height = texture.height as f32;
            }
        }
    }
    if width <= 0.0 {
        width = 64.0;
    }
    if height <= 0.0 {
        height = 64.0;
    }
    doc.res = [width, height];

    let bones_list = geometry
        .get("bones")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut used_names: HashSet<String> = HashSet::new();
    let mut name_to_index: HashMap<String, usize> = HashMap::new();
    for bone in &bones_list {
        let base = bone
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("bone")
            .to_string();
        let mut name = base.clone();
        let mut index = 1;
        while !used_names.insert(name.to_lowercase()) {
            index += 1;
            name = format!("{base}_{index}");
        }
        name_to_index
            .entry(base.to_lowercase())
            .or_insert(doc.bones.len());
        doc.bones.push(Bone {
            name,
            parent: None,
            pivot: mirror_pos(vec3_opt(bone.get("pivot")).unwrap_or([0.0, 0.0, 0.0])),
            rotation: mirror_rot(vec3_opt(bone.get("rotation")).unwrap_or([0.0, 0.0, 0.0])),
        });
    }
    for (index, bone) in bones_list.iter().enumerate() {
        if let Some(parent) = bone.get("parent").and_then(|value| value.as_str()) {
            if let Some(&parent_index) = name_to_index.get(&parent.to_lowercase()) {
                if parent_index != index {
                    doc.bones[index].parent = Some(parent_index);
                }
            }
        }
    }

    for (bone_index, bone) in bones_list.iter().enumerate() {
        let Some(cubes) = bone.get("cubes").and_then(|value| value.as_array()) else {
            continue;
        };
        for cube in cubes {
            parse_bedrock_cube(cube, bone_index, &mut doc);
        }
    }
    if doc.cubes.is_empty() {
        return Err("模型中没有可转换的方块".to_string());
    }

    for path in animation_paths {
        if let Ok(animation_json) = read_json(path) {
            parse_bedrock_animations(&animation_json, &mut doc, &name_to_index);
        }
    }
    Ok(doc)
}

fn parse_bedrock_cube(cube: &Value, bone: usize, doc: &mut Doc) {
    let Some(origin) = vec3_opt(cube.get("origin")) else {
        return;
    };
    let Some(size_raw) = vec3_opt(cube.get("size")) else {
        return;
    };
    let size = [
        size_raw[0].abs(),
        size_raw[1].abs(),
        size_raw[2].abs(),
    ];
    let from = [-(origin[0] + size[0]), origin[1], origin[2]];
    let to = [-origin[0], origin[1] + size[1], origin[2] + size[2]];
    let has_pivot = cube.get("pivot").is_some();
    let pivot = vec3_opt(cube.get("pivot")).unwrap_or([
        (from[0] + to[0]) / 2.0,
        (from[1] + to[1]) / 2.0,
        (from[2] + to[2]) / 2.0,
    ]);
    let origin_value = mirror_pos(pivot);
    let rotation = mirror_rot(vec3_opt(cube.get("rotation")).unwrap_or([0.0, 0.0, 0.0]));
    let inflate = cube.get("inflate").and_then(num_value).unwrap_or(0.0);
    let mirror = cube
        .get("mirror")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let mut faces: [Option<FaceUv>; 6] = std::array::from_fn(|_| None);
    match cube.get("uv") {
        Some(Value::Array(list)) if list.len() >= 2 => {
            let uv = [
                num_value(&list[0]).unwrap_or(0.0),
                num_value(&list[1]).unwrap_or(0.0),
            ];
            let rects = box_rects(size, uv, false);
            for index in 0..6 {
                faces[index] = Some(FaceUv {
                    rect: rects[index],
                    rotation: 0.0,
                    texture: 0,
                });
            }
        }
        Some(Value::Object(map)) if !map.is_empty() => {
            for (face_index, key) in FACE_KEYS.iter().enumerate() {
                let Some(item) = map.get(*key) else {
                    continue;
                };
                let (u, v, w, h) = if let Some(list) = item.as_array() {
                    let default = default_face_size(size, key);
                    (
                        list.first().and_then(num_value).unwrap_or(0.0),
                        list.get(1).and_then(num_value).unwrap_or(0.0),
                        default[0],
                        default[1],
                    )
                } else {
                    let u = item
                        .get("uv")
                        .and_then(|value| value.as_array())
                        .and_then(|list| list.first())
                        .and_then(num_value)
                        .unwrap_or(0.0);
                    let v = item
                        .get("uv")
                        .and_then(|value| value.as_array())
                        .and_then(|list| list.get(1))
                        .and_then(num_value)
                        .unwrap_or(0.0);
                    let (w, h) = match item.get("uv_size").and_then(|value| value.as_array()) {
                        Some(list) if list.len() >= 2 => (
                            list.first().and_then(num_value).unwrap_or(0.0),
                            list.get(1).and_then(num_value).unwrap_or(0.0),
                        ),
                        _ => {
                            let default = default_face_size(size, key);
                            (default[0], default[1])
                        }
                    };
                    (u, v, w, h)
                };
                let mut rect = [u, v, u + w, v + h];
                if *key == "up" || *key == "down" {
                    rect = [rect[2], rect[3], rect[0], rect[1]];
                }
                faces[face_index] = Some(FaceUv {
                    rect,
                    rotation: 0.0,
                    texture: 0,
                });
            }
        }
        _ => return,
    }
    if faces.iter().all(|face| face.is_none()) {
        return;
    }

    if !has_pivot {
        doc.warnings.push("部分方块缺少枢轴，已使用方块中心".to_string());
    }

    doc.cubes.push(Cube {
        name: cube
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("cube")
            .to_string(),
        bone: Some(bone),
        from,
        to,
        origin: origin_value,
        rotation,
        inflate,
        mirror,
        faces,
    });
}

fn default_face_size(size: [f32; 3], face: &str) -> [f32; 2] {
    match face {
        "east" | "west" => [size[2], size[1]],
        "north" | "south" => [size[0], size[1]],
        _ => [size[0], size[2]],
    }
}

fn bedrock_value(value: &Value) -> Option<[f32; 3]> {
    if let Some(list) = value.as_array() {
        if list.len() >= 3 {
            return vec3_opt(Some(value));
        }
        if list.len() == 1 {
            let single = num_value(&list[0])?;
            return Some([single, single, single]);
        }
        return None;
    }
    if let Some(single) = num_value(value) {
        return Some([single, single, single]);
    }
    None
}

fn bedrock_channel_keys(value: &Value) -> Option<Vec<(f32, [f32; 3])>> {
    match value {
        Value::Array(_) | Value::Number(_) | Value::String(_) => {
            Some(vec![(0.0, bedrock_value(value)?)])
        }
        Value::Object(map) => {
            if map.contains_key("pre") || map.contains_key("post") {
                let inner = map.get("post").or_else(|| map.get("pre"))?;
                return Some(vec![(0.0, bedrock_value(inner)?)]);
            }
            let mut keys = Vec::new();
            for (time_text, raw) in map {
                let Ok(time) = time_text.parse::<f32>() else {
                    continue;
                };
                let inner = if raw.is_object() {
                    raw.get("post").or_else(|| raw.get("pre")).unwrap_or(raw)
                } else {
                    raw
                };
                if let Some(values) = bedrock_value(inner) {
                    keys.push((time, values));
                }
            }
            if keys.is_empty() {
                return None;
            }
            keys.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            Some(keys)
        }
        _ => None,
    }
}

fn parse_bedrock_animations(
    json: &Value,
    doc: &mut Doc,
    name_to_index: &HashMap<String, usize>,
) {
    let Some(animations) = json.get("animations").and_then(|value| value.as_object()) else {
        return;
    };
    for (name, animation) in animations {
        let Some(bones) = animation.get("bones").and_then(|value| value.as_object()) else {
            continue;
        };
        let looping = animation
            .get("loop")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let mut length = animation
            .get("animation_length")
            .and_then(num_value)
            .unwrap_or(0.0);
        let mut channels = Vec::new();
        let mut max_time: f32 = 0.0;

        for (bone_name, channel_map) in bones {
            let Some(&bone_index) = name_to_index.get(&bone_name.to_lowercase()) else {
                continue;
            };
            for (key, channel, is_rotation) in [
                ("rotation", Channel::Rotation, true),
                ("position", Channel::Position, false),
                ("scale", Channel::Scale, false),
            ] {
                let Some(raw) = channel_map.get(key) else {
                    continue;
                };
                let Some(keys) = bedrock_channel_keys(raw) else {
                    continue;
                };
                let keys: Vec<(f32, [f32; 3])> = keys
                    .into_iter()
                    .map(|(time, values)| {
                        max_time = max_time.max(time);
                        if is_rotation {
                            (time, mirror_rot(values))
                        } else if matches!(channel, Channel::Position) {
                            (time, mirror_pos(values))
                        } else {
                            (time, values)
                        }
                    })
                    .collect();
                if keys.is_empty() {
                    continue;
                }
                channels.push(AnimChannel {
                    bone: bone_index,
                    channel,
                    keys,
                });
            }
        }

        if channels.is_empty() {
            continue;
        }
        if length <= 0.0 {
            length = max_time;
        }
        if length <= 0.0 {
            continue;
        }
        doc.animations.push(Animation {
            name: name.clone(),
            length,
            looping,
            channels,
        });
    }
}

// ===== 解析：Java Block/Item =====

fn parse_java(json: &Value, texture_paths: &[PathBuf]) -> Result<Doc, String> {
    let mut doc = Doc::default();
    for path in texture_paths {
        doc.textures.push(texture_from_file(path));
    }
    let texture_size = json
        .get("texture_size")
        .and_then(|value| value.as_array())
        .and_then(|list| {
            Some([
                list.first().and_then(num_value)?,
                list.get(1).and_then(num_value)?,
            ])
        });
    let mut width = texture_size.map(|size| size[0]).unwrap_or(0.0);
    let mut height = texture_size.map(|size| size[1]).unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 {
        if let Some(texture) = doc.textures.first() {
            if texture.width > 0 && texture.height > 0 {
                width = texture.width as f32;
                height = texture.height as f32;
            }
        }
    }
    if width <= 0.0 {
        width = 16.0;
    }
    if height <= 0.0 {
        height = 16.0;
    }
    doc.res = [width, height];

    let elements = json
        .get("elements")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    for element in &elements {
        let Some(from_raw) = vec3_opt(element.get("from")) else {
            continue;
        };
        let Some(to_raw) = vec3_opt(element.get("to")) else {
            continue;
        };
        let inflate = element.get("inflate").and_then(num_value).unwrap_or(0.0);
        let from = [
            from_raw[0].min(to_raw[0]) - 8.0,
            from_raw[1].min(to_raw[1]),
            from_raw[2].min(to_raw[2]) - 8.0,
        ];
        let to = [
            from_raw[0].max(to_raw[0]) - 8.0,
            from_raw[1].max(to_raw[1]),
            from_raw[2].max(to_raw[2]) - 8.0,
        ];

        let rotation = element.get("rotation");
        let mut angles = [0.0f32; 3];
        let mut origin = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0, (from[2] + to[2]) / 2.0];
        if let Some(rotation) = rotation {
            if let Some(angle) = rotation.get("angle").and_then(num_value) {
                let axis = rotation
                    .get("axis")
                    .and_then(|value| value.as_str())
                    .unwrap_or("y");
                let index = match axis {
                    "x" => 0,
                    "z" => 2,
                    _ => 1,
                };
                angles[index] = angle;
            } else if let Some(values) = vec3_opt(Some(rotation)) {
                angles = values;
            }
            if let Some(values) = vec3_opt(rotation.get("origin")) {
                origin = [values[0] - 8.0, values[1], values[2] - 8.0];
            }
        }

        let mut faces: [Option<FaceUv>; 6] = std::array::from_fn(|_| None);
        let mut has_face = false;
        if let Some(face_map) = element.get("faces").and_then(|value| value.as_object()) {
            for (face_index, key) in FACE_KEYS.iter().enumerate() {
                let Some(face) = face_map.get(*key) else {
                    continue;
                };
                let Some(uv) = face.get("uv").and_then(|value| value.as_array()) else {
                    continue;
                };
                if uv.len() < 4 {
                    continue;
                }
                let mut rect = [0.0f32; 4];
                let mut valid = true;
                for index in 0..4 {
                    match num_value(&uv[index]) {
                        Some(value) => rect[index] = value * doc.res[index % 2] / 16.0,
                        None => valid = false,
                    }
                }
                if !valid {
                    continue;
                }
                let rotation = face.get("rotation").and_then(num_value).unwrap_or(0.0);
                faces[face_index] = Some(FaceUv {
                    rect,
                    rotation,
                    texture: 0,
                });
                has_face = true;
            }
        }
        if !has_face {
            continue;
        }

        doc.cubes.push(Cube {
            name: element
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("cube")
                .to_string(),
            bone: None,
            from,
            to,
            origin,
            rotation: angles,
            inflate,
            mirror: false,
            faces,
        });
    }
    if doc.cubes.is_empty() {
        return Err("模型中没有可转换的方块".to_string());
    }
    Ok(doc)
}

// ===== 写出 =====

fn bone_order(bones: &[Bone]) -> Vec<usize> {
    let mut order = Vec::new();
    let mut visited = vec![false; bones.len()];
    for start in 0..bones.len() {
        let mut chain = Vec::new();
        let mut current = Some(start);
        while let Some(index) = current {
            if index >= bones.len() || visited[index] {
                break;
            }
            chain.push(index);
            current = bones[index].parent;
        }
        while let Some(index) = chain.pop() {
            if !visited[index] {
                visited[index] = true;
                order.push(index);
            }
        }
    }
    order
}

fn texture_file_name(stem: &str, texture: &Texture, index: usize) -> String {
    let ext = texture_ext(&texture.name);
    if index == 0 {
        format!("{stem}.{ext}")
    } else {
        format!("{stem}_{}.{}", index + 1, ext)
    }
}

fn push_texture_files(files: &mut Vec<(String, Vec<u8>)>, stem: &str, doc: &Doc) {
    for (index, texture) in doc.textures.iter().enumerate() {
        if texture.data.is_empty() {
            continue;
        }
        files.push((texture_file_name(stem, texture, index), texture.data.clone()));
    }
}

fn write_bbmodel_like(doc: &Doc, name: &str, model_format: &str) -> Converted {
    let stem = crate::store::sanitize_file_name(name);
    let is_generic = model_format == "free";
    let mut warnings = doc.warnings.clone();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    let mut elements: Vec<Value> = Vec::new();
    let mut element_uuid: Vec<String> = Vec::new();
    for (index, cube) in doc.cubes.iter().enumerate() {
        let uuid = make_uuid(0x1000 + index);
        element_uuid.push(uuid.clone());
        let mut faces = Map::new();
        let mut hidden = false;
        for (face_index, key) in FACE_KEYS.iter().enumerate() {
            match cube.faces[face_index] {
                Some(face) => {
                    faces.insert(
                        (*key).to_string(),
                        json!({
                            "uv": r4(face.rect),
                            "rotation": fnum(face.rotation),
                            "texture": face.texture
                        }),
                    );
                }
                None => hidden = true,
            }
        }
        if hidden {
            warnings.push(format!(
                "方块「{}」存在隐藏面，其它格式中可能显示为可见",
                cube.name
            ));
        }
        elements.push(json!({
            "name": cube.name,
            "type": "cube",
            "from": v3(cube.from),
            "to": v3(cube.to),
            "origin": v3(cube.origin),
            "rotation": v3(cube.rotation),
            "inflate": fnum(cube.inflate),
            "uv_offset": [0, 0],
            "box_uv": false,
            "mirror_uv": cube.mirror,
            "faces": Value::Object(faces),
            "uuid": uuid,
            "locked": false,
            "export": true,
            "visibility": true,
            "render_order": "default",
            "allow_mirror_modeling": true
        }));
    }

    let bone_uuid: Vec<String> = (0..doc.bones.len())
        .map(|index| make_uuid(0x2000 + index))
        .collect();
    let mut child_nodes: Vec<Vec<Value>> = vec![Vec::new(); doc.bones.len()];
    let mut root_nodes: Vec<Value> = Vec::new();
    for (index, cube) in doc.cubes.iter().enumerate() {
        match cube.bone {
            Some(bone) if bone < doc.bones.len() => {
                child_nodes[bone].push(json!(element_uuid[index]));
            }
            _ => root_nodes.push(json!(element_uuid[index])),
        }
    }
    let mut group_nodes: Vec<Value> = vec![Value::Null; doc.bones.len()];
    for index in (0..doc.bones.len()).rev() {
        let children = std::mem::take(&mut child_nodes[index]);
        let node = json!({
            "uuid": bone_uuid[index],
            "isOpen": false,
            "children": Value::Array(children)
        });
        group_nodes[index] = node.clone();
        match doc.bones[index].parent {
            Some(parent) if parent < doc.bones.len() => child_nodes[parent].push(node),
            _ => root_nodes.push(node),
        }
    }

    let mut groups: Vec<Value> = Vec::new();
    for (index, bone) in doc.bones.iter().enumerate() {
        groups.push(json!({
            "uuid": bone_uuid[index],
            "name": bone.name,
            "origin": v3(bone.pivot),
            "rotation": v3(bone.rotation),
            "isOpen": false,
            "locked": false,
            "visibility": true
        }));
    }

    let mut textures: Vec<Value> = Vec::new();
    for (index, texture) in doc.textures.iter().enumerate() {
        let mut item = Map::new();
        item.insert("path".to_string(), json!(texture.name));
        item.insert("name".to_string(), json!(texture.name));
        item.insert("folder".to_string(), json!(""));
        item.insert("namespace".to_string(), json!(""));
        item.insert("id".to_string(), json!(index.to_string()));
        item.insert("width".to_string(), json!(texture.width));
        item.insert("height".to_string(), json!(texture.height));
        item.insert("uv_width".to_string(), json!(texture.width));
        item.insert("uv_height".to_string(), json!(texture.height));
        item.insert("particle".to_string(), json!(index == 0));
        item.insert("render_mode".to_string(), json!("default"));
        item.insert("uuid".to_string(), json!(make_uuid(0x3000 + index)));
        if !texture.data.is_empty() {
            item.insert(
                "source".to_string(),
                json!(format!(
                    "data:{};base64,{}",
                    texture_mime(&texture.name),
                    STANDARD.encode(&texture.data)
                )),
            );
        }
        textures.push(Value::Object(item));
    }

    let mut animations: Vec<Value> = Vec::new();
    for (anim_index, animation) in doc.animations.iter().enumerate() {
        let mut per_bone: HashMap<usize, Vec<Value>> = HashMap::new();
        for channel in &animation.channels {
            if channel.bone >= doc.bones.len() {
                continue;
            }
            let entry = per_bone.entry(channel.bone).or_default();
            for (key_index, (time, values)) in channel.keys.iter().enumerate() {
                entry.push(json!({
                    "channel": channel.channel.key(),
                    "time": fnum(*time),
                    "data_points": [{
                        "x": fnum(values[0]),
                        "y": fnum(values[1]),
                        "z": fnum(values[2])
                    }],
                    "interpolation": "linear",
                    "color": -1,
                    "uuid": make_uuid(0x5000 + anim_index * 0x100 + key_index)
                }));
            }
        }
        let mut animators = Map::new();
        for (bone, keyframes) in per_bone {
            animators.insert(
                bone_uuid[bone].clone(),
                json!({
                    "name": doc.bones[bone].name,
                    "type": "bone",
                    "keyframes": keyframes
                }),
            );
        }
        if animators.is_empty() {
            continue;
        }
        animations.push(json!({
            "uuid": make_uuid(0x4000 + anim_index),
            "name": animation.name,
            "loop": if animation.looping { "loop" } else { "once" },
            "override": false,
            "length": fnum(animation.length),
            "animation_length": fnum(animation.length),
            "snapping": 0,
            "selected": false,
            "saved": true,
            "path": "",
            "animators": Value::Object(animators)
        }));
    }

    let payload = json!({
        "meta": {
            "format_version": "5.0",
            "model_format": model_format,
            "box_uv": false
        },
        "name": name,
        "model_identifier": "",
        "visible_box": [1, 1, 0],
        "variable_placeholders": "",
        "variable_placeholder_buttons": [],
        "resolution": {
            "width": fnum(doc.res[0]),
            "height": fnum(doc.res[1])
        },
        "elements": elements,
        "outliner": root_nodes,
        "groups": groups,
        "textures": textures,
        "animations": animations
    });
    let text = serde_json::to_vec(&payload).unwrap_or_default();
    let file_name = if is_generic {
        format!("{stem}.json")
    } else {
        format!("{stem}.bbmodel")
    };
    files.push((file_name, text));
    push_texture_files(&mut files, &stem, doc);

    Converted { files, warnings }
}

fn bedrock_cube_value(cube: &Cube) -> Value {
    let origin = [-cube.to[0], cube.from[1], cube.from[2]];
    let size = [
        cube.to[0] - cube.from[0],
        cube.to[1] - cube.from[1],
        cube.to[2] - cube.from[2],
    ];
    let mut uv = Map::new();
    for (index, key) in FACE_KEYS.iter().enumerate() {
        let Some(face) = cube.faces[index] else {
            continue;
        };
        let rect = if *key == "up" || *key == "down" {
            [face.rect[2], face.rect[3], face.rect[0], face.rect[1]]
        } else {
            face.rect
        };
        uv.insert(
            (*key).to_string(),
            json!({
                "uv": [fnum(rect[0]), fnum(rect[1])],
                "uv_size": [fnum(rect[2] - rect[0]), fnum(rect[3] - rect[1])]
            }),
        );
    }
    json!({
        "name": cube.name,
        "origin": v3(origin),
        "size": v3(size),
        "pivot": v3(mirror_pos(cube.origin)),
        "rotation": v3(mirror_rot(cube.rotation)),
        "inflate": fnum(cube.inflate),
        "mirror": cube.mirror,
        "uv": Value::Object(uv)
    })
}

fn identifier(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || *char == '_' || *char == '.')
        .collect();
    if cleaned.is_empty() {
        "geometry.model".to_string()
    } else {
        format!("geometry.{cleaned}")
    }
}

fn write_bedrock(doc: &Doc, name: &str) -> Converted {
    let stem = crate::store::sanitize_file_name(name);
    let mut warnings = doc.warnings.clone();
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();

    let mut per_bone: Vec<Vec<Value>> = vec![Vec::new(); doc.bones.len()];
    let mut loose: Vec<Value> = Vec::new();
    for cube in &doc.cubes {
        let value = bedrock_cube_value(cube);
        match cube.bone {
            Some(bone) if bone < per_bone.len() => per_bone[bone].push(value),
            _ => loose.push(value),
        }
    }

    let mut bones: Vec<Value> = Vec::new();
    if !loose.is_empty() {
        bones.push(json!({
            "name": "root",
            "pivot": [0, 0, 0],
            "cubes": loose
        }));
    }
    for index in bone_order(&doc.bones) {
        let bone = &doc.bones[index];
        let mut item = Map::new();
        item.insert("name".to_string(), json!(bone.name));
        if let Some(parent) = bone.parent {
            if parent < doc.bones.len() {
                item.insert("parent".to_string(), json!(doc.bones[parent].name));
            }
        }
        item.insert("pivot".to_string(), v3(mirror_pos(bone.pivot)));
        if bone.rotation.iter().any(|value| value.abs() > 1e-6) {
            item.insert("rotation".to_string(), v3(mirror_rot(bone.rotation)));
        }
        let cubes = std::mem::take(&mut per_bone[index]);
        if !cubes.is_empty() {
            item.insert("cubes".to_string(), Value::Array(cubes));
        }
        bones.push(Value::Object(item));
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for cube in &doc.cubes {
        for axis in 0..3 {
            min[axis] = min[axis].min(cube.from[axis]);
            max[axis] = max[axis].max(cube.to[axis]);
        }
    }
    let (bounds_width, bounds_height, bounds_offset) = if min[0] <= max[0] {
        (
            ((max[0] - min[0]).max(max[2] - min[2]) / 2.0).max(0.5),
            ((max[1] - min[1]) / 2.0).max(0.5),
            ((min[1] + max[1]) / 2.0).max(0.0),
        )
    } else {
        (4.0, 4.0, 2.0)
    };

    let geometry = json!({
        "format_version": "1.12.0",
        "minecraft:geometry": [{
            "description": {
                "identifier": identifier(name),
                "texture_width": fnum(doc.res[0]),
                "texture_height": fnum(doc.res[1]),
                "visible_bounds_width": fnum(bounds_width),
                "visible_bounds_height": fnum(bounds_height),
                "visible_bounds_offset": [0, fnum(bounds_offset), 0]
            },
            "bones": bones
        }]
    });
    files.push((
        format!("{stem}.geo.json"),
        serde_json::to_vec(&geometry).unwrap_or_default(),
    ));

    if !doc.animations.is_empty() {
        let mut animations = Map::new();
        for animation in &doc.animations {
            let mut bones_map = Map::new();
            for channel in &animation.channels {
                if channel.bone >= doc.bones.len() {
                    continue;
                }
                let entry = bones_map
                    .entry(doc.bones[channel.bone].name.clone())
                    .or_insert_with(|| json!({}));
                let mut times = Map::new();
                for (time, values) in &channel.keys {
                    let value = match channel.channel {
                        Channel::Rotation => mirror_rot(*values),
                        Channel::Position => mirror_pos(*values),
                        Channel::Scale => *values,
                    };
                    times.insert(fmt_time(*time), v3(value));
                }
                if let Value::Object(object) = entry {
                    object.insert(channel.channel.key().to_string(), Value::Object(times));
                }
            }
            if bones_map.is_empty() {
                continue;
            }
            animations.insert(
                animation.name.clone(),
                json!({
                    "loop": animation.looping,
                    "animation_length": fnum(animation.length),
                    "bones": Value::Object(bones_map)
                }),
            );
        }
        if !animations.is_empty() {
            let animation_doc = json!({
                "format_version": "1.8.0",
                "animations": Value::Object(animations)
            });
            files.push((
                format!("{stem}.animation.json"),
                serde_json::to_vec(&animation_doc).unwrap_or_default(),
            ));
        }
    }

    push_texture_files(&mut files, &stem, doc);
    if doc.textures.is_empty() {
        warnings.push("模型没有贴图，基岩版导出后需要自行指定贴图".to_string());
    }

    Converted { files, warnings }
}

fn write_java(doc: &Doc, name: &str) -> Converted {
    let stem = crate::store::sanitize_file_name(name);
    let mut warnings = doc.warnings.clone();
    if !doc.animations.is_empty() {
        warnings.push("Java 版方块模型不支持动画，动画未导出".to_string());
    }
    if doc.bones.iter().any(|bone| {
        bone.rotation.iter().any(|value| value.abs() > 1e-6)
    }) {
        warnings.push("Java 版方块模型不支持骨骼旋转，相关旋转未保留".to_string());
    }
    if doc.bones.len() > 1 || doc.cubes.iter().any(|cube| cube.bone.is_some()) {
        warnings.push("Java 版方块模型没有骨骼层级，所有方块会被摊平".to_string());
    }

    let res_x = doc.res[0].max(1.0);
    let res_y = doc.res[1].max(1.0);
    let scale_x = 16.0 / res_x;
    let scale_y = 16.0 / res_y;

    let mut elements: Vec<Value> = Vec::new();
    let mut multi_axis_warned = false;
    for cube in &doc.cubes {
        let mut faces = Map::new();
        for (index, key) in FACE_KEYS.iter().enumerate() {
            let Some(face) = cube.faces[index] else {
                continue;
            };
            faces.insert(
                (*key).to_string(),
                json!({
                    "uv": [
                        fnum(face.rect[0] * scale_x),
                        fnum(face.rect[1] * scale_y),
                        fnum(face.rect[2] * scale_x),
                        fnum(face.rect[3] * scale_y)
                    ],
                    "rotation": fnum(face.rotation),
                    "texture": "#0"
                }),
            );
        }
        if faces.is_empty() {
            continue;
        }

        let mut element = Map::new();
        element.insert(
            "from".to_string(),
            v3([cube.from[0] + 8.0, cube.from[1], cube.from[2] + 8.0]),
        );
        element.insert(
            "to".to_string(),
            v3([cube.to[0] + 8.0, cube.to[1], cube.to[2] + 8.0]),
        );
        if cube.inflate.abs() > 1e-6 {
            element.insert("inflate".to_string(), fnum(cube.inflate));
        }
        let normalized: Vec<(usize, f32)> = cube
            .rotation
            .iter()
            .enumerate()
            .filter(|(_, value)| value.abs() > 1e-6)
            .map(|(index, value)| (index, *value))
            .collect();
        if normalized.len() > 1 && !multi_axis_warned {
            warnings.push(format!(
                "Java 版方块模型仅支持单轴旋转，方块「{}」只保留了最大角度轴",
                cube.name
            ));
            multi_axis_warned = true;
        }
        if let Some((axis, angle)) = normalized
            .iter()
            .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap_or(std::cmp::Ordering::Equal))
        {
            let axis_name = ["x", "y", "z"][*axis];
            element.insert(
                "rotation".to_string(),
                json!({
                    "angle": fnum(*angle),
                    "axis": axis_name,
                    "origin": v3([cube.origin[0] + 8.0, cube.origin[1], cube.origin[2] + 8.0])
                }),
            );
        }
        element.insert("faces".to_string(), Value::Object(faces));
        elements.push(Value::Object(element));
    }

    if elements.is_empty() {
        warnings.push("没有可导出的方块".to_string());
    }
    let mut textures = Map::new();
    textures.insert("0".to_string(), json!(stem));
    textures.insert("particle".to_string(), json!(stem));

    let payload = json!({
        "credit": "Exported by tsrbox",
        "texture_size": [fnum(res_x), fnum(res_y)],
        "textures": Value::Object(textures),
        "elements": elements
    });
    let mut files = vec![(
        format!("{stem}.json"),
        serde_json::to_vec(&payload).unwrap_or_default(),
    )];
    push_texture_files(&mut files, &stem, doc);
    Converted { files, warnings }
}

/// 把模型目录中的主文件 + 贴图 + 动作文件转换为目标格式
pub fn convert_model(
    dir: &Path,
    main_file: &str,
    texture_files: &[String],
    animation_files: &[String],
    target: Format,
    name: &str,
) -> Result<Converted, String> {
    if target.is_original() {
        return Err("原始格式不需要转换".to_string());
    }
    if main_file.is_empty() {
        return Err("模型主文件缺失".to_string());
    }
    let main_path = dir.join(main_file);
    let json = read_json(&main_path)?;
    let ext = main_path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let kind = detect_kind(&ext, &json)?;
    let texture_paths: Vec<PathBuf> = texture_files
        .iter()
        .map(|item| dir.join(item))
        .filter(|path| path.is_file())
        .collect();
    let animation_paths: Vec<PathBuf> = animation_files
        .iter()
        .map(|item| dir.join(item))
        .filter(|path| path.is_file())
        .collect();

    let doc = match kind {
        Kind::Bbmodel | Kind::Generic => parse_bbmodel(&json, &texture_paths)?,
        Kind::Bedrock => parse_bedrock(&json, &texture_paths, &animation_paths)?,
        Kind::Java => parse_java(&json, &texture_paths)?,
    };

    Ok(match target {
        Format::Bbmodel => write_bbmodel_like(&doc, name, "bedrock"),
        Format::Generic => write_bbmodel_like(&doc, name, "free"),
        Format::Bedrock => write_bedrock(&doc, name),
        Format::Java => write_java(&doc, name),
        Format::Original => unreachable!(),
    })
}

/// 模型自身的格式（用于判断目标格式是否与源格式一致，直接复制文件）
pub fn model_format(dir: &Path, main_file: &str) -> Option<Format> {
    let path = dir.join(main_file);
    let json = read_json(&path).ok()?;
    let ext = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match detect_kind(&ext, &json).ok()? {
        Kind::Bbmodel => Some(Format::Bbmodel),
        Kind::Bedrock => Some(Format::Bedrock),
        Kind::Java => Some(Format::Java),
        Kind::Generic => Some(Format::Generic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_png() -> Vec<u8> {
        // 1x1 透明 PNG
        let data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        STANDARD.decode(data).unwrap()
    }

    fn sample_bbmodel() -> Value {
        json!({
            "meta": {"format_version": "5.0", "model_format": "bedrock", "box_uv": false},
            "name": "demo",
            "resolution": {"width": 64, "height": 64},
            "elements": [{
                "name": "body",
                "uuid": "e1",
                "type": "cube",
                "from": [-4, 8, -2],
                "to": [4, 16, 2],
                "origin": [0, 12, 0],
                "rotation": [0, 0, 0],
                "inflate": 0,
                "uv_offset": [0, 0],
                "box_uv": true,
                "faces": {
                    "north": {"uv": [4, 4, 12, 12], "texture": 0},
                    "south": {"uv": [16, 4, 24, 12], "texture": 0},
                    "east": {"uv": [0, 4, 4, 12], "texture": 0},
                    "west": {"uv": [12, 4, 16, 12], "texture": 0},
                    "up": {"uv": [12, 4, 4, 0], "texture": 0},
                    "down": {"uv": [20, 0, 12, 4], "texture": 0}
                }
            }],
            "outliner": [{
                "name": "bone",
                "origin": [0, 12, 0],
                "rotation": [0, 0, 0],
                "uuid": "g1",
                "children": ["e1"]
            }],
            "textures": [{
                "name": "demo.png",
                "id": "0",
                "width": 64,
                "height": 64,
                "source": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=="
            }],
            "animations": [{
                "name": "idle",
                "loop": "loop",
                "length": 1.0,
                "animators": {
                    "g1": {
                        "name": "bone",
                        "type": "bone",
                        "keyframes": [
                            {"channel": "rotation", "time": 0, "data_points": [{"x": 0, "y": 0, "z": 0}]},
                            {"channel": "rotation", "time": 1, "data_points": [{"x": 15, "y": -10, "z": 5}]},
                            {"channel": "position", "time": 0, "data_points": [{"x": 1, "y": 2, "z": 3}]},
                            {"channel": "position", "time": 1, "data_points": [{"x": -1, "y": 0, "z": 0}]}
                        ]
                    }
                }
            }]
        })
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tsrbox-convert-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn bbmodel_to_bedrock_and_back() {
        let dir = temp_dir("roundtrip");
        let source = sample_bbmodel();
        fs::write(dir.join("demo.bbmodel"), serde_json::to_vec(&source).unwrap()).unwrap();
        let texture = dir.join("demo.png");
        fs::write(&texture, sample_png()).unwrap();

        let converted = convert_model(
            &dir,
            "demo.bbmodel",
            &[],
            &[],
            Format::Bedrock,
            "demo",
        )
        .unwrap();
        assert!(converted
            .files
            .iter()
            .any(|(name, _)| name == "demo.geo.json"));
        assert!(converted
            .files
            .iter()
            .any(|(name, _)| name == "demo.animation.json"));

        let out_dir = temp_dir("roundtrip-back");
        for (name, bytes) in &converted.files {
            fs::write(out_dir.join(name), bytes).unwrap();
        }
        let back = convert_model(
            &out_dir,
            "demo.geo.json",
            &["demo.png".to_string()],
            &["demo.animation.json".to_string()],
            Format::Bbmodel,
            "demo",
        )
        .unwrap();
        let (name, bytes) = back
            .files
            .iter()
            .find(|(name, _)| name.ends_with(".bbmodel"))
            .unwrap();
        assert_eq!(name, "demo.bbmodel");
        let json: Value = serde_json::from_slice(bytes).unwrap();
        let element = &json["elements"][0];
        // 基岩版 X 轴镜像回来，几何应与原模型一致
        assert!((element["from"][0].as_f64().unwrap() + 4.0).abs() < 1e-3);
        assert!((element["to"][0].as_f64().unwrap() - 4.0).abs() < 1e-3);
        assert!((element["from"][1].as_f64().unwrap() - 8.0).abs() < 1e-3);
        assert_eq!(json["animations"][0]["name"], "idle");
        let keyframes = json["animations"][0]["animators"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["keyframes"]
            .as_array()
            .unwrap()
            .clone();
        let rotation = keyframes
            .iter()
            .find(|item| {
                item["channel"] == "rotation" && item["time"].as_f64() == Some(1.0)
            })
            .unwrap();
        assert_eq!(rotation["data_points"][0]["x"].as_f64().unwrap(), 15.0);
        assert_eq!(rotation["data_points"][0]["y"].as_f64().unwrap(), -10.0);
    }

    #[test]
    fn bbmodel_to_java_geometry() {
        let dir = temp_dir("java");
        fs::write(
            dir.join("demo.bbmodel"),
            serde_json::to_vec(&sample_bbmodel()).unwrap(),
        )
        .unwrap();
        let converted = convert_model(&dir, "demo.bbmodel", &[], &[], Format::Java, "demo").unwrap();
        let (_, bytes) = converted
            .files
            .iter()
            .find(|(name, _)| name == "demo.json")
            .unwrap();
        let json: Value = serde_json::from_slice(bytes).unwrap();
        let element = &json["elements"][0];
        assert!((element["from"][0].as_f64().unwrap() - 4.0).abs() < 1e-3);
        assert!((element["to"][0].as_f64().unwrap() - 12.0).abs() < 1e-3);
        assert!((element["from"][1].as_f64().unwrap() - 8.0).abs() < 1e-3);
        // UV 由 64 像素换算到 0-16
        let north = &element["faces"]["north"]["uv"];
        assert!((north[0].as_f64().unwrap() - 1.0).abs() < 1e-3);
        assert!((north[2].as_f64().unwrap() - 3.0).abs() < 1e-3);
        assert_eq!(json["textures"]["0"], "demo");
    }

    #[test]
    fn bedrock_animation_detected_from_file() {
        let dir = temp_dir("bedrock-anim");
        let geometry = json!({
            "format_version": "1.12.0",
            "minecraft:geometry": [{
                "description": {"identifier": "geometry.demo", "texture_width": 64, "texture_height": 64},
                "bones": [{
                    "name": "bone",
                    "pivot": [0, 0, 0],
                    "cubes": [{"origin": [-4, 8, -2], "size": [8, 8, 4], "uv": [0, 0]}]
                }]
            }]
        });
        fs::write(dir.join("demo.geo.json"), serde_json::to_vec(&geometry).unwrap()).unwrap();
        fs::write(
            dir.join("demo.animation.json"),
            serde_json::to_vec(&json!({
                "format_version": "1.8.0",
                "animations": {
                    "idle": {
                        "loop": true,
                        "animation_length": 2.0,
                        "bones": {"bone": {"rotation": {"0.0": [-30, 0, 0], "2.0": [0, 0, 0]}}}
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let converted = convert_model(
            &dir,
            "demo.geo.json",
            &[],
            &["demo.animation.json".to_string()],
            Format::Bbmodel,
            "demo",
        )
        .unwrap();
        let (_, bytes) = converted
            .files
            .iter()
            .find(|(name, _)| name.ends_with(".bbmodel"))
            .unwrap();
        let json: Value = serde_json::from_slice(bytes).unwrap();
        let keyframes = json["animations"][0]["animators"]
            .as_object()
            .unwrap()
            .values()
            .next()
            .unwrap()["keyframes"]
            .as_array()
            .unwrap()
            .clone();
        // 基岩版 -30° 换算到 Blockbench 为 +30°
        let first = keyframes
            .iter()
            .find(|item| item["time"].as_f64() == Some(0.0))
            .unwrap();
        assert_eq!(first["data_points"][0]["x"].as_f64().unwrap(), 30.0);
    }

}
