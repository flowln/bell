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

pub fn create_128x128_image_data() -> Variant<Box<dyn RefArg>> {
    let image_data_container = ImageData {
        width: 128,
        height: 128,
        rowstride: 128 * 3 * std::mem::size_of::<u8>() as i32,
        has_alpha: false,
        bits_per_sample: 3 * std::mem::size_of::<u8>() as i32,
        channels: 3,
        data: get_128x128_opaque_image(),
    };

    let image_data_tuple = ImageDataTuple::from(image_data_container);

    Variant(Box::new(image_data_tuple))
}

#[rustfmt::skip]
fn get_128x128_opaque_image() -> Vec<u8> {
    // Red, Green, Blue, Black, White, Yellow, Cyan, Purple
    let pattern = vec![
        0xFF, 0x00, 0x00,
        0x00, 0xFF, 0x00,
        0x00, 0x00, 0xFF,
        0x00, 0x00, 0x00,
        0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0x00,
        0x00, 0xFF, 0xFF,
        0xFF, 0x00, 0xFF,
    ];

    let mut lines = Vec::new();
    for line in pattern.chunks_exact(3) {
        lines.extend(line.repeat(8 * 128));
    }

    lines.repeat(2)
}

pub fn create_240x240_image_data() -> Variant<Box<dyn RefArg>> {
    let image_data_container = ImageData {
        width: 240,
        height: 240,
        rowstride: 240 * 4 * std::mem::size_of::<u8>() as i32,
        has_alpha: true,
        bits_per_sample: 4 * std::mem::size_of::<u8>() as i32,
        channels: 4,
        data: get_240x240_translucent_image(),
    };

    let image_data_tuple = ImageDataTuple::from(image_data_container);

    Variant(Box::new(image_data_tuple))
}

#[rustfmt::skip]
fn get_240x240_translucent_image() -> Vec<u8> {
    // Red, Green, Blue, Black, Cyan, Purple, White
    // All with 80% transparency
    let pattern = vec![
        0xFF, 0x00, 0x00, 0xCC,
        0x00, 0xFF, 0x00, 0xCC,
        0x00, 0x00, 0xFF, 0xCC,
        0xFF, 0xFF, 0xFF, 0xCC,
        0x00, 0xFF, 0xFF, 0xCC,
        0xFF, 0x00, 0xFF, 0xCC,
        0x00, 0x00, 0x00, 0xCC,
    ];

    let mut ret = pattern.repeat(30 * 240);
    ret.resize(240 * 240 * 4, 0);
    ret
}
