//! Lossless typed access to Analog Rytm Song objects.

use crate::{
    error::{ParameterError, RytmError, SysexConversionError},
    object::types::ObjectName,
    sysex::{
        decode_sysex_response_to_raw, encode_raw_to_sysex, AnySysexType, SysexCompatible,
        SysexMeta, SysexType, SONG_RAW_SIZE,
    },
};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

const NAME_OFFSET: usize = 4;
const NAME_LENGTH: usize = 15;
const HEADER_SIZE: usize = 0x18;
const ROW_TABLE_OFFSET: usize = HEADER_SIZE;
const ROW_RECORD_SIZE: usize = 4;
const PATTERN_TABLE_OFFSET: usize = 0x118;
const PATTERN_RECORD_SIZE: usize = 4;

/// Maximum number of Song rows represented by the object.
pub const SONG_ROW_CAPACITY: usize = 64;
/// Maximum number of pattern positions shared by all Song rows.
pub const SONG_PATTERN_CAPACITY: usize = 256;
/// Number of drum-track mute bits stored per pattern position.
pub const SONG_TRACK_COUNT: usize = 12;

/// Explicit support evidence for the typed Song model.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SongCapabilities {
    pub name: bool,
    pub rows: bool,
    pub pattern_chains: bool,
    pub repeats: bool,
    pub track_mutes: bool,
    pub tempo_overrides: bool,
    pub pattern_length_overrides: bool,
    pub jumps: bool,
    pub loops: bool,
    pub row_labels: bool,
    pub explicit_end: bool,
}

impl SongCapabilities {
    /// Returns capabilities backed by controlled fixtures or the OS 1.72 Song UI.
    pub const fn connected_mkii() -> Self {
        Self {
            name: true,
            rows: true,
            pattern_chains: true,
            repeats: true,
            track_mutes: true,
            tempo_overrides: false,
            pattern_length_overrides: false,
            jumps: false,
            loops: false,
            row_labels: false,
            explicit_end: false,
        }
    }
}

/// One pattern position in a Song row or chain.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SongPattern {
    pattern: u8,
    mute_word: u16,
    unknown_flags: u8,
}

impl SongPattern {
    /// Creates a pattern position with no song mutes and zeroed unknown flags.
    ///
    /// # Errors
    ///
    /// Pattern indices must be in `0..=127`.
    pub fn try_new(pattern: usize) -> Result<Self, RytmError> {
        validate_range(pattern, "song pattern", 0, 127)?;
        Ok(Self {
            pattern: pattern as u8,
            mute_word: 0,
            unknown_flags: 0,
        })
    }

    /// Returns the zero-based Pattern index (`0` is A01, `127` is H16).
    pub const fn pattern(&self) -> usize {
        self.pattern as usize
    }

    /// Sets the zero-based Pattern index.
    ///
    /// # Errors
    ///
    /// Pattern indices must be in `0..=127`.
    pub fn set_pattern(&mut self, pattern: usize) -> Result<(), RytmError> {
        validate_range(pattern, "song pattern", 0, 127)?;
        self.pattern = pattern as u8;
        Ok(())
    }

    /// Returns the 12-bit mask where bit zero represents BD and bit eleven represents CB.
    pub const fn muted_tracks_mask(&self) -> u16 {
        self.mute_word & 0x0FFF
    }

    /// Replaces the 12-bit song-mute mask while preserving unidentified upper bits.
    ///
    /// # Errors
    ///
    /// Bits above the twelve Rytm tracks must be clear.
    pub fn set_muted_tracks_mask(&mut self, muted_tracks: u16) -> Result<(), RytmError> {
        if muted_tracks > 0x0FFF {
            return Err(ParameterError::Range {
                value: muted_tracks.to_string(),
                parameter_name: "song muted tracks".to_owned(),
            }
            .into());
        }
        self.mute_word = (self.mute_word & 0xF000) | muted_tracks;
        Ok(())
    }

    /// Returns whether the zero-based drum track is song-muted at this position.
    ///
    /// # Errors
    ///
    /// Track indices must be in `0..=11`.
    pub fn is_track_muted(&self, track: usize) -> Result<bool, RytmError> {
        validate_range(track, "song mute track", 0, SONG_TRACK_COUNT - 1)?;
        Ok(self.mute_word & (1 << track) != 0)
    }

    /// Changes one song-mute bit without affecting other tracks or unidentified upper bits.
    ///
    /// # Errors
    ///
    /// Track indices must be in `0..=11`.
    pub fn set_track_muted(&mut self, track: usize, muted: bool) -> Result<(), RytmError> {
        validate_range(track, "song mute track", 0, SONG_TRACK_COUNT - 1)?;
        if muted {
            self.mute_word |= 1 << track;
        } else {
            self.mute_word &= !(1 << track);
        }
        Ok(())
    }

    /// Returns unidentified upper bits in the two-byte mute field.
    pub const fn unknown_mute_bits(&self) -> u16 {
        self.mute_word & 0xF000
    }

    /// Returns the currently unidentified third pattern-record byte.
    pub const fn unknown_flags(&self) -> u8 {
        self.unknown_flags
    }
}

/// One Song row, including its pattern chain and repeat count.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SongRow {
    patterns: Vec<SongPattern>,
    repeats: u16,
    unknown_prefix: u8,
    unknown_flags: u8,
}

impl SongRow {
    /// Creates a row from one or more pattern positions.
    ///
    /// # Errors
    ///
    /// A row must contain `1..=255` pattern positions and repeat `1..=256` times.
    pub fn try_new(patterns: Vec<SongPattern>, repeats: usize) -> Result<Self, RytmError> {
        validate_patterns(&patterns)?;
        validate_range(repeats, "song row repeats", 1, 256)?;
        Ok(Self {
            patterns,
            repeats: repeats as u16,
            unknown_prefix: 0,
            unknown_flags: 0,
        })
    }

    /// Returns the pattern chain in playback order.
    pub fn patterns(&self) -> &[SongPattern] {
        &self.patterns
    }

    /// Returns the pattern chain mutably.
    pub fn patterns_mut(&mut self) -> &mut [SongPattern] {
        &mut self.patterns
    }

    /// Replaces the row's pattern chain.
    ///
    /// # Errors
    ///
    /// A row must contain `1..=255` pattern positions.
    pub fn set_patterns(&mut self, patterns: Vec<SongPattern>) -> Result<(), RytmError> {
        validate_patterns(&patterns)?;
        self.patterns = patterns;
        Ok(())
    }

    /// Returns how many times the complete row is played.
    pub const fn repeats(&self) -> usize {
        self.repeats as usize
    }

    /// Sets how many times the complete row is played.
    ///
    /// # Errors
    ///
    /// Repeat counts must be in `1..=256`.
    pub fn set_repeats(&mut self, repeats: usize) -> Result<(), RytmError> {
        validate_range(repeats, "song row repeats", 1, 256)?;
        self.repeats = repeats as u16;
        Ok(())
    }

    /// Returns the unidentified first row-record byte.
    pub const fn unknown_prefix(&self) -> u8 {
        self.unknown_prefix
    }

    /// Returns the unidentified third row-record byte.
    pub const fn unknown_flags(&self) -> u8 {
        self.unknown_flags
    }
}

/// A complete stored or work-buffer Song with lossless preservation of unidentified bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Song {
    sysex_meta: SysexMeta,
    #[serde(with = "BigArray")]
    raw: [u8; SONG_RAW_SIZE],
}

impl Song {
    /// Creates an empty stored Song.
    ///
    /// # Errors
    ///
    /// Song indices must be in `0..=15`.
    pub fn try_default(index: usize) -> Result<Self, RytmError> {
        Self::try_default_with_device_id(index, 0)
    }

    /// Creates an empty stored Song with an explicit device ID.
    ///
    /// # Errors
    ///
    /// Song indices must be in `0..=15`.
    pub fn try_default_with_device_id(index: usize, device_id: u8) -> Result<Self, RytmError> {
        Ok(Self::empty(SysexMeta::try_default_for_song(
            index,
            Some(device_id),
        )?))
    }

    /// Creates an empty work-buffer Song.
    pub fn work_buffer_default_with_device_id(device_id: u8) -> Self {
        Self::empty(SysexMeta::default_for_song_in_work_buffer(Some(device_id)))
    }

    fn empty(sysex_meta: SysexMeta) -> Self {
        let mut raw = [0; SONG_RAW_SIZE];
        raw[0x15] = 1;
        Self { sysex_meta, raw }
    }

    /// Decodes a complete Song SysEx response.
    ///
    /// # Errors
    ///
    /// The response must be a valid 1,506-byte Analog Rytm Song dump.
    pub fn from_sysex(response: &[u8]) -> Result<Self, RytmError> {
        let (raw, meta) = decode_sysex_response_to_raw(response)?;
        Self::try_from_raw(meta, &raw)
    }

    pub(crate) fn try_from_raw(meta: SysexMeta, raw: &[u8]) -> Result<Self, RytmError> {
        if meta.object_type()? != SysexType::Song {
            return Err(SysexConversionError::InvalidObjType.into());
        }
        let raw: [u8; SONG_RAW_SIZE] = raw
            .try_into()
            .map_err(|_| SysexConversionError::InvalidSize(SONG_RAW_SIZE, raw.len()))?;
        let song = Self {
            sysex_meta: meta,
            raw,
        };
        song.rows()?;
        Ok(song)
    }

    /// Returns support evidence for optional Song fields.
    pub const fn capabilities(&self) -> SongCapabilities {
        SongCapabilities::connected_mkii()
    }

    /// Returns the stored slot or normalized work-buffer Song index.
    pub const fn index(&self) -> usize {
        self.sysex_meta.get_normalized_object_index()
    }

    /// Returns whether this object targets the work buffer.
    pub const fn is_work_buffer(&self) -> bool {
        self.sysex_meta.is_targeting_work_buffer()
    }

    /// Returns the name shown by the Song menu.
    pub fn name(&self) -> &str {
        std::str::from_utf8(&self.raw[NAME_OFFSET..NAME_OFFSET + NAME_LENGTH])
            .unwrap_or("")
            .trim_end_matches(char::from(0))
            .trim_end()
    }

    /// Sets the Song name while preserving every unrelated header byte.
    ///
    /// # Errors
    ///
    /// Names must be ASCII and at most 15 characters.
    pub fn set_name(&mut self, name: &str) -> Result<(), RytmError> {
        let name: ObjectName = name.try_into()?;
        self.raw[NAME_OFFSET..NAME_OFFSET + NAME_LENGTH].copy_from_slice(&name.copy_inner());
        Ok(())
    }

    /// Decodes active rows and their pattern positions.
    ///
    /// # Errors
    ///
    /// Returns an error when row counts reference more than 256 pattern positions.
    pub fn rows(&self) -> Result<Vec<SongRow>, RytmError> {
        let mut rows = Vec::new();
        let mut pattern_cursor = 0_usize;
        for row_index in 0..SONG_ROW_CAPACITY {
            let row_offset = ROW_TABLE_OFFSET + row_index * ROW_RECORD_SIZE;
            let pattern_count = usize::from(self.raw[row_offset + 1]);
            if pattern_count == 0 {
                break;
            }
            if pattern_cursor + pattern_count > SONG_PATTERN_CAPACITY {
                return Err(ParameterError::Compatibility {
                    value: (pattern_cursor + pattern_count).to_string(),
                    parameter_name: "song pattern positions".to_owned(),
                    reason: Some("row counts exceed the 256-position Song pool".to_owned()),
                }
                .into());
            }
            let mut patterns = Vec::with_capacity(pattern_count);
            for pattern_index in pattern_cursor..pattern_cursor + pattern_count {
                let offset = PATTERN_TABLE_OFFSET + pattern_index * PATTERN_RECORD_SIZE;
                let pattern = self.raw[offset + 3];
                validate_range(usize::from(pattern), "song pattern", 0, 127)?;
                patterns.push(SongPattern {
                    mute_word: u16::from_be_bytes([self.raw[offset], self.raw[offset + 1]]),
                    unknown_flags: self.raw[offset + 2],
                    pattern,
                });
            }
            rows.push(SongRow {
                patterns,
                repeats: u16::from(self.raw[row_offset + 3]) + 1,
                unknown_prefix: self.raw[row_offset],
                unknown_flags: self.raw[row_offset + 2],
            });
            pattern_cursor += pattern_count;
        }
        Ok(rows)
    }

    /// Replaces all active rows while preserving unidentified header and inactive-record bytes.
    ///
    /// # Errors
    ///
    /// At most 64 rows and 256 total pattern positions are accepted.
    pub fn replace_rows(&mut self, rows: &[SongRow]) -> Result<(), RytmError> {
        validate_range(rows.len(), "song row count", 0, SONG_ROW_CAPACITY)?;
        let pattern_count = rows.iter().map(|row| row.patterns.len()).sum::<usize>();
        validate_range(
            pattern_count,
            "song pattern position count",
            0,
            SONG_PATTERN_CAPACITY,
        )?;
        for row in rows {
            validate_patterns(&row.patterns)?;
            validate_range(row.repeats(), "song row repeats", 1, 256)?;
        }

        for row_index in 0..SONG_ROW_CAPACITY {
            let offset = ROW_TABLE_OFFSET + row_index * ROW_RECORD_SIZE;
            self.raw[offset + 1] = 0;
            self.raw[offset + 3] = 0;
        }

        let mut pattern_cursor = 0_usize;
        for (row_index, row) in rows.iter().enumerate() {
            let offset = ROW_TABLE_OFFSET + row_index * ROW_RECORD_SIZE;
            self.raw[offset] = row.unknown_prefix;
            self.raw[offset + 1] = row.patterns.len() as u8;
            self.raw[offset + 2] = row.unknown_flags;
            self.raw[offset + 3] = (row.repeats - 1) as u8;
            for pattern in &row.patterns {
                let offset = PATTERN_TABLE_OFFSET + pattern_cursor * PATTERN_RECORD_SIZE;
                let [mute_msb, mute_lsb] = pattern.mute_word.to_be_bytes();
                self.raw[offset] = mute_msb;
                self.raw[offset + 1] = mute_lsb;
                self.raw[offset + 2] = pattern.unknown_flags;
                self.raw[offset + 3] = pattern.pattern;
                pattern_cursor += 1;
            }
        }
        Ok(())
    }

    /// Removes all active rows while preserving unidentified bytes.
    pub fn clear(&mut self) {
        for row_index in 0..SONG_ROW_CAPACITY {
            let offset = ROW_TABLE_OFFSET + row_index * ROW_RECORD_SIZE;
            self.raw[offset + 1] = 0;
            self.raw[offset + 3] = 0;
        }
    }

    /// Returns the exact decoded 1,304-byte object.
    pub const fn raw_bytes(&self) -> &[u8; SONG_RAW_SIZE] {
        &self.raw
    }

    /// Returns the 24-byte header containing the version, name, and unidentified state bytes.
    pub fn raw_header(&self) -> &[u8] {
        &self.raw[..HEADER_SIZE]
    }

    /// Changes the target device ID.
    pub fn set_device_id(&mut self, device_id: u8) {
        self.sysex_meta.set_device_id(device_id);
    }
}

impl SysexCompatible for Song {
    fn sysex_type(&self) -> AnySysexType {
        SysexType::Song.into()
    }

    fn as_sysex(&self) -> Result<Vec<u8>, RytmError> {
        encode_raw_to_sysex(&self.raw, self.sysex_meta)
    }
}

fn validate_patterns(patterns: &[SongPattern]) -> Result<(), RytmError> {
    validate_range(patterns.len(), "song row pattern count", 1, 255)
}

fn validate_range(
    value: usize,
    parameter_name: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), RytmError> {
    if value < minimum || value > maximum {
        return Err(ParameterError::Range {
            value: value.to_string(),
            parameter_name: parameter_name.to_owned(),
        }
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_empty_fixture_is_typed_and_byte_stable() {
        let bytes =
            include_bytes!("../../tests/fixtures/mkii-connected-2026-07-17/song-work-buffer.syx");
        let song = Song::from_sysex(bytes).unwrap();
        assert!(song.is_work_buffer());
        assert!(song.name().is_empty());
        assert!(song.rows().unwrap().is_empty());
        assert_eq!(song.raw_bytes().len(), SONG_RAW_SIZE);
        assert_eq!(song.as_sysex().unwrap(), bytes);
    }

    #[test]
    fn rows_repeats_patterns_and_mutes_round_trip() {
        let mut first = SongPattern::try_new(0).unwrap();
        first.set_track_muted(0, true).unwrap();
        first.set_track_muted(11, true).unwrap();
        let second = SongPattern::try_new(17).unwrap();
        let rows = vec![
            SongRow::try_new(vec![first, second], 3).unwrap(),
            SongRow::try_new(vec![SongPattern::try_new(127).unwrap()], 1).unwrap(),
        ];
        let mut song = Song::try_default(2).unwrap();
        song.set_name("AGENT SONG").unwrap();
        song.replace_rows(&rows).unwrap();

        let encoded = song.as_sysex().unwrap();
        let decoded = Song::from_sysex(&encoded).unwrap();
        assert_eq!(decoded.index(), 2);
        assert_eq!(decoded.name(), "AGENT SONG");
        assert_eq!(decoded.rows().unwrap(), rows);
        assert!(decoded.rows().unwrap()[0].patterns()[0]
            .is_track_muted(0)
            .unwrap());
        assert!(decoded.rows().unwrap()[0].patterns()[0]
            .is_track_muted(11)
            .unwrap());

        let mut project = crate::RytmProject::try_default().unwrap();
        project.update_from_sysex_response(&encoded).unwrap();
        assert_eq!(project.songs().len(), 16);
        assert_eq!(project.songs()[2].rows().unwrap(), rows);
    }

    #[test]
    fn semantic_mutations_preserve_unidentified_regions() {
        let bytes =
            include_bytes!("../../tests/fixtures/mkii-connected-2026-07-17/song-work-buffer.syx");
        let mut song = Song::from_sysex(bytes).unwrap();
        let baseline = *song.raw_bytes();
        song.set_name("TEST").unwrap();
        song.replace_rows(&[SongRow::try_new(vec![SongPattern::try_new(4).unwrap()], 2).unwrap()])
            .unwrap();

        let allowed = (NAME_OFFSET..NAME_OFFSET + NAME_LENGTH)
            .chain([ROW_TABLE_OFFSET + 1, ROW_TABLE_OFFSET + 3])
            .chain(PATTERN_TABLE_OFFSET..PATTERN_TABLE_OFFSET + PATTERN_RECORD_SIZE)
            .collect::<std::collections::BTreeSet<_>>();
        for (offset, (before, after)) in baseline.iter().zip(song.raw_bytes()).enumerate() {
            if before != after {
                assert!(
                    allowed.contains(&offset),
                    "unexpected mutation at {offset:#06x}"
                );
            }
        }
    }

    #[test]
    fn unsupported_arranger_fields_are_explicit() {
        let capabilities = Song::try_default(0).unwrap().capabilities();
        assert!(capabilities.rows);
        assert!(capabilities.repeats);
        assert!(capabilities.track_mutes);
        assert!(!capabilities.tempo_overrides);
        assert!(!capabilities.jumps);
        assert!(!capabilities.loops);
        assert!(!capabilities.explicit_end);
    }

    #[test]
    fn capacities_and_ranges_are_enforced() {
        assert!(SongPattern::try_new(128).is_err());
        assert!(SongRow::try_new(Vec::new(), 1).is_err());
        assert!(SongRow::try_new(vec![SongPattern::try_new(0).unwrap()], 257).is_err());
        let row = SongRow::try_new(vec![SongPattern::try_new(0).unwrap(); 255], 1).unwrap();
        let mut song = Song::try_default(0).unwrap();
        assert!(song.replace_rows(&[row.clone(), row]).is_err());

        let mut invalid_raw = Song::try_default(0).unwrap();
        invalid_raw.raw[ROW_TABLE_OFFSET + 1] = 1;
        invalid_raw.raw[PATTERN_TABLE_OFFSET + 3] = 128;
        assert!(invalid_raw.rows().is_err());
    }
}
