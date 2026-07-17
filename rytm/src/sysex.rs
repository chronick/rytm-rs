//! Internal module for sysex related operations.

// All casts in this file are intended or safe within the context of this library.
//
// One can change `allow` to `warn` to review them if necessary.
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

pub mod types;

use crate::error::{RytmError, SysexConversionError};
use rytm_sys::{ar_global_t, ar_kit_t, ar_pattern_t, ar_settings_t, ar_sound_t};
use serde::{Deserialize, Serialize};
use std::ptr::addr_of_mut;
pub use types::*;

/// Pattern sysex response size for FW 1.70.
pub const PATTERN_SYSEX_SIZE: usize = 14988;
/// Kit sysex response size for FW 1.70.
pub const KIT_SYSEX_SIZE: usize = 2998;
/// Sound sysex response size for FW 1.70.
pub const SOUND_SYSEX_SIZE: usize = 201;
/// Settings sysex response size for FW 1.70.
pub const SETTINGS_SYSEX_SIZE: usize = 2401;
/// Global sysex response size for FW 1.70.
pub const GLOBAL_SYSEX_SIZE: usize = 107;
/// Song sysex response size for FW 1.70.
pub const SONG_SYSEX_SIZE: usize = 1506;

const SYSEX_MESSAGE_TYPE_BYTE_INDEX: usize = 6;

pub const PATTERN_RAW_SIZE: usize = std::mem::size_of::<ar_pattern_t>();
pub const KIT_RAW_SIZE: usize = std::mem::size_of::<ar_kit_t>();
pub const SOUND_RAW_SIZE: usize = std::mem::size_of::<ar_sound_t>();
pub const SETTINGS_RAW_SIZE: usize = std::mem::size_of::<ar_settings_t>();
pub const GLOBAL_RAW_SIZE: usize = std::mem::size_of::<ar_global_t>();

/// Meta type for sysex messages.
///
/// Can represent known and unknown sysex types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnySysexType {
    Known(SysexType),
    Unknown(u8),
}

impl From<SysexType> for AnySysexType {
    fn from(t: SysexType) -> Self {
        Self::Known(t)
    }
}

impl From<u8> for AnySysexType {
    fn from(t: u8) -> Self {
        if let Ok(t) = SysexType::try_from_dump_id(t) {
            return Self::from(t);
        }
        if let Ok(t) = SysexType::try_from(t) {
            return Self::from(t);
        }
        Self::Unknown(t)
    }
}

impl From<AnySysexType> for u8 {
    fn from(t: AnySysexType) -> Self {
        match t {
            AnySysexType::Known(t) => t.into(),
            AnySysexType::Unknown(t) => t,
        }
    }
}

/// A trait which is implemented by all objects which can be converted to sysex messages including queries and rytm project structures.
pub trait SysexCompatible {
    /// Returns the sysex type of the object.
    fn sysex_type(&self) -> AnySysexType;

    /// Serializes the object to a sysex message.
    ///
    /// # Errors
    ///
    /// May return a [`SysexConversionError`](crate::error::SysexConversionError) if the conversion fails.
    fn as_sysex(&self) -> Result<Vec<u8>, RytmError>;
}

/// A validated `SysEx` response that preserves its original bytes exactly.
///
/// This is useful for unsupported object models and firmware fixtures. Construction decodes the
/// envelope through `libanalogrytm`, validating the size, checksum, and metadata, while
/// serialization returns the original response without canonicalizing unknown fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawSysexObject {
    bytes: Vec<u8>,
    metadata: SysexMeta,
    sysex_type: SysexType,
}

impl RawSysexObject {
    /// Validates and preserves a complete Analog Rytm `SysEx` response.
    ///
    /// # Errors
    ///
    /// Returns a `SysEx` conversion error when the response is incomplete, has an unsupported type
    /// or size, or fails the codec checksum validation.
    pub fn from_sysex(bytes: &[u8]) -> Result<Self, RytmError> {
        validate_response_trailer(bytes)?;
        let (_, metadata) = decode_sysex_response_to_raw(bytes)?;
        let sysex_type = metadata.object_type()?;
        Ok(Self {
            bytes: bytes.to_vec(),
            metadata,
            sysex_type,
        })
    }

    /// Returns the validated response bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Consumes the object and returns the validated response bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Returns the unpacked object bytes produced by `libanalogrytm`.
    ///
    /// This is primarily useful for diagnosing typed codec round-trip differences without
    /// conflating them with the `SysEx` 7-bit packing and checksum trailer.
    ///
    /// # Errors
    ///
    /// Returns a `SysEx` conversion error if the preserved response can no longer be decoded.
    pub fn decoded_bytes(&self) -> Result<Vec<u8>, RytmError> {
        decode_sysex_response_to_raw(&self.bytes).map(|(bytes, _)| bytes)
    }

    /// Returns the metadata decoded from the response envelope.
    pub const fn metadata(&self) -> SysexMeta {
        self.metadata
    }
}

fn validate_response_trailer(bytes: &[u8]) -> Result<(), RytmError> {
    if bytes.len() < 15 {
        return Err(SysexConversionError::ShortRead.into());
    }
    if bytes[0] != 0xF0 || bytes[bytes.len() - 1] != 0xF7 {
        return Err(SysexConversionError::NotASysexMsg.into());
    }
    if bytes[1..bytes.len() - 1].iter().any(|byte| *byte >= 0x80) {
        return Err(SysexConversionError::NotASysexMsg.into());
    }

    let trailer = bytes.len() - 5;
    let expected_checksum = (u16::from(bytes[trailer]) << 7) | u16::from(bytes[trailer + 1]);
    let calculated_checksum = bytes[10..trailer]
        .iter()
        .fold(0_u16, |sum, byte| sum.wrapping_add(u16::from(*byte)))
        & 0x3FFF;
    if calculated_checksum != expected_checksum {
        return Err(SysexConversionError::Chksum.into());
    }

    let encoded_size = (usize::from(bytes[trailer + 2]) << 7) | usize::from(bytes[trailer + 3]);
    if encoded_size != bytes.len() - 10 {
        return Err(SysexConversionError::InvalidSize(bytes.len() - 10, encoded_size).into());
    }
    Ok(())
}

impl TryFrom<&[u8]> for RawSysexObject {
    type Error = RytmError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        Self::from_sysex(bytes)
    }
}

impl SysexCompatible for RawSysexObject {
    fn sysex_type(&self) -> AnySysexType {
        self.sysex_type.into()
    }

    fn as_sysex(&self) -> Result<Vec<u8>, RytmError> {
        Ok(self.bytes.clone())
    }
}

// Helper macro to implement the SysexCompatible trait for a given object.
#[macro_export]
macro_rules! impl_sysex_compatible {
    ($object_type:ty, $object_raw_type:ty, $object_encoder_function:ident, $object_sysex_type:expr, $object_sysex_size:expr) => {
        impl SysexCompatible for $object_type {
            fn as_sysex(&self) -> Result<Vec<u8>, RytmError> {
                let (sysex_meta, raw_object) = self.as_raw_parts();

                let raw_size = std::mem::size_of::<$object_raw_type>();
                let mut raw_buffer: Vec<u8> = Vec::with_capacity(raw_size);

                unsafe {
                    let raw: *const u8 =
                        std::ptr::from_ref::<$object_raw_type>(&raw_object).cast::<u8>();
                    for i in 0..raw_size {
                        raw_buffer.push(*raw.add(i));
                    }
                }

                let mut encoded_buffer_length: u32 = 0;
                let mut encoded_buf = vec![0; $object_sysex_size];

                let mut meta = sysex_meta.into();
                let meta_ptr = &mut meta as *mut ar_sysex_meta_t;

                unsafe {
                    #[allow(clippy::cast_possible_truncation)]
                    let return_code = $object_encoder_function(
                        encoded_buf.as_mut_ptr(),
                        raw_buffer.as_ptr(),
                        // This cast is fine since the size of the object can't be bigger than u32::MAX.
                        std::mem::size_of::<$object_raw_type>() as u32,
                        std::ptr::from_mut::<u32>(&mut encoded_buffer_length),
                        meta_ptr,
                    );

                    if return_code != 0 {
                        // libanalogrytm return codes are always below 255, cast is fine.
                        #[allow(clippy::cast_possible_truncation)]
                        return Err(SysexConversionError::from(return_code as u8).into());
                    }

                    let _chksum = (u16::from(encoded_buf[encoded_buf.len() - 5]) << 8)
                        | u16::from(encoded_buf[encoded_buf.len() - 4]);
                    let _size = (u16::from(encoded_buf[encoded_buf.len() - 3]) << 8)
                        | u16::from(encoded_buf[encoded_buf.len() - 2]);

                    Ok(encoded_buf)
                }
            }

            fn sysex_type(&self) -> AnySysexType {
                $object_sysex_type.into()
            }
        }
    };
}

/// This function assumes that the response is a valid sysex response.
///
/// It should be used in a context where this case is true and validity check is not necessary.
pub fn decode_sysex_response_to_raw(response: &[u8]) -> Result<(Vec<u8>, SysexMeta), RytmError> {
    if response.get(SYSEX_MESSAGE_TYPE_BYTE_INDEX).is_none() {
        // Message is too short, rytm sometimes sends incomplete sysex messages especially in the initial parts of the transmission.
        // One can check for this error and ignore it.
        return Err(SysexConversionError::ShortRead.into());
    }
    let response_type = SysexType::try_from_dump_id(response[SYSEX_MESSAGE_TYPE_BYTE_INDEX])?;
    let (expected_response_size, expected_raw_size) = match response_type {
        SysexType::Pattern => (PATTERN_SYSEX_SIZE, PATTERN_RAW_SIZE),
        SysexType::Kit => (KIT_SYSEX_SIZE, KIT_RAW_SIZE),
        SysexType::Sound => (SOUND_SYSEX_SIZE, SOUND_RAW_SIZE),
        SysexType::Settings => (SETTINGS_SYSEX_SIZE, SETTINGS_RAW_SIZE),
        SysexType::Global => (GLOBAL_SYSEX_SIZE, GLOBAL_RAW_SIZE),
        // Song raw size is guessed for now.
        SysexType::Song => (SONG_SYSEX_SIZE, 1024 * 16),
    };

    // Check for completeness.
    if response.len() != expected_response_size {
        return Err(
            SysexConversionError::InvalidSize(expected_response_size, response.len()).into(),
        );
    }

    // Make a default meta struct to fill.
    let meta = SysexMeta::default();
    let mut meta: rytm_sys::ar_sysex_meta_t = meta.into();

    // The response buffer.
    let mut src_buf = response.as_ptr();

    // u32 is big enough for any possible buffer in this context.
    #[allow(clippy::cast_possible_truncation)]
    let mut src_buf_size = response.len() as u32;

    // Will be calculated by the first call to ar_sysex_to_raw.
    let mut dst_buf_size = 0u32;

    // The destination buffer, raw buffer.
    let mut dst_buf = vec![0_u8; expected_raw_size];

    unsafe {
        // The count of return error codes from `rytm-sys` is far below 255.
        #[allow(clippy::cast_possible_truncation)]
        let return_code = rytm_sys::ar_sysex_to_raw(
            dst_buf.as_mut_slice().as_mut_ptr(),
            addr_of_mut!(src_buf),
            addr_of_mut!(src_buf_size),
            addr_of_mut!(dst_buf_size),
            addr_of_mut!(meta),
        ) as u8;

        if return_code != 0 {
            return Err(SysexConversionError::from(return_code).into());
        }
    }

    Ok((dst_buf, SysexMeta::from(&meta)))
}
