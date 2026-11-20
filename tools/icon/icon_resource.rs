use std::fs;
use std::io;
use std::path::Path;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const GROUP_NAME: u16 = 1;
const MOVEABLE_PURE_DISCARDABLE: u16 = 0x1030;
const MOVEABLE_PURE: u16 = 0x0030;
const LANGUAGE_NEUTRAL: u16 = 0;
const ICONDIR_SIZE: usize = 6;
const ICONDIRENTRY_SIZE: usize = 16;
const ICON_TYPE: u16 = 1;

struct IconImage<'a> {
    width: u8,
    height: u8,
    colors: u8,
    planes: u16,
    bit_count: u16,
    bytes: &'a [u8],
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

fn u16_at(bytes: &[u8], at: usize) -> io::Result<u16> {
    bytes
        .get(at..at + 2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .ok_or_else(|| invalid("the icon file ends inside a header"))
}

fn u32_at(bytes: &[u8], at: usize) -> io::Result<u32> {
    bytes
        .get(at..at + 4)
        .map(|quad| u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
        .ok_or_else(|| invalid("the icon file ends inside a header"))
}

fn icon_images(ico: &[u8]) -> io::Result<Vec<IconImage<'_>>> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != ICON_TYPE {
        return Err(invalid("not an .ico file"));
    }
    let count = usize::from(u16_at(ico, 4)?);
    if count == 0 {
        return Err(invalid("the icon file holds no images"));
    }
    (0..count)
        .map(|index| {
            let entry = ICONDIR_SIZE + index * ICONDIRENTRY_SIZE;
            let length = u32_at(ico, entry + 8)? as usize;
            let offset = u32_at(ico, entry + 12)? as usize;
            let bytes = ico
                .get(offset..offset + length)
                .ok_or_else(|| invalid("an icon image lies outside the file"))?;
            Ok(IconImage {
                width: ico[entry],
                height: ico[entry + 1],
                colors: ico[entry + 2],
                planes: u16_at(ico, entry + 4)?,
                bit_count: u16_at(ico, entry + 6)?,
                bytes,
            })
        })
        .collect()
}

fn push_resource(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&name.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&LANGUAGE_NEUTRAL.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

fn group_directory(images: &[IconImage<'_>]) -> Vec<u8> {
    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&ICON_TYPE.to_le_bytes());
    group.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        group.extend_from_slice(&[image.width, image.height, image.colors, 0]);
        group.extend_from_slice(&image.planes.to_le_bytes());
        group.extend_from_slice(&image.bit_count.to_le_bytes());
        group.extend_from_slice(&(image.bytes.len() as u32).to_le_bytes());
        group.extend_from_slice(&(index as u16 + 1).to_le_bytes());
    }
    group
}

pub fn resource_file(ico: &[u8]) -> io::Result<Vec<u8>> {
    let images = icon_images(ico)?;
    let mut out = Vec::new();
    push_resource(&mut out, 0, 0, 0, &[]);
    for (index, image) in images.iter().enumerate() {
        push_resource(
            &mut out,
            RT_ICON,
            index as u16 + 1,
            MOVEABLE_PURE_DISCARDABLE,
            image.bytes,
        );
    }
    push_resource(
        &mut out,
        RT_GROUP_ICON,
        GROUP_NAME,
        MOVEABLE_PURE,
        &group_directory(&images),
    );
    Ok(out)
}

pub fn write_resource_file(ico: &Path, res: &Path) -> io::Result<()> {
    fs::write(res, resource_file(&fs::read(ico)?)?)
}
