//! Interior cell lookup and model indexing on a synthetic plugin.

mod common;

use common::{field, group, record, zstr};
use oo_formats::esm::{Plugin, Tag, flags};
use oo_formats::load_order::LoadOrder;
use oo_formats::world::{
    Teleport, form_id_by_editor_id, interior_cell, interior_cell_by_form_id, interior_cell_ids,
    interior_cell_names, interior_reference_cells, model_paths,
};

fn refr(
    form_id: u32,
    base: u32,
    pos: [f32; 3],
    rot: [f32; 3],
    scale: Option<f32>,
    flags: u32,
) -> Vec<u8> {
    let data: Vec<u8> = pos
        .iter()
        .chain(&rot)
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut fields = vec![field(b"NAME", &base.to_le_bytes()), field(b"DATA", &data)];
    if let Some(s) = scale {
        fields.push(field(b"XSCL", &s.to_le_bytes()));
    }
    record(b"REFR", flags, form_id, &fields)
}

fn plugin() -> LoadOrder {
    LoadOrder::new(vec![("Test.esm".into(), master())]).unwrap()
}

fn door(form_id: u32, base: u32, pos: [f32; 3], rot: [f32; 3], destination: u32) -> Vec<u8> {
    let data: Vec<u8> = pos
        .iter()
        .chain(&rot)
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let mut xtel = destination.to_le_bytes().to_vec();
    xtel.extend(
        [7.0f32, 8.0, 9.0, 0.0, 0.0, 3.0]
            .iter()
            .flat_map(|v| v.to_le_bytes()),
    );
    record(
        b"REFR",
        0,
        form_id,
        &[
            field(b"NAME", &base.to_le_bytes()),
            field(b"DATA", &data),
            field(b"XTEL", &xtel),
        ],
    )
}

fn cell(
    form_id: u32,
    edid: &str,
    persistent: Vec<Vec<u8>>,
    temporary: Vec<Vec<u8>>,
) -> Vec<Vec<u8>> {
    let mut children = Vec::new();
    if !persistent.is_empty() {
        children.push(group(&form_id.to_le_bytes(), 8, &persistent));
    }
    if !temporary.is_empty() {
        children.push(group(&form_id.to_le_bytes(), 9, &temporary));
    }
    let mut fields = vec![field(b"EDID", &zstr(edid))];
    if edid == "TestTavern" {
        fields.push(field(b"FULL", &zstr("The Test Tavern")));
    }
    let mut out = vec![record(b"CELL", 0, form_id, &fields)];
    if !children.is_empty() {
        out.push(group(&form_id.to_le_bytes(), 6, &children));
    }
    out
}

fn master() -> Plugin {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(b"TES4", flags::MASTER, 0, &[field(b"HEDR", &hedr)]);
    let stats = group(
        b"STAT",
        0,
        &[
            record(
                b"STAT",
                0,
                0x10,
                &[
                    field(b"EDID", &zstr("Barrel")),
                    field(b"MODL", &zstr("Clutter\\Barrel01.NIF")),
                ],
            ),
            record(
                b"STAT",
                flags::COMPRESSED,
                0x11,
                &[field(b"MODL", &zstr("Arch\\Wall.nif"))],
            ),
        ],
    );
    let mut subblock = cell(0x100, "EmptyCell", vec![], vec![]);
    subblock.extend(cell(
        0x200,
        "TestTavern",
        vec![door(0x201, 0x10, [1.0, 2.0, 3.0], [0.0, 0.0, 1.5], 0x301)],
        vec![
            refr(0x202, 0x11, [4.0, 5.0, 6.0], [0.1, 0.2, 0.3], Some(2.0), 0),
            refr(0x203, 0x11, [0.0; 3], [0.0; 3], None, flags::DELETED),
        ],
    ));
    subblock.extend(cell(
        0x300,
        "LastCell",
        vec![],
        vec![refr(0x301, 0x10, [9.0; 3], [0.0; 3], None, 0)],
    ));
    subblock.extend(cell(0x400, "TrailingCell", vec![], vec![]));
    let cells = group(
        b"CELL",
        0,
        &[group(
            &0i32.to_le_bytes(),
            2,
            &[group(&0i32.to_le_bytes(), 3, &subblock)],
        )],
    );
    Plugin::from_bytes([header, stats, cells].concat()).unwrap()
}

#[test]
fn lists_interior_cells() {
    assert_eq!(
        interior_cell_ids(&plugin()).unwrap(),
        ["EmptyCell", "TestTavern", "LastCell", "TrailingCell"]
    );
}

#[test]
fn finds_cell_references() {
    let cell = interior_cell(&plugin(), "testtavern").unwrap().unwrap();
    assert_eq!(cell.form_id, 0x200);
    assert_eq!(
        cell.references.len(),
        2,
        "deleted reference should be skipped"
    );

    let a = &cell.references[0];
    assert_eq!((a.tag, a.form_id, a.base), (Tag(*b"REFR"), 0x201, 0x10));
    assert_eq!(a.position, [1.0, 2.0, 3.0]);
    assert_eq!(a.rotation, [0.0, 0.0, 1.5]);
    assert_eq!(a.scale, 1.0);
    assert_eq!(cell.references[1].scale, 2.0);
}

#[test]
fn cells_without_children_work() {
    assert!(
        interior_cell(&plugin(), "EmptyCell")
            .unwrap()
            .unwrap()
            .references
            .is_empty()
    );
    assert!(
        interior_cell(&plugin(), "TrailingCell")
            .unwrap()
            .unwrap()
            .references
            .is_empty()
    );
    assert!(interior_cell(&plugin(), "Nowhere").unwrap().is_none());
}

#[test]
fn indexes_models() {
    let models = model_paths(&plugin()).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[&0x10], "Clutter\\Barrel01.NIF");
    assert_eq!(models[&0x11], "Arch\\Wall.nif");
}

#[test]
fn finds_form_id_by_editor_id() {
    let p = plugin();
    assert_eq!(form_id_by_editor_id(&p, "barrel").unwrap(), Some(0x10));
    assert_eq!(form_id_by_editor_id(&p, "TestTavern").unwrap(), Some(0x200));
    assert_eq!(form_id_by_editor_id(&p, "Nothing").unwrap(), None);
}

#[test]
fn reads_load_doors_and_finds_their_destination_cell() {
    let p = plugin();
    let tavern = interior_cell(&p, "TestTavern").unwrap().unwrap();
    let teleport = tavern.references[0].teleport.unwrap();
    assert_eq!(
        teleport,
        Teleport {
            door: 0x301,
            position: [7.0, 8.0, 9.0],
            rotation: [0.0, 0.0, 3.0]
        }
    );
    assert_eq!(tavern.references[1].teleport, None);

    let cells = interior_reference_cells(&p).unwrap();
    assert_eq!(cells[&0x201], 0x200);
    assert_eq!(cells[&teleport.door], 0x300);
    let destination = interior_cell_by_form_id(&p, cells[&teleport.door])
        .unwrap()
        .unwrap();
    assert_eq!(destination.editor_id, "LastCell");
    assert_eq!(destination.references.len(), 1);
}

#[test]
fn reads_cell_names() {
    let p = plugin();
    assert_eq!(
        interior_cell(&p, "TestTavern")
            .unwrap()
            .unwrap()
            .name
            .as_deref(),
        Some("The Test Tavern")
    );
    assert_eq!(interior_cell(&p, "LastCell").unwrap().unwrap().name, None);
    let names = interior_cell_names(&p).unwrap();
    assert_eq!(names[&0x200], "The Test Tavern");
    assert_eq!(names[&0x300], "LastCell");
}

/// A mod that overrides and extends `master()`.
fn esp() -> Plugin {
    let mut hedr = 1.0f32.to_le_bytes().to_vec();
    hedr.extend_from_slice(&[0; 8]);
    let header = record(
        b"TES4",
        0,
        0,
        &[
            field(b"HEDR", &hedr),
            field(b"MAST", &zstr("Test.esm")),
            field(b"DATA", &[0; 8]),
        ],
    );
    // Index 00 is Test.esm, 01 is this plugin.
    let stats = group(
        b"STAT",
        0,
        &[
            // Retexture the barrel, and add a new static.
            record(
                b"STAT",
                0,
                0x10,
                &[
                    field(b"EDID", &zstr("Barrel")),
                    field(b"MODL", &zstr("Mod\\Barrel.nif")),
                ],
            ),
            record(
                b"STAT",
                0,
                0x0100_0050,
                &[
                    field(b"EDID", &zstr("ModStatue")),
                    field(b"MODL", &zstr("Mod\\Statue.nif")),
                ],
            ),
        ],
    );
    let tavern = vec![
        record(
            b"CELL",
            0,
            0x200,
            &[
                field(b"EDID", &zstr("TestTavern")),
                field(b"FULL", &zstr("Renamed Tavern")),
            ],
        ),
        group(
            &0x200u32.to_le_bytes(),
            6,
            &[group(
                &0x200u32.to_le_bytes(),
                9,
                &[
                    // Move the master's 0x202, delete its door 0x201, place a statue.
                    refr(0x202, 0x11, [40.0, 50.0, 60.0], [0.0; 3], None, 0),
                    record(b"REFR", flags::DELETED, 0x201, &[]),
                    refr(0x0100_0060, 0x0100_0050, [1.0; 3], [0.0; 3], None, 0),
                ],
            )],
        ),
    ];
    let cells = group(
        b"CELL",
        0,
        &[group(
            &0i32.to_le_bytes(),
            2,
            &[group(&0i32.to_le_bytes(), 3, &tavern)],
        )],
    );
    Plugin::from_bytes([header, stats, cells].concat()).unwrap()
}

#[test]
fn later_plugins_override_and_extend_earlier_ones() {
    // The mod's own IDs (top byte 01) land at its load order index, here 05.
    let mut plugins: Vec<(String, Plugin)> = (0..4)
        .map(|i| (format!("Filler{i}.esm"), master()))
        .collect();
    plugins.insert(0, ("Test.esm".into(), master()));
    plugins.push(("Mod.esp".into(), esp()));
    let order = LoadOrder::new(plugins).unwrap();
    assert_eq!(order.global(5, 0x0100_0050), 0x0500_0050);
    assert_eq!(order.global(5, 0x10), 0x10);

    let models = model_paths(&order).unwrap();
    assert_eq!(models[&0x10], "Mod\\Barrel.nif");
    assert_eq!(models[&0x0500_0050], "Mod\\Statue.nif");
    assert_eq!(models[&0x11], "Arch\\Wall.nif");

    let cell = interior_cell(&order, "TestTavern").unwrap().unwrap();
    assert_eq!(cell.name.as_deref(), Some("Renamed Tavern"));
    let refs: Vec<_> = cell
        .references
        .iter()
        .map(|r| (r.form_id, r.base, r.position))
        .collect();
    assert_eq!(
        refs,
        [
            (0x202, 0x11, [40.0, 50.0, 60.0]),
            (0x0500_0060, 0x0500_0050, [1.0; 3])
        ],
        "moved, deleted and added references"
    );
    assert_eq!(
        form_id_by_editor_id(&order, "ModStatue").unwrap(),
        Some(0x0500_0050)
    );
}

#[test]
fn missing_master_is_an_error() {
    assert!(LoadOrder::new(vec![("Mod.esp".into(), esp())]).is_err());
}
