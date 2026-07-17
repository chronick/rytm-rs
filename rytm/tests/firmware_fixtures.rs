use rytm_rs::prelude::*;
use serde_json::Value;
use std::{fs, path::PathBuf};

const FIXTURE_DIRECTORY: &str = "tests/fixtures/mkii-connected-2026-07-17";
const FIXTURES: [(&str, SysexType, usize); 6] = [
    ("pattern-work-buffer.syx", SysexType::Pattern, 14_988),
    ("kit-work-buffer.syx", SysexType::Kit, 2_998),
    ("sound-work-buffer-bd.syx", SysexType::Sound, 201),
    ("global-work-buffer.syx", SysexType::Global, 107),
    ("settings.syx", SysexType::Settings, 2_401),
    ("song-work-buffer.syx", SysexType::Song, 1_506),
];

#[test]
fn connected_device_fixtures_validate_and_preserve_every_byte() {
    for (file_name, expected_type, expected_size) in FIXTURES {
        let bytes = fixture(file_name);
        assert_eq!(
            bytes.len(),
            expected_size,
            "unexpected size for {file_name}"
        );
        let raw = RawSysexObject::from_sysex(&bytes).unwrap();
        assert_eq!(raw.metadata().object_type().unwrap(), expected_type);
        assert_eq!(raw.bytes(), bytes);
        assert_eq!(raw.as_sysex().unwrap(), bytes);
    }
}

#[test]
fn typed_objects_decode_and_reencode_without_unknown_byte_loss() {
    let mut failures = Vec::new();
    for (file_name, object_type, _) in FIXTURES {
        let bytes = fixture(file_name);
        let mut project = RytmProject::try_default().unwrap();
        project.update_from_sysex_response(&bytes).unwrap();
        let encoded = match object_type {
            SysexType::Pattern => project.work_buffer().pattern().as_sysex().unwrap(),
            SysexType::Kit => project.work_buffer().kit().as_sysex().unwrap(),
            SysexType::Sound => project.work_buffer().sounds()[0].as_sysex().unwrap(),
            SysexType::Global => project.work_buffer().global().as_sysex().unwrap(),
            SysexType::Settings => project.settings().as_sysex().unwrap(),
            SysexType::Song => project.work_buffer().song().as_sysex().unwrap(),
        };
        if encoded != bytes {
            failures.push(format!(
                "typed round trip changed {file_name}: {}",
                difference_summary(&bytes, &encoded)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn typed_scene_and_performance_macros_match_hardware_readback() {
    let bytes = fixture("macros-defined-kit.syx");
    let raw = RawSysexObject::from_sysex(&bytes).unwrap();
    assert_eq!(raw.metadata().object_type().unwrap(), SysexType::Kit);

    let mut project = RytmProject::try_default().unwrap();
    project.update_from_sysex_response(&bytes).unwrap();
    let kit = project.work_buffer().kit();
    assert_eq!(kit.current_scene_id(), None);
    assert_eq!(kit.current_scene_id_raw(), 0xFF);

    let scene_zero = kit.scene_definitions().definition(0).unwrap();
    assert_eq!(scene_zero.locks().len(), 1);
    assert_eq!(
        scene_lock_tuple(scene_zero.locks()[0]),
        (MacroTrack::Voice(0), 8, 65)
    );
    let scene_one = kit.scene_definitions().definition(1).unwrap();
    assert_eq!(
        scene_one
            .locks()
            .iter()
            .copied()
            .map(scene_lock_tuple)
            .collect::<Vec<_>>(),
        [(MacroTrack::Voice(1), 20, 96), (MacroTrack::Fx, 3, 80),]
    );

    let performance_zero = kit.performance_definitions().definition(0).unwrap();
    assert_eq!(performance_zero.locks().len(), 1);
    assert_eq!(
        performance_lock_tuple(performance_zero.locks()[0]),
        (MacroTrack::Voice(0), 8, 12)
    );
    let performance_one = kit.performance_definitions().definition(1).unwrap();
    assert_eq!(
        performance_one
            .locks()
            .iter()
            .copied()
            .map(performance_lock_tuple)
            .collect::<Vec<_>>(),
        [(MacroTrack::Voice(1), 30, -32), (MacroTrack::Fx, 11, 24),]
    );

    assert_eq!(kit.as_sysex().unwrap(), bytes);
    assert_eq!(
        fixture("macros-restored-kit.syx"),
        fixture("macros-baseline-kit.syx")
    );

    let report: Value = serde_json::from_slice(&fixture("macros-certification.json")).unwrap();
    assert_eq!(report["schema"], "rytm-rs-macro-certification.v1");
    assert_eq!(report["status"], "write-readback-rollback-verified");
    assert_eq!(report["observedFirmware"], "1.72");
    assert_eq!(report["baselineFingerprint"], report["restoredFingerprint"]);
}

#[test]
fn macro_definition_fixture_preserves_unrelated_kit_bytes() {
    const PERFORMANCE_CONTROL_BYTES: std::ops::Range<usize> = 0x0842..0x0902;
    const SCENE_CONTROL_BYTES: std::ops::Range<usize> = 0x0917..0x09D7;

    let baseline = RawSysexObject::from_sysex(&fixture("macros-baseline-kit.syx"))
        .unwrap()
        .decoded_bytes()
        .unwrap();
    let defined = RawSysexObject::from_sysex(&fixture("macros-defined-kit.syx"))
        .unwrap()
        .decoded_bytes()
        .unwrap();
    assert_eq!(baseline.len(), defined.len());

    let changed_offsets = baseline
        .iter()
        .zip(&defined)
        .enumerate()
        .filter_map(|(offset, (before, after))| (before != after).then_some(offset))
        .collect::<Vec<_>>();
    assert!(!changed_offsets.is_empty());
    assert!(changed_offsets.iter().all(|offset| {
        PERFORMANCE_CONTROL_BYTES.contains(offset) || SCENE_CONTROL_BYTES.contains(offset)
    }));
}

#[test]
fn song_fixture_decodes_as_typed_work_buffer_state() {
    let bytes = fixture("song-work-buffer.syx");
    let raw = RawSysexObject::from_sysex(&bytes).unwrap();
    assert_eq!(raw.metadata().object_type().unwrap(), SysexType::Song);
    assert_eq!(raw.as_sysex().unwrap(), bytes);

    let mut project = RytmProject::try_default().unwrap();
    project.update_from_sysex_response(&bytes).unwrap();
    let song = project.work_buffer().song();
    assert!(song.is_work_buffer());
    assert!(song.rows().unwrap().is_empty());
    assert_eq!(song.as_sysex().unwrap(), bytes);
}

#[test]
fn typed_song_matches_hardware_write_readback_and_rollback() {
    let baseline = fixture("song-certification-baseline.syx");
    let defined = fixture("song-certification-defined.syx");
    let restored = fixture("song-certification-restored.syx");
    assert_eq!(restored, baseline);

    let song = Song::from_sysex(&defined).unwrap();
    assert_eq!(song.name(), "AGENT SONG");
    let rows = song.rows().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].repeats(), 2);
    assert_eq!(rows[0].patterns().len(), 2);
    assert_eq!(rows[0].patterns()[0].pattern(), 0);
    assert_eq!(rows[0].patterns()[1].pattern(), 1);
    assert_eq!(rows[0].patterns()[1].muted_tracks_mask(), 1);
    assert_eq!(rows[1].repeats(), 1);
    assert_eq!(rows[1].patterns()[0].pattern(), 16);
    assert_eq!(rows[1].patterns()[0].muted_tracks_mask(), 2);
    assert_eq!(song.as_sysex().unwrap(), defined);

    let report: Value = serde_json::from_slice(&fixture("song-certification.json")).unwrap();
    assert_eq!(report["schema"], "rytm-rs-song-certification.v1");
    assert_eq!(report["status"], "write-readback-rollback-verified");
    assert_eq!(report["observedFirmware"], "1.72");
    assert_eq!(report["baselineFingerprint"], report["restoredFingerprint"]);
}

#[test]
fn fixture_manifest_matches_committed_bytes_and_firmware_policy() {
    let manifest: Value = serde_json::from_slice(&fixture("manifest.json")).unwrap();
    assert_eq!(manifest["schema"], "rytm-rs-firmware-fixtures.v1");
    assert_eq!(manifest["codecTargetFirmware"], "1.70");
    assert_eq!(
        manifest["observedFirmware"],
        "unknown-not-reported-by-device-identity"
    );
    assert_eq!(manifest["sampleAudioIncluded"], false);
    assert_eq!(
        manifest["objects"].as_array().unwrap().len(),
        FIXTURES.len()
    );

    for object in manifest["objects"].as_array().unwrap() {
        let file_name = object["file"].as_str().unwrap();
        let bytes = fixture(file_name);
        let raw = RawSysexObject::from_sysex(&bytes).unwrap();
        assert_eq!(object["bytes"].as_u64().unwrap(), bytes.len() as u64);
        assert_eq!(object["fingerprint"], fingerprint(&bytes));
        assert_eq!(
            object["checksum"].as_u64().unwrap(),
            u64::from(raw.metadata().chksum)
        );
        assert_eq!(
            object["dataSize"].as_u64().unwrap(),
            u64::from(raw.metadata().data_size)
        );
        assert_eq!(
            object["sysexType"].as_str().unwrap(),
            format!("{:?}", raw.metadata().object_type().unwrap()).to_ascii_lowercase()
        );
    }
}

#[test]
fn checksum_corruption_is_rejected() {
    let mut bytes = fixture("sound-work-buffer-bd.syx");
    bytes[20] ^= 1;
    assert!(RawSysexObject::from_sysex(&bytes).is_err());
}

fn fixture(file_name: &str) -> Vec<u8> {
    fs::read(fixture_path().join(file_name)).unwrap()
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIRECTORY)
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}

fn difference_summary(expected: &[u8], actual: &[u8]) -> String {
    let differences = expected
        .iter()
        .zip(actual)
        .enumerate()
        .filter(|(_, (left, right))| left != right)
        .map(|(index, (left, right))| format!("{index}:{left}->{right}"))
        .collect::<Vec<_>>();
    let raw_summary = match (
        RawSysexObject::from_sysex(expected).and_then(|object| object.decoded_bytes()),
        RawSysexObject::from_sysex(actual).and_then(|object| object.decoded_bytes()),
    ) {
        (Ok(expected_raw), Ok(actual_raw)) => {
            let differences = expected_raw
                .iter()
                .zip(actual_raw.iter())
                .enumerate()
                .filter(|(_, (left, right))| left != right)
                .map(|(index, (left, right))| format!("{index}:{left}->{right}"))
                .collect::<Vec<_>>();
            format!(
                "; {} raw differences: {}",
                differences.len(),
                differences
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ => String::new(),
    };
    format!(
        "expected {} bytes, got {}; {} differing bytes; first differences: {}",
        expected.len(),
        actual.len(),
        differences.len(),
        differences
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
    ) + &raw_summary
}

fn scene_lock_tuple(lock: SceneLock) -> (MacroTrack, u8, u8) {
    (lock.track(), lock.parameter().raw_id(), lock.value())
}

fn performance_lock_tuple(lock: PerformanceLock) -> (MacroTrack, u8, i8) {
    (lock.track(), lock.parameter().raw_id(), lock.depth())
}
