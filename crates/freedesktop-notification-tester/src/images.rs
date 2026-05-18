use dbus::arg::{RefArg, Variant};

use crate::types::{ImageData, ImageDataTuple};

pub fn create_16x16_image_data() -> Variant<Box<dyn RefArg>> {
    let image_data_container = ImageData {
        width: 16,
        height: 16,
        rowstride: 16 * 3 * std::mem::size_of::<u8>() as i32,
        has_alpha: false,
        bits_per_sample: 3 * std::mem::size_of::<u8>() as i32,
        channels: 3,
        data: get_16x16_opaque_image(),
    };

    let image_data_tuple = ImageDataTuple::from(image_data_container);

    Variant(Box::new(image_data_tuple))
}

#[rustfmt::skip]
fn get_16x16_opaque_image() -> Vec<u8> {
    // Red, Green, Blue, Black
    let pattern = vec![
        0xFF, 0x00, 0x00,
        0x00, 0xFF, 0x00,
        0x00, 0x00, 0xFF,
        0x00, 0x00, 0x00,
    ];

    pattern.repeat(4 * 16)
}

pub fn create_24x24_image_data() -> Variant<Box<dyn RefArg>> {
    let image_data_container = ImageData {
        width: 24,
        height: 24,
        rowstride: 24 * 4 * std::mem::size_of::<u8>() as i32,
        has_alpha: true,
        bits_per_sample: 4 * std::mem::size_of::<u8>() as i32,
        channels: 4,
        data: get_24x24_translucent_image(),
    };

    let image_data_tuple = ImageDataTuple::from(image_data_container);

    Variant(Box::new(image_data_tuple))
}

#[rustfmt::skip]
fn get_24x24_translucent_image() -> Vec<u8> {
    // Red, Green, Blue. Full transparency.
    // Yellow, Cyan, Purple. Half transparent.
    let pattern = vec![
        0xFF, 0x00, 0x00, 0xFF,
        0x00, 0xFF, 0x00, 0xFF,
        0x00, 0x00, 0xFF, 0xFF,
        0xFF, 0xFF, 0x00, 0x80,
        0x00, 0xFF, 0xFF, 0x80,
        0xFF, 0x00, 0xFF, 0x80,
    ];

    pattern.repeat(4 * 24)
}
