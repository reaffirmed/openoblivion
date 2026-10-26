//! AI packages (`PACK`): what an actor does, where, when and with whom.
//!
//! Selection rules are in `docs/reverse-engineering/ai-packages.md`.

use std::collections::HashMap;

use crate::Result;
use crate::dialogue::{Condition, condition};
use crate::items::{for_each_top_record, u32_at};
use crate::load_order::LoadOrder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PackageKind {
    Find,
    Follow,
    Escort,
    Eat,
    Sleep,
    #[default]
    Wander,
    Travel,
    Accompany,
    UseItemAt,
    Ambush,
    FleeNotCombat,
    CastMagic,
    Other(u8),
}

impl From<u8> for PackageKind {
    fn from(v: u8) -> Self {
        match v {
            0 => Self::Find,
            1 => Self::Follow,
            2 => Self::Escort,
            3 => Self::Eat,
            4 => Self::Sleep,
            5 => Self::Wander,
            6 => Self::Travel,
            7 => Self::Accompany,
            8 => Self::UseItemAt,
            9 => Self::Ambush,
            10 => Self::FleeNotCombat,
            11 => Self::CastMagic,
            other => Self::Other(other),
        }
    }
}

/// Where a package happens (`PLDT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Location {
    NearReference(u32),
    InCell(u32),
    NearCurrentLocation,
    NearEditorLocation,
    ObjectId(u32),
    ObjectType(u32),
    Other(i32, u32),
}

/// Who or what a package is about (`PTDT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Reference(u32),
    ObjectId(u32),
    ObjectType(u32),
    Other(i32, u32),
}

/// When a package applies (`PSDT`). `None` means any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Schedule {
    /// 0-11.
    pub month: Option<u8>,
    /// 0-6 Sunday to Saturday; 7 weekdays, 8 weekends, 9 Mon/Wed/Fri, 10 Tue/Thu.
    pub day_of_week: Option<u8>,
    /// Day of the month, from 1.
    pub date: Option<u8>,
    pub hour: Option<u8>,
    /// Hours.
    pub duration: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Package {
    pub editor_id: String,
    pub kind: PackageKind,
    pub flags: u32,
    pub location: Option<Location>,
    /// Bethesda units.
    pub radius: i32,
    pub schedule: Schedule,
    pub target: Option<Target>,
    /// How many, or how far, depending on the package.
    pub target_count: i32,
    pub conditions: Vec<Condition>,
}

impl Package {
    pub const OFFERS_SERVICES: u32 = 0x1;
    pub const MUST_REACH_LOCATION: u32 = 0x2;
    pub const MUST_COMPLETE: u32 = 0x4;
    pub const LOCK_DOORS_AT_START: u32 = 0x8;
    pub const LOCK_DOORS_AT_END: u32 = 0x10;
    pub const LOCK_DOORS_AT_LOCATION: u32 = 0x20;
    pub const UNLOCK_DOORS_AT_START: u32 = 0x40;
    pub const UNLOCK_DOORS_AT_END: u32 = 0x80;
    pub const UNLOCK_DOORS_AT_LOCATION: u32 = 0x100;
    pub const CONTINUE_IF_PC_NEAR: u32 = 0x200;
    pub const ONCE_PER_DAY: u32 = 0x400;
    pub const SKIP_FALLOUT_BEHAVIOR: u32 = 0x1000;
    pub const ALWAYS_RUN: u32 = 0x2000;
    pub const ALWAYS_SNEAK: u32 = 0x20000;
    pub const ALLOW_SWIMMING: u32 = 0x40000;
    pub const ALLOW_FALLS: u32 = 0x80000;
    pub const ARMOR_UNEQUIPPED: u32 = 0x100000;
    pub const WEAPONS_UNEQUIPPED: u32 = 0x200000;
    pub const DEFENSIVE_COMBAT: u32 = 0x400000;
    pub const USE_HORSE: u32 = 0x800000;
    pub const NO_IDLE_ANIMS: u32 = 0x1000000;
}

pub fn packages(order: &LoadOrder) -> Result<HashMap<u32, Package>> {
    let mut out = HashMap::new();
    for_each_top_record(order, &[b"PACK"], |index, r| {
        let id = order.global(index, r.form_id);
        if r.is_deleted() {
            out.remove(&id);
            return Ok(());
        }
        let global = |local: u32| order.global(index, local);
        let mut p = Package::default();
        let fields = r.fields()?;
        for f in fields.iter() {
            let f = f?;
            let d = f.data;
            match &f.tag.0 {
                b"EDID" => p.editor_id = f.zstring(),
                // Newer records: u32 flags then the type; older: u16 flags.
                b"PKDT" if d.len() >= 8 => (p.flags, p.kind) = (u32_at(d, 0), d[4].into()),
                b"PKDT" if d.len() >= 4 => {
                    (p.flags, p.kind) = (u32::from(u16::from_le_bytes([d[0], d[1]])), d[2].into())
                }
                b"PLDT" if d.len() >= 12 => {
                    let value = u32_at(d, 4);
                    p.location = Some(match u32_at(d, 0) as i32 {
                        0 => Location::NearReference(global(value)),
                        1 => Location::InCell(global(value)),
                        2 => Location::NearCurrentLocation,
                        3 => Location::NearEditorLocation,
                        4 => Location::ObjectId(global(value)),
                        5 => Location::ObjectType(value),
                        other => Location::Other(other, value),
                    });
                    p.radius = u32_at(d, 8) as i32;
                }
                b"PSDT" if d.len() >= 8 => {
                    let any = |v: i8| u8::try_from(v).ok();
                    p.schedule = Schedule {
                        month: any(d[0] as i8),
                        day_of_week: any(d[1] as i8),
                        date: Some(d[2]).filter(|&v| v != 0),
                        hour: any(d[3] as i8),
                        duration: u32_at(d, 4) as i32,
                    };
                }
                b"PTDT" if d.len() >= 12 => {
                    let value = u32_at(d, 4);
                    p.target = Some(match u32_at(d, 0) as i32 {
                        0 => Target::Reference(global(value)),
                        1 => Target::ObjectId(global(value)),
                        2 => Target::ObjectType(value),
                        other => Target::Other(other, value),
                    });
                    p.target_count = u32_at(d, 8) as i32;
                }
                b"CTDA" | b"CTDT" => p.conditions.push(condition(&f, &global)?),
                _ => {}
            }
        }
        out.insert(id, p);
        Ok(())
    })?;
    Ok(out)
}
