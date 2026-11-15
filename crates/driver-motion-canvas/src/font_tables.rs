//! Bounded font metadata through maintained Fontations table readers.
//!
//! This adapter preserves the product's Unicode selection and glyph arithmetic.
//! It does not shape text, interpret outlines, install fonts, or grant permissions.
use read_fonts::{
    FontRead, FontRef, MinByteRange, TableProvider,
    ps::cff::CffFontRef,
    tables::cmap::{Cmap, Cmap2, Cmap4, CmapSubtable, EncodingRecord, SubHeader},
    types::{NameId, Tag},
};

pub(super) struct Axis {
    pub tag: Tag,
    pub min_value: f32,
    pub max_value: f32,
}

pub(super) struct Face<'a> {
    font: FontRef<'a>,
    glyph_count: u16,
    italic: bool,
    oblique: bool,
    outlines: bool,
    embedding: u16,
    weight: u16,
    names: Vec<String>,
    axes: Vec<Axis>,
}

impl<'a> Face<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let font = FontRef::new(bytes).ok()?;
        // Face metadata never admits extended MAXP/GLYF or font collections.
        let head = font.head().ok()?;
        font.hhea().ok()?;
        let maxp = font.maxp().ok()?;
        let maxp_version = font
            .data_for_tag(Tag::new(b"maxp"))?
            .read_at::<u32>(0)
            .ok()?;
        if !matches!(maxp_version, 0x0000_5000 | 0x0001_0000)
            || maxp.num_glyphs() == 0
            || !(16..=16_384).contains(&head.units_per_em())
            || !matches!(head.index_to_loc_format(), 0 | 1)
        {
            return None;
        }
        let glyph_count = maxp.num_glyphs();
        let os2 = font.os2().ok()?;
        let os2_bytes = match os2.version() {
            0 => 78,
            1 => 86,
            2..=4 => 96,
            5 => 100,
            _ => return None,
        };
        os2.offset_data().slice(..os2_bytes)?;
        let selection = os2.fs_selection().bits();
        let italic_angle = if font.data_for_tag(Tag::new(b"post")).is_some() {
            let post = font.post().ok()?;
            let version = font
                .data_for_tag(Tag::new(b"post"))?
                .read_at::<u32>(0)
                .ok()?;
            if !matches!(
                version,
                0x0001_0000 | 0x0002_0000 | 0x0002_5000 | 0x0003_0000 | 0x0004_0000
            ) {
                return None;
            }
            post.italic_angle().to_f32()
        } else {
            0.0
        };
        let outlines = valid_outlines(&font, glyph_count, head.units_per_em());
        let names = family_names(&font)?;
        let mut axes = Vec::new();
        if font.data_for_tag(Tag::new(b"fvar")).is_some() {
            let fvar = font.fvar().ok()?;
            let version = font
                .data_for_tag(Tag::new(b"fvar"))?
                .read_at::<u32>(0)
                .ok()?;
            if version != 0x0001_0000 || fvar.axis_count() > 1 || fvar.axis_size() != 20 {
                return None;
            }
            let records = fvar.axes().ok()?;
            // Fontations' lazy array getter otherwise returns an empty slice on truncation.
            if records.len() != usize::from(fvar.axis_count()) {
                return None;
            }
            for axis in records {
                // Preserve the prior normalization around the default coordinate.
                let default = axis.default_value().to_f32();
                axes.push(Axis {
                    tag: axis.axis_tag(),
                    min_value: default.min(axis.min_value().to_f32()),
                    max_value: default.max(axis.max_value().to_f32()),
                });
            }
        }
        Some(Self {
            glyph_count,
            italic: selection & 1 != 0 || italic_angle != 0.0,
            oblique: os2.version() >= 4 && selection & 0x0200 != 0,
            outlines,
            embedding: os2.fs_type(),
            weight: os2.us_weight_class(),
            names,
            axes,
            font,
        })
    }

    pub fn is_italic(&self) -> bool {
        self.italic
    }
    pub fn is_oblique(&self) -> bool {
        self.oblique
    }
    pub fn has_outlines(&self) -> bool {
        self.outlines
    }
    pub fn embedding_rights(&self) -> u16 {
        self.embedding
    }
    pub fn weight(&self) -> u16 {
        self.weight
    }
    pub fn names(&self) -> &[String] {
        &self.names
    }
    pub fn variation_axes(&self) -> &[Axis] {
        &self.axes
    }
    pub fn number_of_glyphs(&self) -> u16 {
        self.glyph_count
    }

    pub fn glyph_index(&self, character: char) -> Option<u16> {
        map_character(&self.font.cmap().ok()?, u32::from(character))
    }
}

fn valid_outlines(font: &FontRef<'_>, glyph_count: u16, upem: u16) -> bool {
    let glyf = font
        .glyf()
        .ok()
        .zip(font.loca(None).ok())
        .is_some_and(|(glyf, loca)| {
            // A truncated or reversed loca must not make arbitrary bytes an outline.
            loca.len() >= usize::from(glyph_count)
                && loca.all_offsets_are_ascending()
                && loca
                    .get_raw(usize::from(glyph_count))
                    .is_some_and(|end| end as usize <= glyf.offset_data().len())
        });
    let cff = font.data_for_tag(Tag::new(b"CFF ")).is_some_and(|data| {
        CffFontRef::new_cff(data.as_bytes(), 0, Some(i32::from(upem)))
            .is_ok_and(|font| font.num_glyphs() > 0)
    });
    let cff2 = font.data_for_tag(Tag::new(b"CFF2")).is_some_and(|data| {
        CffFontRef::new_cff2(data.as_bytes(), Some(i32::from(upem)))
            .is_ok_and(|font| font.num_glyphs() > 0)
    });
    glyf || cff || cff2
}

fn family_names(font: &FontRef<'_>) -> Option<Vec<String>> {
    let name = font.name().ok()?;
    if !matches!(name.version(), 0 | 1) || name.count() > 256 {
        return None;
    }
    let records = name
        .offset_data()
        .read_array::<read_fonts::tables::name::NameRecord>(name.name_record_byte_range())
        .ok()?;
    let strings = name.string_data();
    let mut names = Vec::new();
    for record in records {
        // An invalid record terminated the previous reader's iterator.
        if record.platform_id() > 4 {
            break;
        }
        let start = usize::try_from(record.string_offset().to_u32()).ok()?;
        let Some(end) = start.checked_add(usize::from(record.length())) else {
            break;
        };
        let Some(raw) = strings.as_bytes().get(start..end) else {
            break;
        };
        let unicode = record.platform_id() == 0
            || (record.platform_id() == 3 && matches!(record.encoding_id(), 0 | 1));
        if !unicode
            || raw.len() > 1024
            || !matches!(
                record.name_id(),
                NameId::FAMILY_NAME | NameId::TYPOGRAPHIC_FAMILY_NAME
            )
        {
            continue;
        }
        // Unlike NameString's Display, this never replaces an invalid surrogate.
        // Discarding an odd trailing byte preserves the prior reader's behavior.
        let units = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        if let Ok(value) = String::from_utf16(&units) {
            names.push(value);
        }
    }
    Some(names)
}

fn cmap2_headers<'a>(table: &Cmap2<'a>) -> Option<&'a [SubHeader]> {
    let keys = table.sub_header_keys();
    if keys.len() != 256 {
        return None;
    }
    let count = usize::from(keys.iter().map(|key| key.get() / 8).max()?) + 1;
    let start = table.sub_header_keys_byte_range().end;
    let end = start.checked_add(count.checked_mul(8)?)?;
    table.offset_data().read_array(start..end).ok()
}

fn map_cmap2(table: &Cmap2<'_>, headers: &[SubHeader], cp: u32) -> Option<u16> {
    let cp = u16::try_from(cp).ok()?;
    let low = cp & 0xff;
    let index = if cp < 0xff {
        0
    } else {
        usize::from(table.sub_header_keys().get(usize::from(cp >> 8))?.get() / 8)
    };
    let header = headers.get(index)?;
    let first = header.first_code();
    let end = first.checked_add(header.entry_count())?;
    if low < first || low >= end {
        return None;
    }
    let position = table
        .sub_header_keys_byte_range()
        .end
        .checked_add(index.checked_mul(8)?)?
        .checked_add(6)?
        .checked_add(usize::from(header.id_range_offset()))?
        .checked_add(usize::from(low - first).checked_mul(2)?)?;
    let raw = table.offset_data().read_at::<u16>(position).ok()?;
    if raw == 0 {
        return None;
    }
    u16::try_from((i32::from(raw) + i32::from(header.id_delta())) % 65_536).ok()
}

fn map_cmap4(table: &Cmap4<'_>, cp: u32) -> Option<u16> {
    let cp = u16::try_from(cp).ok()?;
    let starts = table.start_code();
    let ends = table.end_code();
    let mut lo = 0;
    let mut hi = starts.len();
    while lo < hi {
        let index = lo + (hi - lo) / 2;
        if ends.get(index)?.get() < cp {
            lo = index + 1;
        } else if starts.get(index)?.get() > cp {
            hi = index;
        } else {
            let delta = table.id_delta().get(index)?.get();
            let offset = table.id_range_offsets().get(index)?.get();
            if offset == 0 {
                return Some(cp.wrapping_add(delta as u16));
            }
            if offset == u16::MAX {
                return None;
            }
            let character_offset =
                u16::try_from(u32::from(cp - starts.get(index)?.get()).checked_mul(2)?).ok()?;
            // Reject malformed wrapping offsets instead of looking into a header.
            let position = table
                .id_range_offsets_byte_range()
                .start
                .checked_add(index.checked_mul(2)?)?
                .checked_add(usize::from(character_offset))?
                .checked_add(usize::from(offset))?;
            if position > usize::from(u16::MAX) {
                return None;
            }
            let raw = table.offset_data().read_at::<u16>(position).ok()?;
            if raw == 0 {
                return None;
            }
            return u16::try_from((raw as i16).wrapping_add(delta)).ok();
        }
    }
    None
}

fn map_character(cmap: &Cmap<'_>, cp: u32) -> Option<u16> {
    let data = cmap.offset_data();
    let range = cmap.encoding_records_byte_range();
    let records = data.read_array::<EncodingRecord>(range.clone()).ok()?;
    for (index, record) in records.iter().enumerate() {
        let record_start = range.start.checked_add(index.checked_mul(8)?)?;
        let platform = data.read_at::<u16>(record_start).ok()?;
        if platform > 4 {
            return None;
        }
        let offset =
            usize::try_from(data.read_at::<u32>(record_start.checked_add(4)?).ok()?).ok()?;
        let subdata = data.split_off(offset)?;
        let format = subdata.read_at::<u16>(0).ok()?;
        match format {
            8 => continue, // recognized previously, but no nominal glyph mapping
            0 | 2 | 4 | 6 | 10 | 12 | 13 | 14 => {}
            _ => return None, // invalid record ends selection, even for another platform
        }
        let length = match format {
            0 | 2 | 4 | 6 => usize::from(subdata.read_at::<u16>(2).ok()?),
            10 | 12 | 13 => usize::try_from(subdata.read_at::<u32>(4).ok()?).ok()?,
            14 => usize::try_from(subdata.read_at::<u32>(2).ok()?).ok()?,
            _ => return None,
        };
        // An adjacent subtable must not supply missing bytes for this record.
        let subdata = subdata.slice(..length)?;
        let subtable = CmapSubtable::read(subdata).ok()?;
        subdata.slice(subtable.min_byte_range())?;
        if let CmapSubtable::Format4(table) = &subtable
            && table.seg_count_x2() < 2
        {
            return None;
        }
        let headers = match &subtable {
            CmapSubtable::Format2(table) => Some(cmap2_headers(table)?),
            _ => None,
        };
        let unicode = platform == 0
            || (platform == 3
                && (record.encoding_id() == 1
                    || (record.encoding_id() == 10 && matches!(format, 12 | 13))));
        if !unicode {
            continue;
        }
        let glyph = match &subtable {
            CmapSubtable::Format0(table) => table
                .map_codepoint(cp)
                .and_then(|id| u16::try_from(id.to_u32()).ok())
                .filter(|id| *id != 0),
            CmapSubtable::Format2(table) => map_cmap2(table, headers?, cp),
            CmapSubtable::Format4(table) => map_cmap4(table, cp),
            CmapSubtable::Format6(table) if cp <= 0xffff => table
                .map_codepoint(cp)
                .and_then(|id| u16::try_from(id.to_u32()).ok()),
            CmapSubtable::Format10(table) => table
                .map_codepoint(cp)
                .and_then(|id| u16::try_from(id.to_u32()).ok()),
            CmapSubtable::Format12(table) => {
                let groups = table.groups();
                let found = groups
                    .binary_search_by(|group| {
                        if group.start_char_code() > cp {
                            std::cmp::Ordering::Greater
                        } else if group.end_char_code() < cp {
                            std::cmp::Ordering::Less
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .ok();
                found.and_then(|index| {
                    let group = groups.get(index)?;
                    let id = group
                        .start_glyph_id()
                        .checked_add(cp)?
                        .checked_sub(group.start_char_code())?;
                    u16::try_from(id).ok()
                })
            }
            CmapSubtable::Format13(table) => table
                .groups()
                .iter()
                .find(|group| group.start_char_code() <= cp && cp <= group.end_char_code())
                .and_then(|group| u16::try_from(group.glyph_id()).ok()),
            _ => None,
        };
        // Preserve the first Some, including zero; the caller rejects zero/range
        // without rescuing the character through a later subtable.
        if glyph.is_some() {
            return glyph;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }

    fn u32_at(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    fn cmap6(first: u16, glyphs: &[u16]) -> Vec<u8> {
        let length = 10 + glyphs.len() * 2;
        let mut bytes = vec![0; length];
        u16_at(&mut bytes, 0, 6);
        u16_at(&mut bytes, 2, length as u16);
        u16_at(&mut bytes, 6, first);
        u16_at(&mut bytes, 8, glyphs.len() as u16);
        for (index, glyph) in glyphs.iter().enumerate() {
            u16_at(&mut bytes, 10 + index * 2, *glyph);
        }
        bytes
    }

    fn cmap(records: &[(u16, u16, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0; 4 + records.len() * 8];
        u16_at(&mut bytes, 2, records.len() as u16);
        for (index, (platform, encoding, table)) in records.iter().enumerate() {
            let offset = bytes.len() as u32;
            u16_at(&mut bytes, 4 + index * 8, *platform);
            u16_at(&mut bytes, 6 + index * 8, *encoding);
            u32_at(&mut bytes, 8 + index * 8, offset);
            bytes.extend(table);
        }
        bytes
    }

    fn mapped(bytes: &[u8], point: u32) -> Option<u16> {
        map_character(&Cmap::read(bytes.into()).unwrap(), point)
    }

    #[test]
    fn first_unicode_mapping_including_zero_is_not_rescued() {
        let bytes = cmap(&[(3, 1, cmap6(65, &[0])), (0, 0, cmap6(65, &[2]))]);
        assert_eq!(mapped(&bytes, 65), Some(0));
        let bytes = cmap(&[(3, 1, cmap6(65, &[1])), (0, 0, cmap6(65, &[2]))]);
        assert_eq!(mapped(&bytes, 65), Some(1));
        assert_eq!(mapped(&bytes, 0x1_0041), None);
    }

    #[test]
    fn symbol_and_windows_full_repertoire_legacy_tables_remain_ineligible() {
        let bytes = cmap(&[(3, 0, cmap6(65, &[1])), (3, 10, cmap6(65, &[1]))]);
        assert_eq!(mapped(&bytes, 65), None);
        let bytes = cmap(&[
            (3, 0, cmap6(65, &[1])),
            (3, 10, cmap6(65, &[1])),
            (3, 1, cmap6(65, &[2])),
        ]);
        assert_eq!(mapped(&bytes, 65), Some(2));
    }

    #[test]
    fn malformed_or_unknown_first_record_stops_selection() {
        for (platform, mut table) in [(9, cmap6(65, &[1])), (1, cmap6(65, &[1]))] {
            if platform == 1 {
                u16_at(&mut table, 0, 15);
            }
            let bytes = cmap(&[(platform, 0, table), (3, 1, cmap6(65, &[2]))]);
            assert_eq!(mapped(&bytes, 65), None);
        }
        let mut truncated = cmap6(65, &[1]);
        u16_at(&mut truncated, 8, 2);
        let bytes = cmap(&[(1, 0, truncated), (3, 1, cmap6(65, &[2]))]);
        assert_eq!(mapped(&bytes, 65), None);
        let mut bytes = cmap(&[(3, 1, cmap6(65, &[1]))]);
        u32_at(&mut bytes, 8, u32::MAX);
        assert_eq!(mapped(&bytes, 65), None);
    }

    #[test]
    fn cmap2_preserves_one_byte_boundary_and_signed_delta() {
        let mut table = vec![0; 538];
        u16_at(&mut table, 0, 2);
        u16_at(&mut table, 2, 538);
        u16_at(&mut table, 6, 8); // key 0 chooses subheader 1 only at U+00FF
        u16_at(&mut table, 518, 0xfe);
        u16_at(&mut table, 520, 2);
        u16_at(&mut table, 524, 10);
        u16_at(&mut table, 526, 0xff);
        u16_at(&mut table, 528, 1);
        u16_at(&mut table, 532, 4);
        u16_at(&mut table, 534, 1);
        u16_at(&mut table, 536, 2);
        let bytes = cmap(&[(0, 0, table.clone())]);
        assert_eq!(mapped(&bytes, 0xfe), Some(1));
        assert_eq!(mapped(&bytes, 0xff), Some(2));
        u16_at(&mut table, 522, (-2i16) as u16);
        let bytes = cmap(&[(0, 0, table.clone())]);
        assert_eq!(mapped(&bytes, 0xfe), None);
        u16_at(&mut table, 534, 2);
        let bytes = cmap(&[(0, 0, table), (3, 1, cmap6(0xfe, &[3]))]);
        assert_eq!(mapped(&bytes, 0xfe), Some(0));
    }

    #[test]
    fn cmap4_does_not_turn_negative_range_result_into_a_large_glyph() {
        let mut table = vec![0; 26];
        u16_at(&mut table, 0, 4);
        u16_at(&mut table, 2, 26);
        u16_at(&mut table, 6, 2);
        u16_at(&mut table, 14, 65);
        u16_at(&mut table, 18, 65);
        u16_at(&mut table, 22, 2);
        u16_at(&mut table, 24, 0x8001);
        assert_eq!(mapped(&cmap(&[(0, 0, table.clone())]), 65), None);
        u16_at(&mut table, 24, 2);
        assert_eq!(mapped(&cmap(&[(0, 0, table.clone())]), 65), Some(2));
        u16_at(&mut table, 22, u16::MAX);
        assert_eq!(mapped(&cmap(&[(0, 0, table)]), 65), None);
    }

    #[test]
    fn cmap12_overflow_is_rejected_and_cmap13_preserves_first_group() {
        let mut table = vec![0; 28];
        u16_at(&mut table, 0, 12);
        u32_at(&mut table, 4, 28);
        u32_at(&mut table, 12, 1);
        u32_at(&mut table, 16, 65);
        u32_at(&mut table, 20, 65);
        u32_at(&mut table, 24, u32::MAX);
        assert_eq!(mapped(&cmap(&[(3, 10, table.clone())]), 65), None);
        u32_at(&mut table, 24, 1);
        assert_eq!(mapped(&cmap(&[(3, 10, table.clone())]), 65), Some(1));
        u16_at(&mut table, 0, 13);
        u32_at(&mut table, 20, 67);
        table.extend_from_slice(&[0; 12]);
        u32_at(&mut table, 4, 40);
        u32_at(&mut table, 12, 2);
        u32_at(&mut table, 28, 66);
        u32_at(&mut table, 32, 68);
        u32_at(&mut table, 36, 2);
        assert_eq!(mapped(&cmap(&[(3, 10, table)]), 66), Some(1));
    }

    // Inert table fixtures exercise parsing only; no outlines are executed.
    fn test_font(extra: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
        let mut head = vec![0; 54];
        u16_at(&mut head, 18, 1000);
        let mut maxp = vec![0; 6];
        u32_at(&mut maxp, 0, 0x0001_0000);
        u16_at(&mut maxp, 4, 3);
        let mut os2 = vec![0; 96];
        u16_at(&mut os2, 0, 4);
        u16_at(&mut os2, 4, 400);
        u16_at(&mut os2, 8, 8);
        let mut name = vec![0; 18];
        u16_at(&mut name, 2, 1);
        u16_at(&mut name, 4, 18);
        u16_at(&mut name, 6, 3);
        u16_at(&mut name, 8, 1);
        u16_at(&mut name, 12, 1);
        u16_at(&mut name, 14, 14);
        for unit in "Fixture".encode_utf16() {
            name.extend_from_slice(&unit.to_be_bytes());
        }
        let mut tables = std::collections::BTreeMap::from([
            (*b"head", head),
            (*b"hhea", vec![0; 36]),
            (*b"maxp", maxp),
            (*b"OS/2", os2),
            (*b"name", name),
            (*b"glyf", vec![]),
            (*b"loca", vec![0; 8]),
            (*b"cmap", cmap(&[(3, 1, cmap6(65, &[1]))])),
        ]);
        tables.extend(extra);
        let mut bytes = vec![0; 12 + tables.len() * 16];
        u32_at(&mut bytes, 0, 0x0001_0000);
        u16_at(&mut bytes, 4, tables.len() as u16);
        for (index, (tag, table)) in tables.into_iter().enumerate() {
            let record = 12 + index * 16;
            let offset = bytes.len() as u32;
            bytes[record..record + 4].copy_from_slice(&tag);
            u32_at(&mut bytes, record + 8, offset);
            u32_at(&mut bytes, record + 12, table.len() as u32);
            bytes.extend(table);
        }
        bytes
    }

    fn table_record(bytes: &[u8], tag: &[u8; 4]) -> (usize, usize) {
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let record = (0..count)
            .map(|index| 12 + index * 16)
            .find(|at| &bytes[*at..*at + 4] == tag)
            .unwrap();
        let offset =
            u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
        (record, offset)
    }

    #[test]
    fn required_versioned_tables_and_outline_ranges_are_not_defaulted() {
        let original = test_font(vec![]);
        let face = Face::parse(&original).unwrap();
        assert_eq!(face.names(), &["Fixture"]);
        assert_eq!(face.weight(), 400);
        assert_eq!(face.embedding_rights(), 8);
        assert_eq!(face.glyph_index('A'), Some(1));
        assert!(face.has_outlines());
        for (tag, short) in [(b"head", 53), (b"hhea", 35), (b"maxp", 5), (b"OS/2", 78)] {
            let mut bytes = original.clone();
            let (record, _) = table_record(&bytes, tag);
            u32_at(&mut bytes, record + 12, short);
            assert!(Face::parse(&bytes).is_none(), "{tag:?}");
        }
        let mut bytes = original.clone();
        let (record, _) = table_record(&bytes, b"loca");
        u32_at(&mut bytes, record + 12, 6);
        assert!(!Face::parse(&bytes).unwrap().has_outlines());
        let (_, offset) = table_record(&bytes, b"loca");
        u32_at(&mut bytes, record + 12, 8);
        u16_at(&mut bytes, offset + 6, 1);
        assert!(!Face::parse(&bytes).unwrap().has_outlines());
    }

    #[test]
    fn family_decode_stays_strict_and_does_not_admit_windows_encoding_10() {
        let original = test_font(vec![]);
        let (_, offset) = table_record(&original, b"name");
        let mut bytes = original.clone();
        u16_at(&mut bytes, offset + 8, 10);
        assert!(Face::parse(&bytes).unwrap().names().is_empty());
        let mut bytes = original;
        u16_at(&mut bytes, offset + 14, 2);
        u16_at(&mut bytes, offset + 18, 0xd800);
        assert!(Face::parse(&bytes).unwrap().names().is_empty());
    }

    #[test]
    fn post_italic_angle_and_os2_style_bits_are_preserved() {
        let mut post = vec![0; 32];
        u32_at(&mut post, 0, 0x0003_0000);
        u32_at(&mut post, 4, 0x0001_0000);
        let bytes = test_font(vec![(*b"post", post)]);
        assert!(Face::parse(&bytes).unwrap().is_italic());
        let mut bytes = test_font(vec![]);
        let (_, offset) = table_record(&bytes, b"OS/2");
        u16_at(&mut bytes, offset + 62, 0x0201);
        let face = Face::parse(&bytes).unwrap();
        assert!(face.is_italic());
        assert!(face.is_oblique());
    }

    #[test]
    fn weight_axis_is_bounded_and_cannot_disappear_when_truncated() {
        let mut fvar = vec![0; 36];
        u32_at(&mut fvar, 0, 0x0001_0000);
        u16_at(&mut fvar, 4, 16);
        u16_at(&mut fvar, 6, 2);
        u16_at(&mut fvar, 8, 1);
        u16_at(&mut fvar, 10, 20);
        u16_at(&mut fvar, 14, 8);
        fvar[16..20].copy_from_slice(b"wght");
        for (at, weight) in [(20, 100), (24, 400), (28, 900)] {
            u32_at(&mut fvar, at, weight << 16);
        }
        let mut bytes = test_font(vec![(*b"fvar", fvar)]);
        let face = Face::parse(&bytes).unwrap();
        assert_eq!(face.variation_axes().len(), 1);
        assert_eq!(face.variation_axes()[0].tag, Tag::new(b"wght"));
        assert_eq!(face.variation_axes()[0].min_value, 100.0);
        assert_eq!(face.variation_axes()[0].max_value, 900.0);
        let (record, _) = table_record(&bytes, b"fvar");
        u32_at(&mut bytes, record + 12, 35);
        assert!(Face::parse(&bytes).is_none());
    }
}
