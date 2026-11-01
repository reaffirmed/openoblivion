//! Exterior terrain lookup on a synthetic plugin.

mod common;

use common::{field, group, record, zstr};
use oo_formats::esm::Plugin;
use oo_formats::land::{GRID, exterior_land, land_textures};
use oo_formats::load_order::LoadOrder;

fn land() -> Vec<u8> {
    let mut vhgt = 2.0f32.to_le_bytes().to_vec();
    let mut steps = vec![0u8; GRID * GRID];
    steps[0] = 1; // first point 3, the rest of the cell follows it
    vhgt.extend(steps);
    vhgt.extend([0, 0, 0]);
    let mut atxt = 0x20u32.to_le_bytes().to_vec();
    atxt.extend([1, 0, 2, 0]); // south-east quarter, slot 2
    let mut vtxt = 5u16.to_le_bytes().to_vec();
    vtxt.extend([0, 0]);
    vtxt.extend(0.75f32.to_le_bytes());
    let mut btxt = 0x21u32.to_le_bytes().to_vec();
    btxt.extend([0, 0, 0, 0]);
    record(
        b"LAND",
        0,
        0x40,
        &[
            field(b"DATA", &7u32.to_le_bytes()),
            field(b"VNML", &[0, 0, 127].repeat(GRID * GRID)),
            field(b"VHGT", &vhgt),
            field(b"BTXT", &btxt),
            field(b"ATXT", &atxt),
            field(b"VTXT", &vtxt),
        ],
    )
}

fn cell(form_id: u32, x: i32, y: i32, children: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let grid: Vec<u8> = [x, y].iter().flat_map(|v| v.to_le_bytes()).collect();
    let mut out = vec![record(b"CELL", 0, form_id, &[field(b"XCLC", &grid)])];
    if !children.is_empty() {
        out.push(group(
            &form_id.to_le_bytes(),
            6,
            &[group(&form_id.to_le_bytes(), 9, &children)],
        ));
    }
    out
}

fn master() -> Plugin {
    let tes4 = record(b"TES4", 1, 0, &[field(b"HEDR", &[0; 12])]);
    let ltex = group(
        b"LTEX",
        0,
        &[record(
            b"LTEX",
            0,
            0x20,
            &[
                field(b"EDID", &zstr("TerrainGrass")),
                field(b"ICON", &zstr("TerrainHDGrass01.dds")),
                field(b"SNAM", &[30]),
                field(b"GNAM", &0x30u32.to_le_bytes()),
            ],
        )],
    );
    let mut cells = cell(0x11, 0, 0, vec![]);
    cells.extend(cell(0x12, 3, -2, vec![land()]));
    let sub_block = group(&[0; 4], 5, &cells);
    let block = group(&[0; 4], 4, &[sub_block]);
    let wrld = group(
        b"WRLD",
        0,
        &[
            record(b"WRLD", 0, 0x10, &[field(b"EDID", &zstr("Tamriel"))]),
            group(&0x10u32.to_le_bytes(), 1, &[block]),
        ],
    );
    Plugin::from_bytes([tes4, ltex, wrld].concat()).unwrap()
}

#[test]
fn finds_a_cells_terrain_by_grid() {
    let order = LoadOrder::new(vec![("Test.esm".into(), master())]).unwrap();
    assert!(exterior_land(&order, 0x10, 0, 0).unwrap().is_none());
    assert!(exterior_land(&order, 0x99, 3, -2).unwrap().is_none());
    let land = exterior_land(&order, 0x10, 3, -2).unwrap().unwrap();
    assert_eq!(land.form_id, 0x40);
    assert_eq!(land.height(0, 0), Some(24.0));
    assert_eq!(land.height(32, 32), Some(24.0));
    assert_eq!(land.normals.as_ref().unwrap()[0], [0.0, 0.0, 1.0]);
    assert_eq!(land.quarters[0].base, Some(0x21));
    let layer = &land.quarters[1].layers[0];
    assert_eq!((layer.texture, layer.layer), (0x20, 2));
    assert_eq!(layer.opacity[5], 0.75);
    let mesh = land.quarter_mesh(1, [3.0 * 4096.0, -2.0 * 4096.0]).unwrap();
    assert_eq!(
        mesh.positions[0],
        [3.0 * 4096.0 + 2048.0, -2.0 * 4096.0, 24.0]
    );
    assert_eq!(mesh.textures, vec![None, Some(0x20)]);
    assert_eq!(mesh.weights[5], vec![0.25, 0.75]);
}

#[test]
fn reads_oblivion_land_textures() {
    let order = LoadOrder::new(vec![("Test.esm".into(), master())]).unwrap();
    let textures = land_textures(&order).unwrap();
    let t = &textures[&0x20];
    assert_eq!(t.editor_id.as_deref(), Some("TerrainGrass"));
    assert_eq!(t.texture.as_deref(), Some("TerrainHDGrass01.dds"));
    assert_eq!(t.specular_exponent, Some(30));
    assert_eq!(t.grasses, vec![0x30]);
}
