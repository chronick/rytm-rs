use crate::error::{ParameterError, RytmError};
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

pub const KIT_MACRO_COUNT: usize = 12;
pub const KIT_MACRO_LOCK_CAPACITY: usize = 48;
const RECORD_SIZE: usize = 4;
const RAW_SIZE: usize = KIT_MACRO_LOCK_CAPACITY * RECORD_SIZE;
const EMPTY_RECORD: [u8; RECORD_SIZE] = [0xFF, 0xFF, 0x00, 0xFF];

/// A track that can be targeted by a Scene or Performance lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MacroTrack {
    /// One of the 12 voice tracks, indexed from zero.
    Voice(u8),
    /// The Kit FX track.
    Fx,
}

impl MacroTrack {
    /// Creates a voice-track target.
    ///
    /// # Errors
    ///
    /// Returns an error when `index` is outside `0..=11`.
    pub fn try_voice(index: usize) -> Result<Self, RytmError> {
        if index >= 12 {
            return Err(range_error(index, "macro voice track"));
        }
        Ok(Self::Voice(index as u8))
    }

    /// Returns the zero-based voice-track index, or 12 for FX.
    pub const fn raw_id(self) -> u8 {
        match self {
            Self::Voice(index) => index,
            Self::Fx => 12,
        }
    }

    fn validate(self) -> Result<(), RytmError> {
        if matches!(self, Self::Voice(index) if index >= 12) {
            return Err(range_error(self.raw_id(), "macro voice track"));
        }
        Ok(())
    }

    fn from_raw(value: u8) -> Option<Self> {
        match value {
            0..=11 => Some(Self::Voice(value)),
            12 => Some(Self::Fx),
            _ => None,
        }
    }
}

/// Logical parameter pages used by Scene and Performance locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MacroParameterPage {
    Machine,
    Sample,
    Filter,
    Amp,
    Lfo,
    Delay,
    Reverb,
    Distortion,
    Compressor,
    FxLfo,
}

/// A validated parameter ID in the voice or FX p-lock namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MacroParameter {
    page: MacroParameterPage,
    raw_id: u8,
}

impl MacroParameter {
    /// Creates a parameter after validating it against the target track family.
    ///
    /// # Errors
    ///
    /// Returns an error for reserved IDs or voice/FX namespace mismatches.
    pub fn try_from_raw(track: MacroTrack, raw_id: u8) -> Result<Self, RytmError> {
        track.validate()?;
        parameter_from_raw(track, raw_id).ok_or_else(|| {
            RytmError::Parameter(ParameterError::Compatibility {
                value: raw_id.to_string(),
                parameter_name: "macro parameter".to_string(),
                reason: Some(format!("parameter is not available for {track:?}")),
            })
        })
    }

    /// Returns the p-lock namespace ID stored by the device.
    pub const fn raw_id(self) -> u8 {
        self.raw_id
    }

    /// Returns the logical parameter page.
    pub const fn page(self) -> MacroParameterPage {
        self.page
    }

    /// Returns a stable semantic name for compact inspection and declarative tools.
    pub const fn name(self) -> &'static str {
        parameter_name(self.page, self.raw_id)
    }
}

/// A fixed-value lock in a Scene definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SceneLock {
    track: MacroTrack,
    parameter: MacroParameter,
    value: u8,
}

impl SceneLock {
    /// Creates a fixed Scene lock.
    ///
    /// # Errors
    ///
    /// Returns an error for incompatible targets or values outside `0..=127`.
    pub fn try_new(
        track: MacroTrack,
        parameter: MacroParameter,
        value: usize,
    ) -> Result<Self, RytmError> {
        validate_target(track, parameter)?;
        if value > 127 {
            return Err(range_error(value, "scene lock value"));
        }
        Ok(Self {
            track,
            parameter,
            value: value as u8,
        })
    }

    pub const fn track(self) -> MacroTrack {
        self.track
    }

    pub const fn parameter(self) -> MacroParameter {
        self.parameter
    }

    pub const fn value(self) -> u8 {
        self.value
    }
}

/// A signed modulation-depth assignment in a Performance definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PerformanceLock {
    track: MacroTrack,
    parameter: MacroParameter,
    depth: i8,
}

impl PerformanceLock {
    /// Creates a Performance modulation assignment.
    ///
    /// # Errors
    ///
    /// Returns an error when the parameter is incompatible with the target track.
    pub fn try_new(
        track: MacroTrack,
        parameter: MacroParameter,
        depth: i8,
    ) -> Result<Self, RytmError> {
        validate_target(track, parameter)?;
        Ok(Self {
            track,
            parameter,
            depth,
        })
    }

    pub const fn track(self) -> MacroTrack {
        self.track
    }

    pub const fn parameter(self) -> MacroParameter {
        self.parameter
    }

    pub const fn depth(self) -> i8 {
        self.depth
    }
}

/// One unrecognized four-byte record retained at its original slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UnknownMacroLock {
    slot: u8,
    raw: [u8; RECORD_SIZE],
}

impl UnknownMacroLock {
    pub const fn slot(self) -> u8 {
        self.slot
    }

    pub const fn raw(self) -> [u8; RECORD_SIZE] {
        self.raw
    }
}

/// Typed and unknown locks belonging to one Scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneDefinition {
    id: u8,
    locks: Vec<SceneLock>,
    unknown_locks: Vec<UnknownMacroLock>,
}

impl SceneDefinition {
    pub const fn id(&self) -> u8 {
        self.id
    }

    pub fn locks(&self) -> &[SceneLock] {
        &self.locks
    }

    pub fn unknown_locks(&self) -> &[UnknownMacroLock] {
        &self.unknown_locks
    }

    pub fn lock_count(&self) -> usize {
        self.locks.len() + self.unknown_locks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock_count() == 0
    }
}

/// Typed and unknown locks belonging to one Performance macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceDefinition {
    id: u8,
    locks: Vec<PerformanceLock>,
    unknown_locks: Vec<UnknownMacroLock>,
}

impl PerformanceDefinition {
    pub const fn id(&self) -> u8 {
        self.id
    }

    pub fn locks(&self) -> &[PerformanceLock] {
        &self.locks
    }

    pub fn unknown_locks(&self) -> &[UnknownMacroLock] {
        &self.unknown_locks
    }

    pub fn lock_count(&self) -> usize {
        self.locks.len() + self.unknown_locks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock_count() == 0
    }
}

/// Lossless storage and typed access for all 12 Scene definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SceneDefinitions {
    #[serde(with = "BigArray")]
    raw: [u8; RAW_SIZE],
}

impl Default for SceneDefinitions {
    fn default() -> Self {
        Self::from_raw(default_macro_records())
    }
}

impl SceneDefinitions {
    pub(crate) const fn from_raw(raw: [u8; RAW_SIZE]) -> Self {
        Self { raw }
    }

    pub(crate) const fn raw(&self) -> [u8; RAW_SIZE] {
        self.raw
    }

    /// Returns one typed Scene while retaining unrecognized records separately.
    pub fn definition(&self, id: usize) -> Result<SceneDefinition, RytmError> {
        let id = validate_macro_id(id)?;
        let mut locks = Vec::new();
        let mut unknown_locks = Vec::new();
        for (slot, record) in records(&self.raw).enumerate() {
            if record[3] != id {
                continue;
            }
            match decode_scene_record(record) {
                Some(lock) => locks.push(lock),
                None => unknown_locks.push(unknown_lock(slot, record)),
            }
        }
        Ok(SceneDefinition {
            id,
            locks,
            unknown_locks,
        })
    }

    /// Returns all 12 Scene definitions.
    pub fn definitions(&self) -> Vec<SceneDefinition> {
        (0..KIT_MACRO_COUNT)
            .map(|id| self.definition(id).expect("known Scene ID is valid"))
            .collect()
    }

    /// Defines or replaces one target inside a Scene.
    pub fn set_lock(&mut self, id: usize, lock: SceneLock) -> Result<(), RytmError> {
        validate_target(lock.track, lock.parameter)?;
        set_record(
            &mut self.raw,
            validate_macro_id(id)?,
            lock.track.raw_id(),
            lock.parameter.raw_id,
            lock.value,
        )
    }

    /// Replaces every lock belonging to one Scene.
    pub fn replace(&mut self, id: usize, locks: &[SceneLock]) -> Result<(), RytmError> {
        let records = locks
            .iter()
            .map(|lock| {
                validate_target(lock.track, lock.parameter)?;
                Ok([lock.value, lock.track.raw_id(), lock.parameter.raw_id, 0])
            })
            .collect::<Result<Vec<_>, RytmError>>()?;
        replace_records(&mut self.raw, validate_macro_id(id)?, &records)
    }

    /// Clears every typed or unknown record belonging to one Scene.
    pub fn clear(&mut self, id: usize) -> Result<(), RytmError> {
        clear_records(&mut self.raw, validate_macro_id(id)?);
        Ok(())
    }

    /// Copies a complete Scene, including unrecognized records, to another ID.
    pub fn copy(&mut self, source: usize, target: usize) -> Result<(), RytmError> {
        copy_records(
            &mut self.raw,
            validate_macro_id(source)?,
            validate_macro_id(target)?,
        )
    }

    pub fn lock_count(&self) -> usize {
        used_record_count(&self.raw)
    }

    pub fn unknown_locks(&self) -> Vec<UnknownMacroLock> {
        unknown_records(&self.raw, decode_scene_record)
    }
}

/// Lossless storage and typed access for all 12 Performance definitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceDefinitions {
    #[serde(with = "BigArray")]
    raw: [u8; RAW_SIZE],
}

impl Default for PerformanceDefinitions {
    fn default() -> Self {
        Self::from_raw(default_macro_records())
    }
}

impl PerformanceDefinitions {
    pub(crate) const fn from_raw(raw: [u8; RAW_SIZE]) -> Self {
        Self { raw }
    }

    pub(crate) const fn raw(&self) -> [u8; RAW_SIZE] {
        self.raw
    }

    /// Returns one typed Performance macro while retaining unrecognized records separately.
    pub fn definition(&self, id: usize) -> Result<PerformanceDefinition, RytmError> {
        let id = validate_macro_id(id)?;
        let mut locks = Vec::new();
        let mut unknown_locks = Vec::new();
        for (slot, record) in records(&self.raw).enumerate() {
            if record[3] != id {
                continue;
            }
            match decode_performance_record(record) {
                Some(lock) => locks.push(lock),
                None => unknown_locks.push(unknown_lock(slot, record)),
            }
        }
        Ok(PerformanceDefinition {
            id,
            locks,
            unknown_locks,
        })
    }

    /// Returns all 12 Performance definitions.
    pub fn definitions(&self) -> Vec<PerformanceDefinition> {
        (0..KIT_MACRO_COUNT)
            .map(|id| self.definition(id).expect("known Performance ID is valid"))
            .collect()
    }

    /// Defines or replaces one target inside a Performance macro.
    pub fn set_lock(&mut self, id: usize, lock: PerformanceLock) -> Result<(), RytmError> {
        validate_target(lock.track, lock.parameter)?;
        set_record(
            &mut self.raw,
            validate_macro_id(id)?,
            lock.track.raw_id(),
            lock.parameter.raw_id,
            lock.depth as u8,
        )
    }

    /// Replaces every lock belonging to one Performance macro.
    pub fn replace(&mut self, id: usize, locks: &[PerformanceLock]) -> Result<(), RytmError> {
        let records = locks
            .iter()
            .map(|lock| {
                validate_target(lock.track, lock.parameter)?;
                Ok([
                    lock.depth as u8,
                    lock.track.raw_id(),
                    lock.parameter.raw_id,
                    0,
                ])
            })
            .collect::<Result<Vec<_>, RytmError>>()?;
        replace_records(&mut self.raw, validate_macro_id(id)?, &records)
    }

    /// Clears every typed or unknown record belonging to one Performance macro.
    pub fn clear(&mut self, id: usize) -> Result<(), RytmError> {
        clear_records(&mut self.raw, validate_macro_id(id)?);
        Ok(())
    }

    /// Copies a complete Performance macro, including unrecognized records, to another ID.
    pub fn copy(&mut self, source: usize, target: usize) -> Result<(), RytmError> {
        copy_records(
            &mut self.raw,
            validate_macro_id(source)?,
            validate_macro_id(target)?,
        )
    }

    pub fn lock_count(&self) -> usize {
        used_record_count(&self.raw)
    }

    pub fn unknown_locks(&self) -> Vec<UnknownMacroLock> {
        unknown_records(&self.raw, decode_performance_record)
    }
}

const fn default_macro_records() -> [u8; RAW_SIZE] {
    let mut raw = [0; RAW_SIZE];
    let mut slot = 0;
    while slot < KIT_MACRO_LOCK_CAPACITY {
        let offset = slot * RECORD_SIZE;
        raw[offset] = EMPTY_RECORD[0];
        raw[offset + 1] = EMPTY_RECORD[1];
        raw[offset + 2] = EMPTY_RECORD[2];
        raw[offset + 3] = EMPTY_RECORD[3];
        slot += 1;
    }
    raw
}

fn records(raw: &[u8; RAW_SIZE]) -> impl Iterator<Item = &[u8]> {
    raw.chunks_exact(RECORD_SIZE)
}

fn records_mut(raw: &mut [u8; RAW_SIZE]) -> impl Iterator<Item = &mut [u8]> {
    raw.chunks_exact_mut(RECORD_SIZE)
}

fn validate_macro_id(id: usize) -> Result<u8, RytmError> {
    if id >= KIT_MACRO_COUNT {
        return Err(range_error(id, "Scene or Performance ID"));
    }
    Ok(id as u8)
}

fn validate_target(track: MacroTrack, parameter: MacroParameter) -> Result<(), RytmError> {
    track.validate()?;
    let expected = MacroParameter::try_from_raw(track, parameter.raw_id)?;
    if expected.page != parameter.page {
        return Err(RytmError::Parameter(ParameterError::Compatibility {
            value: format!("{:?}:{}", parameter.page, parameter.raw_id),
            parameter_name: "macro parameter".to_string(),
            reason: Some(format!("expected page {:?}", expected.page)),
        }));
    }
    Ok(())
}

fn parameter_from_raw(track: MacroTrack, raw_id: u8) -> Option<MacroParameter> {
    let page = match track {
        MacroTrack::Voice(_) => match raw_id {
            0..=7 => MacroParameterPage::Machine,
            8..=15 => MacroParameterPage::Sample,
            16..=23 => MacroParameterPage::Filter,
            24..=31 => MacroParameterPage::Amp,
            33..=40 => MacroParameterPage::Lfo,
            _ => return None,
        },
        MacroTrack::Fx => match raw_id {
            0..=7 => MacroParameterPage::Delay,
            8 | 9 | 17..=19 => MacroParameterPage::Distortion,
            10..=16 => MacroParameterPage::Reverb,
            21..=28 => MacroParameterPage::Compressor,
            29..=36 => MacroParameterPage::FxLfo,
            _ => return None,
        },
    };
    Some(MacroParameter { page, raw_id })
}

const fn parameter_name(page: MacroParameterPage, raw_id: u8) -> &'static str {
    match (page, raw_id) {
        (MacroParameterPage::Machine, 0) => "machine_parameter_1",
        (MacroParameterPage::Machine, 1) => "machine_parameter_2",
        (MacroParameterPage::Machine, 2) => "machine_parameter_3",
        (MacroParameterPage::Machine, 3) => "machine_parameter_4",
        (MacroParameterPage::Machine, 4) => "machine_parameter_5",
        (MacroParameterPage::Machine, 5) => "machine_parameter_6",
        (MacroParameterPage::Machine, 6) => "machine_parameter_7",
        (MacroParameterPage::Machine, 7) => "machine_parameter_8",
        (MacroParameterPage::Sample, 8) => "sample_tune",
        (MacroParameterPage::Sample, 9) => "sample_fine_tune",
        (MacroParameterPage::Sample, 10) => "sample_number",
        (MacroParameterPage::Sample, 11) => "sample_bit_reduction",
        (MacroParameterPage::Sample, 12) => "sample_start",
        (MacroParameterPage::Sample, 13) => "sample_end",
        (MacroParameterPage::Sample, 14) => "sample_loop",
        (MacroParameterPage::Sample, 15) => "sample_level",
        (MacroParameterPage::Filter, 16) => "filter_attack",
        (MacroParameterPage::Filter, 17) => "filter_sustain",
        (MacroParameterPage::Filter, 18) => "filter_decay",
        (MacroParameterPage::Filter, 19) => "filter_release",
        (MacroParameterPage::Filter, 20) => "filter_frequency",
        (MacroParameterPage::Filter, 21) => "filter_resonance",
        (MacroParameterPage::Filter, 22) => "filter_type",
        (MacroParameterPage::Filter, 23) => "filter_envelope",
        (MacroParameterPage::Amp, 24) => "amp_attack",
        (MacroParameterPage::Amp, 25) => "amp_hold",
        (MacroParameterPage::Amp, 26) => "amp_decay",
        (MacroParameterPage::Amp, 27) => "amp_overdrive",
        (MacroParameterPage::Amp, 28) => "amp_delay_send",
        (MacroParameterPage::Amp, 29) => "amp_reverb_send",
        (MacroParameterPage::Amp, 30) => "amp_pan",
        (MacroParameterPage::Amp, 31) => "amp_volume",
        (MacroParameterPage::Lfo, 33) => "lfo_speed",
        (MacroParameterPage::Lfo, 34) => "lfo_multiplier",
        (MacroParameterPage::Lfo, 35) => "lfo_fade",
        (MacroParameterPage::Lfo, 36) => "lfo_destination",
        (MacroParameterPage::Lfo, 37) => "lfo_waveform",
        (MacroParameterPage::Lfo, 38) => "lfo_phase",
        (MacroParameterPage::Lfo, 39) => "lfo_mode",
        (MacroParameterPage::Lfo, 40) => "lfo_depth",
        (MacroParameterPage::Delay, 0) => "delay_time",
        (MacroParameterPage::Delay, 1) => "delay_ping_pong",
        (MacroParameterPage::Delay, 2) => "delay_stereo_width",
        (MacroParameterPage::Delay, 3) => "delay_feedback",
        (MacroParameterPage::Delay, 4) => "delay_hpf",
        (MacroParameterPage::Delay, 5) => "delay_lpf",
        (MacroParameterPage::Delay, 6) => "delay_reverb_send",
        (MacroParameterPage::Delay, 7) => "delay_volume",
        (MacroParameterPage::Distortion, 8) => "distortion_delay_overdrive",
        (MacroParameterPage::Distortion, 9) => "distortion_delay_post",
        (MacroParameterPage::Reverb, 10) => "reverb_pre_delay",
        (MacroParameterPage::Reverb, 11) => "reverb_decay",
        (MacroParameterPage::Reverb, 12) => "reverb_shelving_frequency",
        (MacroParameterPage::Reverb, 13) => "reverb_shelving_gain",
        (MacroParameterPage::Reverb, 14) => "reverb_hpf",
        (MacroParameterPage::Reverb, 15) => "reverb_lpf",
        (MacroParameterPage::Reverb, 16) => "reverb_volume",
        (MacroParameterPage::Distortion, 17) => "distortion_reverb_post",
        (MacroParameterPage::Distortion, 18) => "distortion_amount",
        (MacroParameterPage::Distortion, 19) => "distortion_symmetry",
        (MacroParameterPage::Compressor, 21) => "compressor_threshold",
        (MacroParameterPage::Compressor, 22) => "compressor_attack",
        (MacroParameterPage::Compressor, 23) => "compressor_release",
        (MacroParameterPage::Compressor, 24) => "compressor_ratio",
        (MacroParameterPage::Compressor, 25) => "compressor_sidechain_eq",
        (MacroParameterPage::Compressor, 26) => "compressor_makeup_gain",
        (MacroParameterPage::Compressor, 27) => "compressor_mix",
        (MacroParameterPage::Compressor, 28) => "compressor_volume",
        (MacroParameterPage::FxLfo, 29) => "fx_lfo_speed",
        (MacroParameterPage::FxLfo, 30) => "fx_lfo_multiplier",
        (MacroParameterPage::FxLfo, 31) => "fx_lfo_fade",
        (MacroParameterPage::FxLfo, 32) => "fx_lfo_destination",
        (MacroParameterPage::FxLfo, 33) => "fx_lfo_waveform",
        (MacroParameterPage::FxLfo, 34) => "fx_lfo_phase",
        (MacroParameterPage::FxLfo, 35) => "fx_lfo_mode",
        (MacroParameterPage::FxLfo, 36) => "fx_lfo_depth",
        _ => "unknown",
    }
}

fn decode_scene_record(record: &[u8]) -> Option<SceneLock> {
    if record[0] > 127 {
        return None;
    }
    let track = MacroTrack::from_raw(record[1])?;
    let parameter = parameter_from_raw(track, record[2])?;
    Some(SceneLock {
        track,
        parameter,
        value: record[0],
    })
}

fn decode_performance_record(record: &[u8]) -> Option<PerformanceLock> {
    let track = MacroTrack::from_raw(record[1])?;
    let parameter = parameter_from_raw(track, record[2])?;
    Some(PerformanceLock {
        track,
        parameter,
        depth: record[0] as i8,
    })
}

fn set_record(
    raw: &mut [u8; RAW_SIZE],
    id: u8,
    track: u8,
    parameter: u8,
    value: u8,
) -> Result<(), RytmError> {
    if let Some(record) = records_mut(raw)
        .find(|record| record[3] == id && record[1] == track && record[2] == parameter)
    {
        record.copy_from_slice(&[value, track, parameter, id]);
        return Ok(());
    }
    let record = records_mut(raw)
        .find(|record| record[3] == 0xFF)
        .ok_or_else(macro_memory_full)?;
    record.copy_from_slice(&[value, track, parameter, id]);
    Ok(())
}

fn replace_records(
    raw: &mut [u8; RAW_SIZE],
    id: u8,
    new_records: &[[u8; RECORD_SIZE]],
) -> Result<(), RytmError> {
    let mut targets = Vec::with_capacity(new_records.len());
    for record in new_records {
        let target = (record[1], record[2]);
        if targets.contains(&target) {
            return Err(RytmError::Parameter(ParameterError::Compatibility {
                value: format!("track {} parameter {}", target.0, target.1),
                parameter_name: "macro locks".to_string(),
                reason: Some("a macro cannot contain duplicate targets".to_string()),
            }));
        }
        targets.push(target);
    }
    let available = records(raw)
        .filter(|record| record[3] == 0xFF || record[3] == id)
        .count();
    if new_records.len() > available {
        return Err(macro_memory_full());
    }
    clear_records(raw, id);
    for new_record in new_records {
        let record = records_mut(raw)
            .find(|record| record[3] == 0xFF)
            .expect("capacity was checked before replacement");
        record.copy_from_slice(&[new_record[0], new_record[1], new_record[2], id]);
    }
    Ok(())
}

fn clear_records(raw: &mut [u8; RAW_SIZE], id: u8) {
    for record in records_mut(raw).filter(|record| record[3] == id) {
        record.copy_from_slice(&EMPTY_RECORD);
    }
}

fn copy_records(raw: &mut [u8; RAW_SIZE], source: u8, target: u8) -> Result<(), RytmError> {
    if source == target {
        return Ok(());
    }
    let source_records = records(raw)
        .filter(|record| record[3] == source)
        .map(|record| <[u8; RECORD_SIZE]>::try_from(record).expect("record size is fixed"))
        .collect::<Vec<_>>();
    let available = records(raw)
        .filter(|record| record[3] == 0xFF || record[3] == target)
        .count();
    if source_records.len() > available {
        return Err(macro_memory_full());
    }
    clear_records(raw, target);
    for mut source_record in source_records {
        source_record[3] = target;
        let record = records_mut(raw)
            .find(|record| record[3] == 0xFF)
            .expect("capacity was checked before copy");
        record.copy_from_slice(&source_record);
    }
    Ok(())
}

fn unknown_records<T>(
    raw: &[u8; RAW_SIZE],
    decode: impl Fn(&[u8]) -> Option<T>,
) -> Vec<UnknownMacroLock> {
    records(raw)
        .enumerate()
        .filter(|(_, record)| record[3] != 0xFF)
        .filter(|(_, record)| record[3] >= KIT_MACRO_COUNT as u8 || decode(record).is_none())
        .map(|(slot, record)| unknown_lock(slot, record))
        .collect()
}

fn unknown_lock(slot: usize, record: &[u8]) -> UnknownMacroLock {
    UnknownMacroLock {
        slot: slot as u8,
        raw: <[u8; RECORD_SIZE]>::try_from(record).expect("record size is fixed"),
    }
}

fn used_record_count(raw: &[u8; RAW_SIZE]) -> usize {
    records(raw).filter(|record| record[3] != 0xFF).count()
}

fn macro_memory_full() -> RytmError {
    RytmError::Custom(format!(
        "Kit macro lock memory full; at most {KIT_MACRO_LOCK_CAPACITY} locks are available"
    ))
}

fn range_error(value: impl ToString, parameter_name: &str) -> RytmError {
    RytmError::Parameter(ParameterError::Range {
        value: value.to_string(),
        parameter_name: parameter_name.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_define_replace_clear_and_copy_are_lossless() {
        let mut scenes = SceneDefinitions::default();
        assert_eq!(scenes.definitions().len(), KIT_MACRO_COUNT);
        assert!(scenes.definitions().iter().all(SceneDefinition::is_empty));

        let voice = MacroTrack::try_voice(0).unwrap();
        let tune = MacroParameter::try_from_raw(voice, 8).unwrap();
        let cutoff = MacroParameter::try_from_raw(voice, 20).unwrap();
        scenes
            .set_lock(0, SceneLock::try_new(voice, tune, 65).unwrap())
            .unwrap();
        scenes
            .set_lock(0, SceneLock::try_new(voice, cutoff, 96).unwrap())
            .unwrap();
        assert_eq!(scenes.definition(0).unwrap().lock_count(), 2);

        let before_copy = scenes.raw();
        scenes.copy(0, 1).unwrap();
        assert_eq!(
            scenes.definition(1).unwrap().locks(),
            scenes.definition(0).unwrap().locks()
        );
        assert_eq!(&scenes.raw()[0..8], &before_copy[0..8]);

        scenes
            .replace(1, &[SceneLock::try_new(voice, tune, 64).unwrap()])
            .unwrap();
        assert_eq!(scenes.definition(1).unwrap().lock_count(), 1);
        assert_eq!(scenes.definition(0).unwrap().lock_count(), 2);
        scenes.clear(1).unwrap();
        assert!(scenes.definition(1).unwrap().is_empty());
        assert_eq!(scenes.definition(0).unwrap().lock_count(), 2);
    }

    #[test]
    fn performance_depths_and_fx_targets_round_trip() {
        let mut performances = PerformanceDefinitions::default();
        let delay_feedback = MacroParameter::try_from_raw(MacroTrack::Fx, 3).unwrap();
        let locks = [
            PerformanceLock::try_new(MacroTrack::Fx, delay_feedback, 63).unwrap(),
            PerformanceLock::try_new(
                MacroTrack::try_voice(11).unwrap(),
                MacroParameter::try_from_raw(MacroTrack::try_voice(11).unwrap(), 31).unwrap(),
                -64,
            )
            .unwrap(),
        ];
        performances.replace(11, &locks).unwrap();
        assert_eq!(performances.definition(11).unwrap().locks(), locks);
        assert_eq!(performances.raw()[0..4], [63, 12, 3, 11]);
        assert_eq!(performances.raw()[4..8], [192, 11, 31, 11]);
    }

    #[test]
    fn reserved_and_cross_family_parameters_are_rejected() {
        let voice = MacroTrack::try_voice(0).unwrap();
        assert!(MacroParameter::try_from_raw(voice, 32).is_err());
        assert!(MacroParameter::try_from_raw(MacroTrack::Fx, 20).is_err());
        assert!(MacroParameter::try_from_raw(MacroTrack::Fx, 8).is_ok());
        assert!(MacroParameter::try_from_raw(voice, 8).is_ok());
        assert!(MacroTrack::try_voice(12).is_err());
    }

    #[test]
    fn unknown_records_survive_unrelated_mutations_and_copy() {
        let mut raw = default_macro_records();
        raw[40..44].copy_from_slice(&[200, 31, 250, 3]);
        let mut scenes = SceneDefinitions::from_raw(raw);
        assert_eq!(scenes.definition(3).unwrap().unknown_locks().len(), 1);
        let original = scenes.raw()[40..44].to_vec();
        scenes
            .set_lock(
                0,
                SceneLock::try_new(
                    MacroTrack::try_voice(0).unwrap(),
                    MacroParameter::try_from_raw(MacroTrack::try_voice(0).unwrap(), 8).unwrap(),
                    64,
                )
                .unwrap(),
            )
            .unwrap();
        assert_eq!(scenes.raw()[40..44], original);
        scenes.copy(3, 4).unwrap();
        assert_eq!(scenes.definition(4).unwrap().unknown_locks().len(), 1);
    }

    #[test]
    fn kit_wide_capacity_and_duplicate_targets_are_enforced() {
        let mut performances = PerformanceDefinitions::default();
        for index in 0..KIT_MACRO_LOCK_CAPACITY {
            let macro_id = index / 4;
            let track = MacroTrack::try_voice(macro_id).unwrap();
            let parameter = MacroParameter::try_from_raw(track, (index % 4) as u8).unwrap();
            performances
                .set_lock(
                    macro_id,
                    PerformanceLock::try_new(track, parameter, 1).unwrap(),
                )
                .unwrap();
        }
        assert_eq!(performances.lock_count(), KIT_MACRO_LOCK_CAPACITY);
        let extra = PerformanceLock::try_new(
            MacroTrack::Fx,
            MacroParameter::try_from_raw(MacroTrack::Fx, 0).unwrap(),
            1,
        )
        .unwrap();
        assert!(performances.set_lock(0, extra).is_err());

        let duplicate = PerformanceLock::try_new(
            MacroTrack::try_voice(0).unwrap(),
            MacroParameter::try_from_raw(MacroTrack::try_voice(0).unwrap(), 8).unwrap(),
            1,
        )
        .unwrap();
        assert!(PerformanceDefinitions::default()
            .replace(0, &[duplicate, duplicate])
            .is_err());
    }
}
