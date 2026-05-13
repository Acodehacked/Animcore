/// Chunk-based binary .anim format — version 2
///
/// Layout:
///   magic[4]    "ANIM"
///   version     u16 LE  (1 = legacy, 2 = current)
///   _reserved   u16
///   chunks…
///     chunk_type  u16 LE
///     data_len    u32 LE
///     data        [u8; data_len]  (postcard-encoded payload)
///   ENND chunk (type=0x454E, len=0) terminates the file.
///
/// V2 chunks (in order):
///   STBL — deduplicated string table  Vec<String>
///   ARTB — compact document (string fields replaced by STBL indices)
///   PATH — delta-encoded path control points  Vec<PathRecord>
///   ASST — embedded asset blobs  Vec<AssetBlob>  (omitted when empty)
///   ENND
///
/// V1 compatibility: a single ARTB chunk containing the full postcard Document.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::constraints::Constraint;
use crate::effects::Effect;
use crate::paint::Color;
use crate::path::{AnimPath, PathVerb};
use crate::schema::{Animation, Artboard, Document, Geometry, Node, ShapeData, Track};
use crate::transform::Transform;
use crate::schema::LoopMode;

// ── Constants ─────────────────────────────────────────────────────────────────

const MAGIC: &[u8; 4] = b"ANIM";
const VERSION: u16 = 2;

const CHUNK_STBL: u16 = 0x5354; // "ST"
const CHUNK_ARTB: u16 = 0x4152; // "AR"
const CHUNK_PATH: u16 = 0x5041; // "PA"
const CHUNK_ASST: u16 = 0x4153; // "AS"
const CHUNK_ENND: u16 = 0x454E; // "EN"

// ── Public types ──────────────────────────────────────────────────────────────

/// An embedded asset (image, font, etc.) stored in the `.anim` file.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AssetBlob {
    pub name: String,
    pub mime: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum AnimFormatError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u16),
    TruncatedChunk,
    MissingData,
    Codec(postcard::Error),
}

impl std::fmt::Display for AnimFormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnimFormatError::TooShort => write!(f, "data too short"),
            AnimFormatError::BadMagic => write!(f, "not an ANIM file"),
            AnimFormatError::UnsupportedVersion(v) => write!(f, "unsupported version {v}"),
            AnimFormatError::TruncatedChunk => write!(f, "truncated chunk"),
            AnimFormatError::MissingData => write!(f, "required chunk missing"),
            AnimFormatError::Codec(e) => write!(f, "codec error: {e}"),
        }
    }
}

impl From<postcard::Error> for AnimFormatError {
    fn from(e: postcard::Error) -> Self {
        AnimFormatError::Codec(e)
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Serialize a `Document` to the current (v2) binary `.anim` format.
pub fn write_anim(doc: &Document) -> Vec<u8> {
    write_anim_with_assets(doc, &[])
}

/// Serialize a `Document` plus embedded asset blobs to the v2 format.
pub fn write_anim_with_assets(doc: &Document, assets: &[AssetBlob]) -> Vec<u8> {
    let mut stbl = StringTable::new();
    let mut path_records: Vec<PathRecord> = Vec::new();
    let compact = compact_document(doc, &mut stbl, &mut path_records);

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    append_chunk(&mut out, CHUNK_STBL, &postcard::to_allocvec(&stbl.strings).expect("stbl"));
    append_chunk(&mut out, CHUNK_ARTB, &postcard::to_allocvec(&compact).expect("artb"));
    if !path_records.is_empty() {
        append_chunk(&mut out, CHUNK_PATH, &postcard::to_allocvec(&path_records).expect("path"));
    }
    if !assets.is_empty() {
        append_chunk(&mut out, CHUNK_ASST, &postcard::to_allocvec(&assets).expect("asst"));
    }
    append_chunk(&mut out, CHUNK_ENND, &[]);
    out
}

/// Deserialize a `Document` from raw `.anim` bytes (v1 or v2).
pub fn read_anim(data: &[u8]) -> Result<Document, AnimFormatError> {
    let (doc, _) = read_anim_full(data)?;
    Ok(doc)
}

/// Deserialize a `Document` and any embedded assets from raw `.anim` bytes.
pub fn read_anim_full(data: &[u8]) -> Result<(Document, Vec<AssetBlob>), AnimFormatError> {
    if data.len() < 8 {
        return Err(AnimFormatError::TooShort);
    }
    if &data[0..4] != MAGIC {
        return Err(AnimFormatError::BadMagic);
    }
    let version = u16::from_le_bytes([data[4], data[5]]);
    match version {
        1 => Ok((read_v1(data)?, vec![])),
        2 => read_v2(data),
        v => Err(AnimFormatError::UnsupportedVersion(v)),
    }
}

/// Re-encode a v1 `.anim` file to the current v2 format.
pub fn migrate_v1(data: &[u8]) -> Result<Vec<u8>, AnimFormatError> {
    let (doc, assets) = read_anim_full(data)?;
    Ok(write_anim_with_assets(&doc, &assets))
}

// ── String table ──────────────────────────────────────────────────────────────

struct StringTable {
    strings: Vec<String>,
    map: HashMap<String, u32>,
}

impl StringTable {
    fn new() -> Self {
        Self { strings: Vec::new(), map: HashMap::new() }
    }

    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&idx) = self.map.get(s) {
            return idx;
        }
        let idx = self.strings.len() as u32;
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), idx);
        idx
    }
}

// ── Compact document types (V2 ARTB payload) ──────────────────────────────────

#[derive(Serialize, Deserialize)]
struct CompactDocument {
    version: u32,
    artboards: Vec<CompactArtboard>,
}

#[derive(Serialize, Deserialize)]
struct CompactArtboard {
    id: Uuid,
    name: u32,
    width: f32,
    height: f32,
    background: Color,
    nodes: Vec<CompactNode>,
    animations: Vec<CompactAnimation>,
    constraints: Vec<Constraint>,
}

#[derive(Serialize, Deserialize)]
struct CompactNode {
    id: Uuid,
    name: u32,
    transform: Transform,
    parent_id: Option<Uuid>,
    opacity: f32,
    visible: bool,
    /// `Geometry::Path` nodes have their path data moved to the PATH chunk;
    /// this field stores an empty `AnimPath` placeholder.
    shape: Option<ShapeData>,
    clip_children: bool,
    effects: Vec<Effect>,
}

#[derive(Serialize, Deserialize)]
struct CompactAnimation {
    id: Uuid,
    name: u32,
    duration_secs: f32,
    fps: u32,
    loop_mode: LoopMode,
    tracks: Vec<Track>,
}

// ── PATH chunk ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct PathRecord {
    node_id: Uuid,
    verbs: Vec<PathVerb>,
    /// X coordinates, delta-encoded from 0.
    dx: Vec<f32>,
    /// Y coordinates, delta-encoded from 0.
    dy: Vec<f32>,
}

fn delta_encode(points: &[[f32; 2]]) -> (Vec<f32>, Vec<f32>) {
    let mut dx = Vec::with_capacity(points.len());
    let mut dy = Vec::with_capacity(points.len());
    let (mut px, mut py) = (0.0f32, 0.0f32);
    for &[x, y] in points {
        dx.push(x - px);
        dy.push(y - py);
        px = x;
        py = y;
    }
    (dx, dy)
}

fn delta_decode(dx: &[f32], dy: &[f32]) -> Vec<[f32; 2]> {
    let mut pts = Vec::with_capacity(dx.len());
    let (mut px, mut py) = (0.0f32, 0.0f32);
    for (&x, &y) in dx.iter().zip(dy.iter()) {
        px += x;
        py += y;
        pts.push([px, py]);
    }
    pts
}

// ── Document → compact ────────────────────────────────────────────────────────

fn compact_document(doc: &Document, stbl: &mut StringTable, paths: &mut Vec<PathRecord>) -> CompactDocument {
    CompactDocument {
        version: doc.version,
        artboards: doc.artboards.iter()
            .map(|ab| compact_artboard(ab, stbl, paths))
            .collect(),
    }
}

fn compact_artboard(ab: &Artboard, stbl: &mut StringTable, paths: &mut Vec<PathRecord>) -> CompactArtboard {
    CompactArtboard {
        id: ab.id,
        name: stbl.intern(&ab.name),
        width: ab.width,
        height: ab.height,
        background: ab.background,
        nodes: ab.nodes.iter().map(|n| compact_node(n, stbl, paths)).collect(),
        animations: ab.animations.iter().map(|a| compact_animation(a, stbl)).collect(),
        constraints: ab.constraints.clone(),
    }
}

fn compact_node(n: &Node, stbl: &mut StringTable, paths: &mut Vec<PathRecord>) -> CompactNode {
    let shape = n.shape.as_ref().map(|sd| {
        if let Geometry::Path(path) = &sd.geometry {
            if !path.is_empty() {
                let (dx, dy) = delta_encode(&path.points);
                paths.push(PathRecord { node_id: n.id, verbs: path.verbs.clone(), dx, dy });
            }
            ShapeData { geometry: Geometry::Path(AnimPath::default()), paint: sd.paint.clone() }
        } else {
            sd.clone()
        }
    });
    CompactNode {
        id: n.id,
        name: stbl.intern(&n.name),
        transform: n.transform.clone(),
        parent_id: n.parent_id,
        opacity: n.opacity,
        visible: n.visible,
        shape,
        clip_children: n.clip_children,
        effects: n.effects.clone(),
    }
}

fn compact_animation(a: &Animation, stbl: &mut StringTable) -> CompactAnimation {
    CompactAnimation {
        id: a.id,
        name: stbl.intern(&a.name),
        duration_secs: a.duration_secs,
        fps: a.fps,
        loop_mode: a.loop_mode,
        tracks: a.tracks.clone(),
    }
}

// ── Compact → Document ────────────────────────────────────────────────────────

fn expand_document(c: CompactDocument, stbl: &[String], paths: &[PathRecord]) -> Document {
    let path_map: HashMap<Uuid, AnimPath> = paths.iter().map(|pr| {
        let pts = delta_decode(&pr.dx, &pr.dy);
        (pr.node_id, AnimPath { verbs: pr.verbs.clone(), points: pts })
    }).collect();
    Document {
        version: c.version,
        artboards: c.artboards.into_iter()
            .map(|ab| expand_artboard(ab, stbl, &path_map))
            .collect(),
    }
}

fn expand_artboard(c: CompactArtboard, stbl: &[String], paths: &HashMap<Uuid, AnimPath>) -> Artboard {
    Artboard {
        id: c.id,
        name: stbl.get(c.name as usize).cloned().unwrap_or_default(),
        width: c.width,
        height: c.height,
        background: c.background,
        nodes: c.nodes.into_iter().map(|n| expand_node(n, stbl, paths)).collect(),
        animations: c.animations.into_iter().map(|a| expand_animation(a, stbl)).collect(),
        constraints: c.constraints,
    }
}

fn expand_node(c: CompactNode, stbl: &[String], paths: &HashMap<Uuid, AnimPath>) -> Node {
    let shape = c.shape.map(|mut sd| {
        if matches!(sd.geometry, Geometry::Path(_)) {
            if let Some(path) = paths.get(&c.id) {
                sd.geometry = Geometry::Path(path.clone());
            }
        }
        sd
    });
    Node {
        id: c.id,
        name: stbl.get(c.name as usize).cloned().unwrap_or_default(),
        transform: c.transform,
        parent_id: c.parent_id,
        opacity: c.opacity,
        visible: c.visible,
        shape,
        clip_children: c.clip_children,
        effects: c.effects,
    }
}

fn expand_animation(c: CompactAnimation, stbl: &[String]) -> Animation {
    Animation {
        id: c.id,
        name: stbl.get(c.name as usize).cloned().unwrap_or_default(),
        duration_secs: c.duration_secs,
        fps: c.fps,
        loop_mode: c.loop_mode,
        tracks: c.tracks,
    }
}

// ── Chunk parsing ─────────────────────────────────────────────────────────────

fn parse_chunks(data: &[u8]) -> Result<Vec<(u16, &[u8])>, AnimFormatError> {
    let mut pos = 8usize; // skip 4-byte magic + 2-byte version + 2-byte reserved
    let mut chunks = Vec::new();
    while pos + 6 <= data.len() {
        let chunk_type = u16::from_le_bytes([data[pos], data[pos + 1]]);
        let chunk_len = u32::from_le_bytes([data[pos + 2], data[pos + 3], data[pos + 4], data[pos + 5]]) as usize;
        pos += 6;
        if pos + chunk_len > data.len() {
            return Err(AnimFormatError::TruncatedChunk);
        }
        let chunk_data = &data[pos..pos + chunk_len];
        pos += chunk_len;
        if chunk_type == CHUNK_ENND {
            break;
        }
        chunks.push((chunk_type, chunk_data));
    }
    Ok(chunks)
}

fn append_chunk(out: &mut Vec<u8>, chunk_type: u16, data: &[u8]) {
    out.extend_from_slice(&chunk_type.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
}

// ── V1 reader ─────────────────────────────────────────────────────────────────

fn read_v1(data: &[u8]) -> Result<Document, AnimFormatError> {
    let chunks = parse_chunks(data)?;
    for (typ, cdata) in chunks {
        if typ == CHUNK_ARTB {
            return Ok(postcard::from_bytes::<Document>(cdata)?);
        }
    }
    Err(AnimFormatError::MissingData)
}

// ── V2 reader ─────────────────────────────────────────────────────────────────

fn read_v2(data: &[u8]) -> Result<(Document, Vec<AssetBlob>), AnimFormatError> {
    let chunks = parse_chunks(data)?;
    let mut stbl: Vec<String> = Vec::new();
    let mut compact_opt: Option<CompactDocument> = None;
    let mut path_records: Vec<PathRecord> = Vec::new();
    let mut assets: Vec<AssetBlob> = Vec::new();

    for (typ, cdata) in chunks {
        match typ {
            CHUNK_STBL => { stbl = postcard::from_bytes(cdata)?; }
            CHUNK_ARTB => { compact_opt = Some(postcard::from_bytes(cdata)?); }
            CHUNK_PATH => { path_records = postcard::from_bytes(cdata)?; }
            CHUNK_ASST => { assets = postcard::from_bytes(cdata)?; }
            _ => {} // forward-compatible: ignore unknown chunk types
        }
    }

    let compact = compact_opt.ok_or(AnimFormatError::MissingData)?;
    Ok((expand_document(compact, &stbl, &path_records), assets))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paint::{Color, Paint};
    use crate::schema::Artboard;

    fn minimal_doc() -> Document {
        Document {
            version: 1,
            artboards: vec![Artboard {
                id: Uuid::new_v4(),
                name: "Test".into(),
                width: 100.0,
                height: 100.0,
                background: Color::WHITE,
                nodes: vec![],
                animations: vec![],
                constraints: vec![],
            }],
        }
    }

    fn doc_with_paths() -> Document {
        let mut node = Node::new("star");
        let mut path = AnimPath::new();
        path.move_to(50.0, 10.0)
            .line_to(61.0, 35.0)
            .line_to(90.0, 35.0)
            .line_to(68.0, 57.0)
            .line_to(79.0, 91.0)
            .line_to(50.0, 70.0)
            .line_to(21.0, 91.0)
            .line_to(32.0, 57.0)
            .line_to(10.0, 35.0)
            .line_to(39.0, 35.0)
            .close();
        node.shape = Some(ShapeData {
            geometry: Geometry::Path(path),
            paint: Paint::filled(Color { r: 1.0, g: 0.8, b: 0.0, a: 1.0 }),
        });
        Document {
            version: 1,
            artboards: vec![Artboard {
                id: Uuid::new_v4(),
                name: "Canvas".into(),
                width: 100.0,
                height: 100.0,
                background: Color::WHITE,
                nodes: vec![node],
                animations: vec![],
                constraints: vec![],
            }],
        }
    }

    fn doc_with_repeated_names() -> Document {
        let anims: Vec<crate::schema::Animation> = (0..5).map(|i| crate::schema::Animation {
            id: Uuid::new_v4(),
            name: if i % 2 == 0 { "idle".into() } else { "run".into() },
            duration_secs: 1.0,
            fps: 60,
            loop_mode: crate::schema::LoopMode::Loop,
            tracks: vec![],
        }).collect();
        Document {
            version: 1,
            artboards: vec![Artboard {
                id: Uuid::new_v4(),
                name: "Main".into(),
                width: 200.0,
                height: 200.0,
                background: Color::WHITE,
                nodes: vec![],
                animations: anims,
                constraints: vec![],
            }],
        }
    }

    #[test]
    fn roundtrip_minimal() {
        let doc = minimal_doc();
        let bytes = write_anim(&doc);
        let loaded = read_anim(&bytes).expect("read_anim failed");
        assert_eq!(loaded.artboards.len(), 1);
        assert_eq!(loaded.artboards[0].name, "Test");
    }

    #[test]
    fn roundtrip_with_path() {
        let doc = doc_with_paths();
        let bytes = write_anim(&doc);
        let loaded = read_anim(&bytes).expect("read_anim failed");
        let node = &loaded.artboards[0].nodes[0];
        assert_eq!(node.name, "star");
        if let Some(ShapeData { geometry: Geometry::Path(p), .. }) = &node.shape {
            assert_eq!(p.verbs.len(), 11); // 1 move + 9 lines + close
            assert!((p.points[0][0] - 50.0).abs() < 1e-4, "first point x");
            assert!((p.points[0][1] - 10.0).abs() < 1e-4, "first point y");
        } else {
            panic!("expected Geometry::Path");
        }
    }

    #[test]
    fn magic_bytes() {
        let bytes = write_anim(&minimal_doc());
        assert_eq!(&bytes[0..4], b"ANIM");
        assert_eq!(u16::from_le_bytes([bytes[4], bytes[5]]), 2);
    }

    #[test]
    fn bad_magic_rejected() {
        let mut bytes = write_anim(&minimal_doc());
        bytes[0] = b'X';
        assert!(matches!(read_anim(&bytes), Err(AnimFormatError::BadMagic)));
    }

    #[test]
    fn truncated_rejected() {
        assert!(matches!(read_anim(&[0u8; 4]), Err(AnimFormatError::TooShort)));
    }

    #[test]
    fn file_smaller_than_serde_json() {
        let doc = minimal_doc();
        let anim_bytes = write_anim(&doc);
        let json_bytes = serde_json::to_vec(&doc).unwrap();
        assert!(
            anim_bytes.len() < json_bytes.len(),
            ".anim ({}) should be smaller than JSON ({})",
            anim_bytes.len(), json_bytes.len()
        );
    }

    #[test]
    fn string_table_deduplicates() {
        let doc = doc_with_repeated_names();
        let bytes = write_anim(&doc);
        // Parse STBL chunk manually and verify only "Main", "idle", "run" are present
        let chunks = parse_chunks(&bytes).unwrap();
        let stbl_data = chunks.iter().find(|(t, _)| *t == CHUNK_STBL).map(|(_, d)| *d).unwrap();
        let stbl: Vec<String> = postcard::from_bytes(stbl_data).unwrap();
        // 5 animations using only 2 unique names → STBL has 3 entries: "Main", "idle", "run"
        assert_eq!(stbl.len(), 3, "expected 3 unique strings, got {stbl:?}");
        assert!(stbl.contains(&"idle".to_string()));
        assert!(stbl.contains(&"run".to_string()));
    }

    #[test]
    fn path_chunk_present_for_path_nodes() {
        let doc = doc_with_paths();
        let bytes = write_anim(&doc);
        let chunks = parse_chunks(&bytes).unwrap();
        assert!(chunks.iter().any(|(t, _)| *t == CHUNK_PATH), "PATH chunk missing");
    }

    #[test]
    fn delta_encoding_roundtrip() {
        let points: Vec<[f32; 2]> = vec![[10.0, 20.0], [15.0, 25.0], [30.0, 10.0]];
        let (dx, dy) = delta_encode(&points);
        assert!((dx[0] - 10.0).abs() < 1e-6);
        assert!((dx[1] - 5.0).abs() < 1e-6);
        assert!((dx[2] - 15.0).abs() < 1e-6);
        let recovered = delta_decode(&dx, &dy);
        for (a, b) in points.iter().zip(recovered.iter()) {
            assert!((a[0] - b[0]).abs() < 1e-5);
            assert!((a[1] - b[1]).abs() < 1e-5);
        }
    }

    #[test]
    fn asset_blob_roundtrip() {
        let doc = minimal_doc();
        let assets = vec![AssetBlob {
            name: "logo.png".into(),
            mime: "image/png".into(),
            data: vec![0x89, 0x50, 0x4E, 0x47],
        }];
        let bytes = write_anim_with_assets(&doc, &assets);
        let (_, loaded_assets) = read_anim_full(&bytes).unwrap();
        assert_eq!(loaded_assets.len(), 1);
        assert_eq!(loaded_assets[0].name, "logo.png");
        assert_eq!(loaded_assets[0].data, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn migrate_v1_produces_v2() {
        // Build a v1 file by hand (single ARTB chunk, version=1 in header)
        let doc = minimal_doc();
        let v1_bytes = {
            let payload = postcard::to_allocvec(&doc).unwrap();
            let mut out = Vec::new();
            out.extend_from_slice(b"ANIM");
            out.extend_from_slice(&1u16.to_le_bytes()); // version 1
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&CHUNK_ARTB.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&payload);
            out.extend_from_slice(&CHUNK_ENND.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out
        };
        assert_eq!(u16::from_le_bytes([v1_bytes[4], v1_bytes[5]]), 1);

        let v2_bytes = migrate_v1(&v1_bytes).unwrap();
        assert_eq!(u16::from_le_bytes([v2_bytes[4], v2_bytes[5]]), 2);

        let loaded = read_anim(&v2_bytes).unwrap();
        assert_eq!(loaded.artboards[0].name, "Test");
    }
}
