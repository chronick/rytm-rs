use crate::{error::RytmError, util::stable_partition, RytmError::ParameterLockMemoryFull};
use derivative::Derivative;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

#[derive(Derivative, Clone, Copy, Serialize, Deserialize)]
#[derivative(Debug)]
pub struct PlockSeq {
    pub track_nr: u8,
    pub plock_type: u8,

    #[serde(with = "BigArray")]
    pub data: [u8; 64],
}

impl Default for PlockSeq {
    fn default() -> Self {
        Self {
            track_nr: 0xFF,
            plock_type: 0xFF,
            // This is not project default in AR but I think it is more sound.
            // The project default is 0x00 in AR.
            data: [0xFF; 64],
        }
    }
}

impl From<rytm_sys::ar_plock_seq_t> for PlockSeq {
    fn from(raw: rytm_sys::ar_plock_seq_t) -> Self {
        Self {
            track_nr: raw.track_nr,
            plock_type: raw.plock_type,
            data: raw.data,
        }
    }
}

impl From<&PlockSeq> for rytm_sys::ar_plock_seq_t {
    fn from(plock_seq: &PlockSeq) -> Self {
        Self {
            track_nr: plock_seq.track_nr,
            plock_type: plock_seq.plock_type,
            data: plock_seq.data,
        }
    }
}

/// Wrapper type for the parameter lock pool.
///
/// This represents the parameter lock pool for a single patterns.
///
/// Has a total of 72 slots for 72 different parameter locks.
///
/// Each slot can hold 64 parameter lock values which corresponds to the 64 possible trigs in a pattern.
#[derive(Derivative, Clone, Serialize, Deserialize)]
#[derivative(Debug)]
pub struct ParameterLockPool {
    pub owner_pattern_index: usize,
    pub inner: Vec<PlockSeq>,
    pub is_owner_pattern_work_buffer: bool,
}

impl Default for ParameterLockPool {
    fn default() -> Self {
        let mut inner = Vec::with_capacity(72);
        for _ in 0..72 {
            inner.push(PlockSeq::default());
        }

        Self {
            owner_pattern_index: 0,
            inner,
            is_owner_pattern_work_buffer: false,
        }
    }
}

impl ParameterLockPool {
    pub fn as_raw(&self) -> [rytm_sys::ar_plock_seq_t; 72] {
        self.inner
            .iter()
            .map(std::convert::Into::into)
            .collect::<Vec<_>>()
            .try_into()
            .expect("This can not fail until we change the size of 72 anywhere.")
    }

    // This type is around 4kb in size, copying is indeed inefficient.
    // But until now it didn't create any practical problems.
    // If we see a slowdown in the future we can change this.
    pub fn from_raw(
        raw: &[rytm_sys::ar_plock_seq_t; 72],
        owner_pattern_index: usize,
        is_owner_pattern_work_buffer: bool,
    ) -> Self {
        let inner = raw
            .iter()
            .map(|plock_seq| (*plock_seq).into())
            .collect::<Vec<_>>();

        Self {
            owner_pattern_index,
            inner,
            is_owner_pattern_work_buffer,
        }
    }

    pub fn set_basic_plock(
        &mut self,
        trig_index: usize,
        track_index: u8,
        plock_type: u8,
        value: u8,
    ) -> Result<(), RytmError> {
        // Check if we have this type of basic plock already set if so modify it.
        if let Some(plock) = self.inner.iter_mut().find(|plock_seq| {
            plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
        }) {
            plock.data[trig_index] = value;
            return Ok(());
        }

        // Check if we have an available slot anywhere in the array.
        if let Some(empty_slot) = self
            .inner
            .iter_mut()
            .find(|plock_seq| plock_seq.track_nr == 0xFF || plock_seq.plock_type == 0xFF)
        {
            // We know at this point that an empty slot is available.
            //
            // Reset every trig column to 0xFF ("no lock") before writing the
            // target column. A pool decoded from the device carries 0x00 in the
            // columns of trigs without a lock (the device marks locked trigs with
            // the per-trig *_PL_EN flags instead), and a claimed slot may carry
            // whatever bytes were there before. With crate-written patterns, which
            // do not set *_PL_EN, stale 0x00 columns were audible on hardware as
            // live locks (amp pan byte 0x00 = hard left).
            empty_slot.data = [0xFF; 64];
            empty_slot.track_nr = track_index;
            empty_slot.plock_type = plock_type;
            empty_slot.data[trig_index] = value;

            return Ok(());
        }

        Err(ParameterLockMemoryFull)
    }

    pub fn set_compound_plock(
        &mut self,
        trig_index: usize,
        track_index: u8,
        plock_type: u8,
        value: u16,
    ) -> Result<(), RytmError> {
        const ADJACENT_PLOCK_SLOT_TRACK_NUMBER_BYTE: u8 = 128;
        const ADJACENT_PLOCK_SLOT_TYPE_BYTE: u8 = 128;

        let value_msb = (value >> 8) as u8;
        let value_lsb = value as u8;

        // Partition the pool preserving the order so empty slots are stacked at the end.
        stable_partition(&mut self.inner[..], |plock_seq| {
            plock_seq.track_nr != 0xFF || plock_seq.plock_type != 0xFF
        });

        let last_slot = &self.inner[self.inner.len() - 1];
        let last_slot_available = last_slot.track_nr == 0xFF || last_slot.plock_type == 0xFF;

        // Check if we have this type of compound plock already set if so modify it.
        if let Some((i, found_plock)) =
            self.inner
                .iter_mut()
                .enumerate()
                .find(|(_, plock_seq)| -> bool {
                    plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
                })
        {
            // This is safe because if we could have set it it means these indexes are valid.
            found_plock.data[trig_index] = value_msb;
            self.inner[i + 1].data[trig_index] = value_lsb;

            return Ok(());
        }

        // Check if we have an available slot in the end of the array because if not we can't set the companion byte.
        // Thus we return memory full error.
        if !last_slot_available {
            return Err(ParameterLockMemoryFull);
        }

        // Then we have enough slots to set the companion byte.
        // Let's find the first available slot.
        if let Some((i, found_empty_slot)) = self
            .inner
            .iter_mut()
            .enumerate()
            .find(|(_, plock_seq)| plock_seq.track_nr == 0xFF || plock_seq.plock_type == 0xFF)
        {
            // We know at this point that 2 empty slots are available.
            //
            // Reset both slots' trig columns to 0xFF before writing the target
            // column (see `set_basic_plock` for why stale columns matter).
            found_empty_slot.data = [0xFF; 64];
            found_empty_slot.track_nr = track_index;
            found_empty_slot.plock_type = plock_type;
            found_empty_slot.data[trig_index] = value_msb;

            self.inner[i + 1].data = [0xFF; 64];
            self.inner[i + 1].track_nr = ADJACENT_PLOCK_SLOT_TRACK_NUMBER_BYTE;
            self.inner[i + 1].plock_type = ADJACENT_PLOCK_SLOT_TYPE_BYTE;
            self.inner[i + 1].data[trig_index] = value_lsb;

            return Ok(());
        }

        Err(ParameterLockMemoryFull)
    }

    pub fn get_basic_plock(
        &self,
        trig_index: usize,
        track_index: u8,
        plock_type: u8,
    ) -> Option<u8> {
        // Check if we have this type of basic plock already set if so modify it.
        if let Some(plock) = self.inner.iter().find(|plock_seq| {
            plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
        }) {
            return Some(plock.data[trig_index]);
        }
        None
    }

    pub fn get_compound_plock(
        &self,
        trig_index: usize,
        track_index: u8,
        plock_type: u8,
    ) -> Option<u16> {
        // Check if we have this type of basic plock already set if so modify it.
        if let Some((i, plock)) = self
            .inner
            .iter()
            .enumerate()
            .find(|(_, plock_seq)| -> bool {
                plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
            })
        {
            return Some(
                ((plock.data[trig_index] as u16) << 8)
                    | (self.inner[i + 1].data[trig_index] as u16),
            );
        }
        None
    }

    /// Clears the basic plock for the given trig.
    pub fn clear_basic_plock(&mut self, trig_index: usize, track_index: u8, plock_type: u8) {
        let mut plock_seq_index_which_we_cleared_from: Option<usize> = None;

        // Check if we have this type of basic plock already set if so modify it.
        if let Some((i, plock)) = self.inner.iter_mut().enumerate().find(|(_, plock_seq)| {
            plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
        }) {
            plock.data[trig_index] = 0xFF;

            plock_seq_index_which_we_cleared_from = Some(i);
        }

        if let Some(i) = plock_seq_index_which_we_cleared_from {
            let plock = &mut self.inner[i];
            if plock.data.iter_mut().all(|byte| *byte == 0xFF) {
                // Release slot.
                plock.track_nr = 0xFF;
                plock.plock_type = 0xFF;
            }
        }
    }

    pub fn clear_compound_plock(&mut self, trig_index: usize, track_index: u8, plock_type: u8) {
        let mut plock_seq_index_which_we_cleared_from: Option<usize> = None;

        // Check if we have this type of compound plock already set if so modify it.
        if let Some((i, plock)) = self.inner.iter_mut().enumerate().find(|(_, plock_seq)| {
            plock_seq.track_nr == track_index && plock_seq.plock_type == plock_type
        }) {
            plock.data[trig_index] = 0xFF;
            self.inner[i + 1].data[trig_index] = 0xFF;

            plock_seq_index_which_we_cleared_from = Some(i);
        }

        if let Some(i) = plock_seq_index_which_we_cleared_from {
            let plock = &mut self.inner[i];
            if plock.data.iter_mut().all(|byte| *byte == 0xFF) {
                // Release both slots. Reset the companion's data columns to the
                // unset sentinel as well: only explicitly-cleared columns were
                // 0xFF'd above, so a device-decoded companion could otherwise
                // leave stale LSB bytes behind in a slot marked free.
                plock.track_nr = 0xFF;
                plock.plock_type = 0xFF;
                self.inner[i + 1].track_nr = 0xFF;
                self.inner[i + 1].plock_type = 0xFF;
                self.inner[i + 1].data = [0xFF; 64];
            }
        }
    }

    pub fn set_fx_basic_plock(
        &mut self,
        trig_index: usize,
        plock_type: u8,
        value: u8,
    ) -> Result<(), RytmError> {
        self.set_basic_plock(trig_index, 12, plock_type, value)
    }

    pub fn set_fx_compound_plock(
        &mut self,
        trig_index: usize,
        plock_type: u8,
        value: u16,
    ) -> Result<(), RytmError> {
        self.set_compound_plock(trig_index, 12, plock_type, value)
    }

    pub fn get_fx_basic_plock(&self, trig_index: usize, plock_type: u8) -> Option<u8> {
        self.get_basic_plock(trig_index, 12, plock_type)
    }

    pub fn get_fx_compound_plock(&self, trig_index: usize, plock_type: u8) -> Option<u16> {
        self.get_compound_plock(trig_index, 12, plock_type)
    }

    pub fn clear_fx_basic_plock(&mut self, trig_index: usize, plock_type: u8) {
        self.clear_basic_plock(trig_index, 12, plock_type);
    }

    pub fn clear_fx_compound_plock(&mut self, trig_index: usize, plock_type: u8) {
        self.clear_compound_plock(trig_index, 12, plock_type);
    }

    pub fn clear_all_plocks(&mut self) {
        for plock_seq in &mut self.inner {
            plock_seq.track_nr = 0xFF;
            plock_seq.plock_type = 0xFF;
            for byte in &mut plock_seq.data {
                // Like the default.
                *byte = 0xFF;
            }
        }
    }

    pub fn clear_all_plocks_for_track(&mut self, track_index: u8) {
        for plock_seq in &mut self.inner {
            if plock_seq.track_nr == track_index {
                plock_seq.track_nr = 0xFF;
                plock_seq.plock_type = 0xFF;
                for byte in &mut plock_seq.data {
                    // Like the default.
                    *byte = 0xFF;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RytmProject;

    /// Dirty a free slot's data buffer to simulate the stale bytes a claimed
    /// slot can carry (e.g. 0x00 fill decoded from a device pool).
    fn dirty_free_slot(pool: &mut ParameterLockPool, index: usize) {
        pool.inner[index].data = [0x00; 64];
        assert_eq!(pool.inner[index].track_nr, 0xFF, "slot must stay free");
        assert_eq!(pool.inner[index].plock_type, 0xFF, "slot must stay free");
    }

    #[test]
    fn basic_plock_claim_resets_all_columns_to_the_unset_sentinel() {
        let mut pool = ParameterLockPool::default();
        dirty_free_slot(&mut pool, 0);

        // CH (track 8) AMP_PAN (0x1E) lock of -20 (byte 44) on trig 5.
        pool.set_basic_plock(5, 8, 0x1E, 44).unwrap();

        let slot = &pool.inner[0];
        assert_eq!(slot.track_nr, 8);
        assert_eq!(slot.plock_type, 0x1E);
        assert_eq!(slot.data[5], 44);
        for (i, byte) in slot.data.iter().enumerate() {
            if i != 5 {
                assert_eq!(
                    *byte, 0xFF,
                    "column {i} must be the unset sentinel, not a live value (0x00 = pan hard-left)"
                );
            }
        }
    }

    #[test]
    fn compound_plock_claim_resets_all_columns_of_both_slots() {
        let mut pool = ParameterLockPool::default();
        dirty_free_slot(&mut pool, 0);
        dirty_free_slot(&mut pool, 1);

        pool.set_compound_plock(3, 8, 0x28, 0x5900).unwrap();

        let msb = &pool.inner[0];
        let lsb = &pool.inner[1];
        assert_eq!((msb.track_nr, msb.plock_type), (8, 0x28));
        assert_eq!((lsb.track_nr, lsb.plock_type), (128, 128));
        assert_eq!(msb.data[3], 0x59);
        assert_eq!(lsb.data[3], 0x00);
        for i in 0..64 {
            if i != 3 {
                assert_eq!(msb.data[i], 0xFF, "MSB column {i} must be sentinel");
                assert_eq!(lsb.data[i], 0xFF, "LSB column {i} must be sentinel");
            }
        }
    }

    #[test]
    fn modifying_an_existing_basic_plock_does_not_disturb_other_columns() {
        let mut pool = ParameterLockPool::default();
        pool.set_basic_plock(5, 8, 0x1E, 44).unwrap();
        pool.set_basic_plock(9, 8, 0x1E, 84).unwrap();

        let slot = &pool.inner[0];
        assert_eq!(slot.data[5], 44);
        assert_eq!(slot.data[9], 84);
        for i in 0..64 {
            if i != 5 && i != 9 {
                assert_eq!(slot.data[i], 0xFF);
            }
        }
    }

    /// Returns the pattern's pool slots as (`track_nr`, `plock_type`, `data`)
    /// after running `set` against trig 0 of track 0.
    fn pool_after<F: Fn(&crate::object::pattern::Trig)>(set: F) -> Vec<(u8, u8, [u8; 64])> {
        let mut project = RytmProject::try_default().unwrap();
        let pattern = &mut project.patterns_mut()[0];
        set(&pattern.tracks()[0].trigs()[0]);
        let pool = pattern.parameter_lock_pool.lock();
        pool.inner
            .iter()
            .filter(|slot| slot.track_nr != 0xFF || slot.plock_type != 0xFF)
            .map(|slot| (slot.track_nr, slot.plock_type, slot.data))
            .collect()
    }

    #[test]
    fn lfo_depth_is_a_single_basic_byte_slot_with_the_compound_msb_value() {
        // pattern.h: AR_PLOCK_TYPE_LFO_DEPTH (0x28) depth (0..127) — BASIC.
        // Byte mapping (device readback of previously compound-written depths):
        // 50.0 -> 89, 90.0 -> 109.
        for (depth, expected_byte) in [(50.0f32, 89u8), (90.0, 109), (0.0, 64), (-128.0, 0)] {
            let slots = pool_after(|trig| trig.plock_set_lfo_depth(depth).unwrap());
            assert_eq!(
                slots.len(),
                1,
                "depth {depth}: exactly one slot, no companion"
            );
            let (track_nr, plock_type, data) = &slots[0];
            assert_eq!((*track_nr, *plock_type), (0, 0x28));
            assert_eq!(
                data[0], expected_byte,
                "depth {depth} -> byte {expected_byte}"
            );
        }
    }

    #[test]
    fn sample_start_and_end_are_single_basic_byte_slots_position_exact() {
        // pattern.h: SMP_START (0x0C) / SMP_END (0x0D), 0..120 — BASIC. Integer
        // positions map to the position byte itself (former compound factor 256).
        let slots = pool_after(|trig| trig.plock_set_sample_start(30.0).unwrap());
        assert_eq!(slots.len(), 1, "no companion slot");
        assert_eq!((slots[0].0, slots[0].1, slots[0].2[0]), (0, 0x0C, 30));

        let slots = pool_after(|trig| trig.plock_set_sample_end(120.0).unwrap());
        assert_eq!(slots.len(), 1, "no companion slot");
        assert_eq!((slots[0].0, slots[0].1, slots[0].2[0]), (0, 0x0D, 120));
    }

    #[test]
    fn basic_routed_params_round_trip_and_clear_through_the_public_api() {
        let mut project = RytmProject::try_default().unwrap();
        let pattern = &mut project.patterns_mut()[0];
        let trig = &pattern.tracks()[0].trigs()[0];

        trig.plock_set_lfo_depth(50.0).unwrap();
        let depth = trig.plock_get_lfo_depth().unwrap().unwrap();
        assert!((depth - 50.0).abs() < 0.1, "lfo_depth read back {depth}");

        trig.plock_set_sample_start(30.0).unwrap();
        assert_eq!(trig.plock_get_sample_start().unwrap(), Some(30.0));

        trig.plock_set_sample_end(120.0).unwrap();
        assert_eq!(trig.plock_get_sample_end().unwrap(), Some(120.0));

        trig.plock_clear_lfo_depth().unwrap();
        assert_eq!(trig.plock_get_lfo_depth().unwrap(), None);
        trig.plock_clear_sample_start().unwrap();
        assert_eq!(trig.plock_get_sample_start().unwrap(), None);
        trig.plock_clear_sample_end().unwrap();
        assert_eq!(trig.plock_get_sample_end().unwrap(), None);

        // All slots released after the clears.
        let all_released = {
            let pool = pattern.parameter_lock_pool.lock();
            pool.inner
                .iter()
                .all(|slot| slot.track_nr == 0xFF && slot.plock_type == 0xFF)
        };
        assert!(all_released);
    }
}
