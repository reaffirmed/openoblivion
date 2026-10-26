//! Items, containers and leveled lists.
//!
//! Form IDs in and out are global (see [`LoadOrder`]).

use std::collections::HashMap;

use crate::Result;
use crate::esm::{Node, Plugin, Record, Tag};
use crate::load_order::LoadOrder;

/// Record types the player can pick up and carry.
pub const ITEM_TYPES: [&[u8; 4]; 12] = [
    b"MISC", b"WEAP", b"ARMO", b"CLOT", b"BOOK", b"INGR", b"ALCH", b"KEYM", b"AMMO", b"SGST",
    b"SLGM", b"APPA",
];

const ITEM_AND_LIGHT_TYPES: [&[u8; 4]; 13] = [
    b"MISC", b"WEAP", b"ARMO", b"CLOT", b"BOOK", b"INGR", b"ALCH", b"KEYM", b"AMMO", b"SGST",
    b"SLGM", b"APPA", b"LIGH",
];

/// What the inventory needs to know about an item's base record.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemInfo {
    pub tag: Tag,
    /// In-game name (`FULL`), falling back to the editor ID.
    pub name: String,
    /// Inventory icon (`ICON`), relative to `textures\menus\icons\`.
    pub icon: Option<String>,
    /// Base value in gold.
    pub value: i32,
    pub weight: f32,
}

/// Where each item type keeps its value and weight in `DATA` (or, for
/// ingredients and potions, the value in `ENIT`): byte offsets.
fn value_weight_offsets(tag: &[u8; 4]) -> (Option<(&'static [u8; 4], usize)>, Option<usize>) {
    match tag {
        b"MISC" | b"KEYM" => (Some((b"DATA", 0)), Some(4)),
        b"WEAP" => (Some((b"DATA", 16)), Some(24)),
        b"ARMO" => (Some((b"DATA", 2)), Some(10)),
        b"CLOT" | b"SLGM" => (Some((b"DATA", 0)), Some(4)),
        b"BOOK" | b"APPA" | b"SGST" => (Some((b"DATA", 1)), Some(5)),
        b"AMMO" => (Some((b"DATA", 8)), Some(12)),
        b"INGR" | b"ALCH" => (Some((b"ENIT", 0)), Some(0)),
        b"LIGH" => (Some((b"DATA", 24)), Some(28)),
        _ => (None, None),
    }
}

fn item_details(r: &Record<'_>) -> Result<(Option<String>, i32, f32)> {
    let (value_at, weight_at) = value_weight_offsets(&r.tag.0);
    let (mut icon, mut value, mut weight) = (None, 0, 0.0);
    let fields = r.fields()?;
    for f in fields.iter() {
        let f = f?;
        let d = f.data;
        if f.tag.0 == *b"ICON" && icon.is_none() {
            icon = Some(f.zstring()).filter(|s| !s.is_empty());
        }
        if let Some((tag, at)) = value_at
            && f.tag.0 == *tag
            && d.len() >= at + 4
        {
            value = u32_at(d, at) as i32;
        }
        if let Some(at) = weight_at
            && f.tag.0 == *b"DATA"
            && d.len() >= at + 4
        {
            weight = f32::from_le_bytes(d[at..at + 4].try_into().unwrap());
        }
    }
    Ok((icon, value, weight))
}

/// One line of a container's or leveled list's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stack {
    pub item: u32,
    pub count: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContainerInfo {
    pub name: String,
    /// Base contents (`CNTO`); entries may be leveled lists.
    pub contents: Vec<Stack>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeveledList {
    /// Percent chance the list gives nothing.
    pub chance_none: u8,
    pub flags: u8,
    pub entries: Vec<LeveledEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeveledEntry {
    pub level: u16,
    pub item: u32,
    pub count: i16,
}

impl LeveledList {
    /// Choose from every entry at or below the player's level, not just the highest.
    pub const ALL_LEVELS: u8 = 0x1;
    /// Roll separately for each item in the count.
    pub const EACH_ITEM: u8 = 0x2;
}

/// Every carriable item, by global form ID. Light records count only if they
/// can be carried (torches).
pub fn items(order: &LoadOrder) -> Result<HashMap<u32, ItemInfo>> {
    let mut out = HashMap::new();
    for_each_top_record(order, &ITEM_AND_LIGHT_TYPES, |index, r| {
        let id = order.global(index, r.form_id);
        let carriable =
            ITEM_TYPES.contains(&&r.tag.0) || (r.tag.0 == *b"LIGH" && light_can_carry(r)?);
        if !carriable {
            return Ok(());
        }
        if r.is_deleted() {
            out.remove(&id);
            return Ok(());
        }
        let (edid, full) = names(r)?;
        let (icon, value, weight) = item_details(r)?;
        out.insert(
            id,
            ItemInfo {
                tag: r.tag,
                name: full.or(edid).unwrap_or_default(),
                icon,
                value,
                weight,
            },
        );
        Ok(())
    })?;
    Ok(out)
}

/// Every container base record (`CONT`), by global form ID.
pub fn containers(order: &LoadOrder) -> Result<HashMap<u32, ContainerInfo>> {
    let mut out = HashMap::new();
    for_each_top_record(order, &[b"CONT"], |index, r| {
        if r.tag.0 != *b"CONT" {
            return Ok(());
        }
        let id = order.global(index, r.form_id);
        if r.is_deleted() {
            out.remove(&id);
            return Ok(());
        }
        let (edid, full) = names(r)?;
        let mut contents = Vec::new();
        let fields = r.fields()?;
        for f in fields.iter() {
            let f = f?;
            if f.tag.0 == *b"CNTO" && f.data.len() >= 8 {
                contents.push(Stack {
                    item: order.global(index, u32_at(f.data, 0)),
                    count: u32_at(f.data, 4) as i32,
                });
            }
        }
        out.insert(
            id,
            ContainerInfo {
                name: full.or(edid).unwrap_or_default(),
                contents,
            },
        );
        Ok(())
    })?;
    Ok(out)
}

/// Every leveled item list (`LVLI`), by global form ID.
pub fn leveled_items(order: &LoadOrder) -> Result<HashMap<u32, LeveledList>> {
    let mut out = HashMap::new();
    for_each_top_record(order, &[b"LVLI"], |index, r| {
        if r.tag.0 != *b"LVLI" {
            return Ok(());
        }
        let id = order.global(index, r.form_id);
        if r.is_deleted() {
            out.remove(&id);
            return Ok(());
        }
        let mut list = LeveledList {
            chance_none: 0,
            flags: 0,
            entries: Vec::new(),
        };
        let fields = r.fields()?;
        for f in fields.iter() {
            let f = f?;
            let d = f.data;
            match &f.tag.0 {
                b"LVLD" if !d.is_empty() => list.chance_none = d[0],
                b"LVLF" if !d.is_empty() => list.flags = d[0],
                b"LVLO" if d.len() >= 8 => list.entries.push(LeveledEntry {
                    level: u16::from_le_bytes([d[0], d[1]]),
                    item: order.global(index, u32_at(d, 4)),
                    count: if d.len() >= 10 {
                        i16::from_le_bytes([d[8], d[9]])
                    } else {
                        1
                    },
                }),
                _ => {}
            }
        }
        out.insert(id, list);
        Ok(())
    })?;
    Ok(out)
}

/// Resolves contents that may include leveled lists into plain items, for a
/// player at `level`. `seed` makes the rolls repeatable: use the container
/// reference's form ID so every co-op peer, and every reload, agrees.
pub fn resolve_contents(
    contents: &[Stack],
    leveled: &HashMap<u32, LeveledList>,
    level: u16,
    seed: u64,
) -> Vec<Stack> {
    let mut rng = SplitMix(seed);
    let mut out: Vec<Stack> = Vec::new();
    for s in contents {
        resolve_into(
            s.item,
            s.count.max(0),
            leveled,
            level,
            &mut rng,
            &mut out,
            0,
        );
    }
    out
}

fn resolve_into(
    item: u32,
    count: i32,
    leveled: &HashMap<u32, LeveledList>,
    level: u16,
    rng: &mut SplitMix,
    out: &mut Vec<Stack>,
    depth: u8,
) {
    let Some(list) = leveled.get(&item) else {
        match out.iter_mut().find(|s| s.item == item) {
            Some(s) => s.count += count,
            None => out.push(Stack { item, count }),
        }
        return;
    };
    // Guard against lists that contain themselves.
    if depth > 16 {
        return;
    }
    let rolls = if list.flags & LeveledList::EACH_ITEM != 0 {
        count
    } else {
        1
    };
    let per_roll = if list.flags & LeveledList::EACH_ITEM != 0 {
        1
    } else {
        count
    };
    for _ in 0..rolls {
        if rng.below(100) < u64::from(list.chance_none) {
            continue;
        }
        let eligible: Vec<&LeveledEntry> =
            list.entries.iter().filter(|e| e.level <= level).collect();
        let pool: Vec<&LeveledEntry> = if list.flags & LeveledList::ALL_LEVELS != 0 {
            eligible
        } else {
            let top = eligible.iter().map(|e| e.level).max();
            eligible
                .into_iter()
                .filter(|e| Some(e.level) == top)
                .collect()
        };
        if pool.is_empty() {
            continue;
        }
        let pick = pool[rng.below(pool.len() as u64) as usize];
        resolve_into(
            pick.item,
            i32::from(pick.count.max(1)) * per_roll,
            leveled,
            level,
            rng,
            out,
            depth + 1,
        );
    }
}

/// A tiny deterministic generator; quality doesn't matter, repeatability does.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Every record of the given types in every plugin, in load order, with the
/// plugin's index. Only top-level groups: base records never live in cells.
pub(crate) fn for_each_top_record(
    order: &LoadOrder,
    types: &[&[u8; 4]],
    mut f: impl FnMut(usize, &Record<'_>) -> Result<()>,
) -> Result<()> {
    for (index, _, plugin) in order.plugins() {
        walk_top(plugin, types, &mut |r| f(index, r))?;
    }
    Ok(())
}

fn walk_top(
    plugin: &Plugin,
    types: &[&[u8; 4]],
    f: &mut dyn FnMut(&Record<'_>) -> Result<()>,
) -> Result<()> {
    for node in plugin.top_level() {
        let Node::Group(g) = node? else { continue };
        let Some(tag) = g.record_type() else { continue };
        if !types.contains(&&tag.0) {
            continue;
        }
        for node in g.children() {
            if let Node::Record(r) = node? {
                f(&r)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn names(r: &Record<'_>) -> Result<(Option<String>, Option<String>)> {
    let (mut edid, mut full) = (None, None);
    let fields = r.fields()?;
    for f in fields.iter() {
        let f = f?;
        match &f.tag.0 {
            b"EDID" => edid = Some(f.zstring()),
            b"FULL" => full = Some(f.zstring()).filter(|s| !s.is_empty()),
            _ => {}
        }
    }
    Ok((edid, full))
}

fn light_can_carry(r: &Record<'_>) -> Result<bool> {
    let fields = r.fields()?;
    for f in fields.iter() {
        let f = f?;
        if f.tag.0 == *b"DATA" && f.data.len() >= 16 {
            return Ok(u32_at(f.data, 12) & 0x2 != 0);
        }
    }
    Ok(false)
}

pub(crate) fn u32_at(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
