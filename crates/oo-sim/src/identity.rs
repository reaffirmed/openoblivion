//! What names a thing in the world, in a way that outlives the session.
//!
//! In memory every form (base objects, placed references, quests, globals) is
//! named by its *global* form ID: the low 24 bits are the object's number in
//! the plugin that first defines it, and the top byte is that plugin's place
//! in the load order (see [`LoadOrder`]). That is stable while the load order
//! is, and it's what every map in [`ScriptState`](crate::ScriptState) and
//! [`WorldChanges`](crate::WorldChanges) is keyed by. A Bevy `Entity` is only
//! a handle to whatever is drawn or simulated right now; it never names
//! anything that is kept.
//!
//! There are three kinds of identity:
//!
//! - **Plugin-defined**: anything a plugin defines. Its stable form is a
//!   [`FormKey`], the defining plugin's file name plus the 24-bit object
//!   number, so a save made under one load order loads under another, and
//!   entries from a plugin that's gone can be dropped instead of landing on
//!   another plugin's records.
//! - **Created at run time**: references the game makes as it goes (an item
//!   dropped, `PlaceAtMe`). They get IDs whose top byte is [`RUNTIME_TOP`],
//!   which no plugin can have since at most 255 plugins load, from
//!   [`RuntimeIds`]. They're saved as they are; the counter is saved too, so
//!   an ID is never handed out twice.
//! - **Needing persistent state** isn't a third kind of ID but a property:
//!   anything with an entry in the change maps is saved, everything else is
//!   rebuilt from the plugins when its cell loads.

use bevy::prelude::*;
use oo_formats::load_order::LoadOrder;

/// The top byte of form IDs made at run time. Plugins take 0x00 to 0xFE.
pub const RUNTIME_TOP: u8 = 0xFF;

/// At most this many plugins load, so that [`RUNTIME_TOP`] stays free.
pub const MAX_PLUGINS: usize = RUNTIME_TOP as usize;

const _: () = assert!(MAX_PLUGINS <= RUNTIME_TOP as usize);

/// Whether a form ID was made at run time rather than defined by a plugin.
pub fn is_runtime(id: u32) -> bool {
    id >> 24 == u32::from(RUNTIME_TOP)
}

/// A plugin-defined form, named so that it survives load-order changes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FormKey {
    /// The defining plugin's file name, as it is in `Data`. Compared without
    /// regard to case, as the game does.
    pub plugin: String,
    /// The low 24 bits of the form ID.
    pub object: u32,
}

/// Hands out IDs for references made at run time. Saved with the game.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeIds {
    next: u32,
}

impl Default for RuntimeIds {
    fn default() -> Self {
        // 0xFF000000 itself is left unused, so no runtime ID is ever "plugin 0xFF, object 0".
        Self { next: 1 }
    }
}

impl RuntimeIds {
    /// Continues from a saved counter.
    pub fn resume(next: u32) -> Self {
        Self {
            next: next.clamp(1, 0x0100_0000),
        }
    }

    /// The counter as saved: the low 24 bits of the next ID to hand out.
    pub fn next(&self) -> u32 {
        self.next
    }

    /// A new ID, or `None` once all 16 million have been used.
    pub fn allocate(&mut self) -> Option<u32> {
        if self.next > 0x00FF_FFFF {
            return None;
        }
        let id = (u32::from(RUNTIME_TOP) << 24) | self.next;
        self.next += 1;
        Some(id)
    }
}

/// Plugin file names in load order, which is how a save records what its form
/// IDs' top bytes mean.
pub fn plugin_names(order: &LoadOrder) -> Vec<String> {
    order
        .plugins()
        .map(|(_, name, _)| name.to_owned())
        .collect()
}

/// A global form ID's stable name under load order `plugins`, or `None` for
/// runtime IDs, 0, and top bytes past the end of the load order.
pub fn form_key(plugins: &[String], id: u32) -> Option<FormKey> {
    if id == 0 || is_runtime(id) {
        return None;
    }
    let plugin = plugins.get((id >> 24) as usize)?;
    Some(FormKey {
        plugin: plugin.clone(),
        object: id & 0x00FF_FFFF,
    })
}

/// A stable name back to a global form ID under load order `plugins`, or
/// `None` if that plugin isn't loaded.
pub fn resolve(plugins: &[String], key: &FormKey) -> Option<u32> {
    let index = plugins
        .iter()
        .position(|p| p.eq_ignore_ascii_case(&key.plugin))?;
    Some(((index as u32) << 24) | (key.object & 0x00FF_FFFF))
}

/// Rewrites global form IDs from one load order to another: a save's to the
/// one loaded now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remap {
    /// Old top byte → new top byte, or `None` for a plugin that isn't loaded.
    tops: Vec<Option<u8>>,
}

impl Remap {
    pub fn new(from: &[String], to: &[String]) -> Self {
        Self {
            tops: from
                .iter()
                .map(|name| {
                    to.iter()
                        .position(|p| p.eq_ignore_ascii_case(name))
                        .map(|i| i as u8)
                })
                .collect(),
        }
    }

    /// Whether every ID stays as it is.
    pub fn is_identity(&self) -> bool {
        self.tops
            .iter()
            .enumerate()
            .all(|(i, t)| *t == Some(i as u8))
    }

    /// The old load order's plugins that aren't loaded now, by old index.
    pub fn missing(&self) -> impl Iterator<Item = usize> + '_ {
        self.tops
            .iter()
            .enumerate()
            .filter(|(_, t)| t.is_none())
            .map(|(i, _)| i)
    }

    /// An ID under the new load order. 0 and runtime IDs stay as they are;
    /// IDs from a plugin that isn't loaded, or past the end of the old load
    /// order, have no counterpart.
    pub fn id(&self, id: u32) -> Option<u32> {
        if id == 0 || is_runtime(id) {
            return Some(id);
        }
        let top = (*self.tops.get((id >> 24) as usize)?)?;
        Some((u32::from(top) << 24) | (id & 0x00FF_FFFF))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn runtime_ids_never_collide_with_plugins() {
        let mut ids = RuntimeIds::default();
        let a = ids.allocate().unwrap();
        let b = ids.allocate().unwrap();
        assert_ne!(a, b);
        assert!(is_runtime(a) && is_runtime(b));
        assert_eq!(a, 0xFF00_0001);
        // The last plugin a load order can hold still isn't runtime.
        assert!(!is_runtime(((MAX_PLUGINS as u32 - 1) << 24) | 5));
        // A resumed counter carries on rather than repeating.
        let mut resumed = RuntimeIds::resume(ids.next());
        assert_eq!(resumed.allocate(), Some(0xFF00_0003));
        // And runs out instead of wrapping.
        let mut full = RuntimeIds::resume(0x00FF_FFFF);
        assert_eq!(full.allocate(), Some(0xFFFF_FFFF));
        assert_eq!(full.allocate(), None);
    }

    #[test]
    fn form_keys_round_trip_through_a_reordered_load_order() {
        let before = names(&["Oblivion.esm", "Knights.esp", "MyMod.esp"]);
        let after = names(&["oblivion.esm", "MyMod.esp", "Knights.esp"]);
        let id = 0x0200_1234; // MyMod's object 0x1234
        let key = form_key(&before, id).unwrap();
        assert_eq!(key.plugin, "MyMod.esp");
        assert_eq!(key.object, 0x1234);
        assert_eq!(resolve(&after, &key), Some(0x0100_1234));
        // Oblivion.esm's hard-coded player reference keeps its ID.
        let player = form_key(&before, crate::script::PLAYER_REF).unwrap();
        assert_eq!(resolve(&after, &player), Some(crate::script::PLAYER_REF));

        let remap = Remap::new(&before, &after);
        assert!(!remap.is_identity());
        assert_eq!(remap.id(0x0200_1234), Some(0x0100_1234));
        assert_eq!(remap.id(0x0100_0042), Some(0x0200_0042));
        assert_eq!(remap.id(0x0000_0014), Some(0x0000_0014));
        assert_eq!(remap.id(0), Some(0));
        assert_eq!(remap.id(0xFF00_0007), Some(0xFF00_0007));
        assert!(Remap::new(&before, &before).is_identity());
    }

    #[test]
    fn a_removed_plugin_drops_its_forms() {
        let before = names(&["Oblivion.esm", "Gone.esp", "Kept.esp"]);
        let after = names(&["Oblivion.esm", "Kept.esp"]);
        let remap = Remap::new(&before, &after);
        assert_eq!(remap.missing().collect::<Vec<_>>(), [1]);
        assert_eq!(remap.id(0x0100_0800), None);
        assert_eq!(remap.id(0x0200_0800), Some(0x0100_0800));
        // A top byte the save never had is corrupt, not plugin 0.
        assert_eq!(remap.id(0x0500_0001), None);
        assert_eq!(
            resolve(
                &after,
                &FormKey {
                    plugin: "Gone.esp".into(),
                    object: 0x800
                }
            ),
            None
        );
    }
}
