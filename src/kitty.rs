use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use flate2::{write::ZlibEncoder, Compression};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;

use crate::render::RenderedPage;

const ESCAPE_PREFIX: &str = "\x1b_G";
const ESCAPE_SUFFIX: &str = "\x1b\\";
const BASE64_CHUNK_SIZE: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KittyImageIds {
    pub image_id: u32,
    pub placement_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KittyTransport {
    Direct,
    TmuxPassthrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KittyProbeResult {
    Supported,
    Unsupported,
    Unknown,
}

impl KittyImageIds {
    pub const DEFAULT: Self = Self {
        image_id: 1,
        placement_id: 1,
    };
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn encode_transmit_and_display(rendered: &RenderedPage, ids: KittyImageIds) -> Vec<String> {
    let payload = encode_rgba_payload(&rendered.rgba);
    let chunks = payload
        .as_bytes()
        .chunks(BASE64_CHUNK_SIZE)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>();

    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let has_more = usize::from(index + 1 < chunks.len());
            if index == 0 {
                format!(
                    "{ESCAPE_PREFIX}a=T,q=2,i={},p={},f=32,s={},v={},o=z,C=1,c={},r={},z=-1,m={};{}{ESCAPE_SUFFIX}",
                    ids.image_id,
                    ids.placement_id,
                    rendered.bitmap_width,
                    rendered.bitmap_height,
                    rendered.placement_columns,
                    rendered.placement_rows,
                    has_more,
                    chunk,
                )
            } else {
                format!(
                    "{ESCAPE_PREFIX}m={},q=2;{}{ESCAPE_SUFFIX}",
                    has_more, chunk,
                )
            }
        })
        .collect()
}

pub fn encode_delete_image(ids: KittyImageIds) -> String {
    let _ = ids.placement_id;
    format!(
        "{ESCAPE_PREFIX}a=d,d=I,q=2,i={}{}",
        ids.image_id, ESCAPE_SUFFIX
    )
}

pub fn encode_probe_query(query_id: u32) -> String {
    format!("{ESCAPE_PREFIX}a=q,t=d,s=1,v=1,i={query_id};AAAA{ESCAPE_SUFFIX}")
}

pub fn parse_probe_response(response: &str, query_id: u32) -> KittyProbeResult {
    let expected_prefix = format!("\x1b_Gi={query_id};");
    if response.contains(&expected_prefix) {
        KittyProbeResult::Supported
    } else if response.contains("\x1b[") {
        KittyProbeResult::Unsupported
    } else {
        KittyProbeResult::Unknown
    }
}

pub fn wrap_command_for_transport(command: &str, transport: KittyTransport) -> String {
    match transport {
        KittyTransport::Direct => command.to_string(),
        KittyTransport::TmuxPassthrough => {
            let escaped = command.replace('\x1b', "\x1b\x1b");
            format!("\x1bPtmux;{escaped}\x1b\\")
        }
    }
}

pub fn encode_transmit_only(rendered: &RenderedPage, image_id: u32) -> Vec<String> {
    let payload = encode_rgba_payload(&rendered.rgba);
    let chunks = payload
        .as_bytes()
        .chunks(BASE64_CHUNK_SIZE)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or_default())
        .collect::<Vec<_>>();

    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| {
            let has_more = usize::from(index + 1 < chunks.len());
            if index == 0 {
                format!(
                    "{ESCAPE_PREFIX}a=t,q=2,i={},f=32,s={},v={},o=z,m={};{}{ESCAPE_SUFFIX}",
                    image_id, rendered.bitmap_width, rendered.bitmap_height, has_more, chunk,
                )
            } else {
                format!("{ESCAPE_PREFIX}m={},q=2;{}{ESCAPE_SUFFIX}", has_more, chunk,)
            }
        })
        .collect()
}

pub fn encode_put_existing_image(
    rendered: &RenderedPage,
    ids: KittyImageIds,
    z_index: i32,
) -> String {
    format!(
        "{ESCAPE_PREFIX}a=p,q=2,i={},p={},c={},r={},C=1,z={}{ESCAPE_SUFFIX}",
        ids.image_id,
        ids.placement_id,
        rendered.placement_columns,
        rendered.placement_rows,
        z_index,
    )
}

pub fn encode_positioned_put_existing_image(
    rendered: &RenderedPage,
    ids: KittyImageIds,
    z_index: i32,
) -> String {
    format!(
        "\x1b[{};{}H{}",
        rendered.placement_row + 1,
        rendered.placement_col + 1,
        encode_put_existing_image(rendered, ids, z_index)
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BitmapKey {
    pub page_index: usize,
    pub placement_col: u16,
    pub placement_row: u16,
    pub placement_columns: u16,
    pub placement_rows: u16,
    pub bitmap_width: u32,
    pub bitmap_height: u32,
    pub rgba_hash: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacementState {
    pub image_id: u32,
    pub placement_id: u32,
}

#[derive(Clone, Debug)]
pub struct RendererState {
    next_image_id: u32,
    placements: HashMap<usize, PlacementState>,
    last_bitmaps: HashMap<usize, BitmapKey>,
    next_z: i32,
}

#[derive(Clone, Debug)]
pub struct HighlightRendererState {
    next_image_id: u32,
    placements: HashMap<usize, PlacementState>,
    last_bitmaps: HashMap<usize, BitmapKey>,
    next_z: i32,
}

impl Default for HighlightRendererState {
    fn default() -> Self {
        Self {
            next_image_id: 10_000,
            placements: HashMap::new(),
            last_bitmaps: HashMap::new(),
            next_z: -1_000_000_000,
        }
    }
}

impl Default for RendererState {
    fn default() -> Self {
        Self {
            next_image_id: 1,
            placements: HashMap::new(),
            last_bitmaps: HashMap::new(),
            next_z: -1_000_000_000,
        }
    }
}

impl RendererState {
    pub fn prepare_commands(&mut self, rendered_pages: &[RenderedPage]) -> Vec<String> {
        let mut commands = Vec::new();
        let visible_pages = rendered_pages
            .iter()
            .map(|rendered| rendered.page_index)
            .collect::<Vec<_>>();

        let stale_pages = self
            .placements
            .keys()
            .copied()
            .filter(|page_index| !visible_pages.contains(page_index))
            .collect::<Vec<_>>();

        for page_index in stale_pages {
            if let Some(placement) = self.placements.remove(&page_index) {
                commands.push(encode_delete_image(KittyImageIds {
                    image_id: placement.image_id,
                    placement_id: placement.placement_id,
                }));
            }
            self.last_bitmaps.remove(&page_index);
        }

        for rendered in rendered_pages {
            commands.extend(self.prepare_page_commands(rendered));
        }

        commands
    }

    fn prepare_page_commands(&mut self, rendered: &RenderedPage) -> Vec<String> {
        let bitmap_key = BitmapKey {
            page_index: rendered.page_index,
            placement_col: rendered.placement_col,
            placement_row: rendered.placement_row,
            placement_columns: rendered.placement_columns,
            placement_rows: rendered.placement_rows,
            bitmap_width: rendered.bitmap_width,
            bitmap_height: rendered.bitmap_height,
            rgba_hash: rgba_hash(&rendered.rgba),
        };
        let mut commands = Vec::new();
        let last_bitmap = self.last_bitmaps.get(&rendered.page_index).copied();

        if last_bitmap != Some(bitmap_key) {
            let placement = self
                .placements
                .entry(rendered.page_index)
                .or_insert_with(|| PlacementState {
                    image_id: 0,
                    placement_id: rendered.page_index as u32 + 1,
                });
            let previous_image_id = placement.image_id;
            let image_id = self.next_image_id;
            self.next_image_id += 1;
            placement.image_id = image_id;
            commands.extend(encode_transmit_only(rendered, image_id));
            commands.push(encode_positioned_put_existing_image(
                rendered,
                KittyImageIds {
                    image_id: placement.image_id,
                    placement_id: placement.placement_id,
                },
                self.next_z,
            ));
            self.next_z = self.next_z.saturating_add(1);
            if previous_image_id != 0 {
                commands.push(encode_delete_image(KittyImageIds {
                    image_id: previous_image_id,
                    placement_id: placement.placement_id,
                }));
            }
            self.last_bitmaps.insert(rendered.page_index, bitmap_key);
        }

        commands
    }

    pub fn clear_commands(&mut self) -> Vec<String> {
        let commands = self
            .placements
            .values()
            .map(|placement| {
                encode_delete_image(KittyImageIds {
                    image_id: placement.image_id,
                    placement_id: placement.placement_id,
                })
            })
            .collect::<Vec<_>>();
        self.placements.clear();
        self.last_bitmaps.clear();
        commands
    }
}

impl HighlightRendererState {
    pub fn prepare_commands(&mut self, rendered: &RenderedPage) -> Vec<String> {
        let bitmap_key = BitmapKey {
            page_index: rendered.page_index,
            placement_col: rendered.placement_col,
            placement_row: rendered.placement_row,
            placement_columns: rendered.placement_columns,
            placement_rows: rendered.placement_rows,
            bitmap_width: rendered.bitmap_width,
            bitmap_height: rendered.bitmap_height,
            rgba_hash: rgba_hash(&rendered.rgba),
        };
        let mut commands = Vec::new();
        let last_bitmap = self.last_bitmaps.get(&rendered.page_index).copied();

        if last_bitmap != Some(bitmap_key) {
            let placement = self
                .placements
                .entry(rendered.page_index)
                .or_insert_with(|| PlacementState {
                    image_id: 0,
                    placement_id: rendered.page_index as u32 + 10_001,
                });
            let previous_image_id = placement.image_id;
            let image_id = self.next_image_id;
            self.next_image_id += 1;
            placement.image_id = image_id;
            commands.extend(encode_transmit_only(rendered, image_id));
            commands.push(encode_positioned_put_existing_image(
                rendered,
                KittyImageIds {
                    image_id: placement.image_id,
                    placement_id: placement.placement_id,
                },
                self.next_z,
            ));
            self.next_z = self.next_z.saturating_add(1);
            if previous_image_id != 0 {
                commands.push(encode_delete_image(KittyImageIds {
                    image_id: previous_image_id,
                    placement_id: placement.placement_id,
                }));
            }
            self.last_bitmaps.insert(rendered.page_index, bitmap_key);
        }

        commands
    }
}

fn encode_rgba_payload(rgba: &[u8]) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(rgba)
        .expect("writing RGBA bytes into zlib encoder should succeed");
    let compressed = encoder
        .finish()
        .expect("finishing zlib encoder should succeed");
    STANDARD.encode(compressed)
}

fn rgba_hash(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}
