use anyhow::{anyhow, Context, Result};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use rytm_rs::prelude::*;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const DEFAULT_PORT_MATCH: &str = "Elektron Analog Rytm MKII";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);

struct MidiSession {
    _input: MidiInputConnection<()>,
    output: MidiOutputConnection,
    receiver: Receiver<Vec<u8>>,
    input_name: String,
    output_name: String,
}

impl MidiSession {
    fn open(port_match: &str) -> Result<Self> {
        let mut input = MidiInput::new("rytm-rs-fixture-input")?;
        input.ignore(Ignore::None);
        let input_port = input
            .ports()
            .into_iter()
            .find(|port| {
                input
                    .port_name(port)
                    .is_ok_and(|name| name.contains(port_match))
            })
            .ok_or_else(|| anyhow!("no MIDI input contains {port_match:?}"))?;
        let input_name = input.port_name(&input_port)?;

        let output = MidiOutput::new("rytm-rs-fixture-output")?;
        let output_port = output
            .ports()
            .into_iter()
            .find(|port| {
                output
                    .port_name(port)
                    .is_ok_and(|name| name.contains(port_match))
            })
            .ok_or_else(|| anyhow!("no MIDI output contains {port_match:?}"))?;
        let output_name = output.port_name(&output_port)?;

        let (sender, receiver) = mpsc::channel();
        let input_connection = input
            .connect(
                &input_port,
                "rytm-rs-fixture-input",
                move |_stamp, message, _| {
                    let _ = sender.send(message.to_vec());
                },
                (),
            )
            .map_err(|error| anyhow!(error.to_string()))?;
        let output_connection = output
            .connect(&output_port, "rytm-rs-fixture-output")
            .map_err(|error| anyhow!(error.to_string()))?;

        Ok(Self {
            _input: input_connection,
            output: output_connection,
            receiver,
            input_name,
            output_name,
        })
    }

    fn request(&mut self, request: &[u8]) -> Result<Vec<u8>> {
        while self.receiver.try_recv().is_ok() {}
        self.output
            .send(request)
            .map_err(|error| anyhow!(error.to_string()))?;
        self.receive_sysex()
    }

    fn receive_sysex(&self) -> Result<Vec<u8>> {
        let deadline = std::time::Instant::now() + RESPONSE_TIMEOUT;
        let mut response = Vec::new();
        let mut receiving = false;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return Err(anyhow!("timed out waiting for a complete SysEx response"));
            }
            let message = match self.receiver.recv_timeout(remaining) {
                Ok(message) => message,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(anyhow!("timed out waiting for a SysEx response"));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("MIDI input disconnected while waiting for SysEx"));
                }
            };
            for byte in message {
                if byte >= 0xF8 {
                    continue;
                }
                if !receiving {
                    if byte != 0xF0 {
                        continue;
                    }
                    receiving = true;
                }
                response.push(byte);
                if byte == 0xF7 {
                    return Ok(response);
                }
            }
        }
    }
}

fn main() -> Result<()> {
    let options = Options::parse()?;
    fs::create_dir_all(&options.output_directory).with_context(|| {
        format!(
            "failed to create fixture directory {}",
            options.output_directory.display()
        )
    })?;
    let mut session = MidiSession::open(&options.port_match)?;
    let queries = [
        (
            "pattern-work-buffer",
            PatternQuery::new_targeting_work_buffer().as_sysex()?,
        ),
        (
            "kit-work-buffer",
            KitQuery::new_targeting_work_buffer().as_sysex()?,
        ),
        (
            "sound-work-buffer-bd",
            SoundQuery::new_targeting_work_buffer(0)?.as_sysex()?,
        ),
        (
            "global-work-buffer",
            GlobalQuery::new_targeting_work_buffer().as_sysex()?,
        ),
        ("settings", SettingsQuery::new().as_sysex()?),
        (
            "song-work-buffer",
            SongQuery::new_targeting_work_buffer().as_sysex()?,
        ),
    ];

    let mut objects = Vec::new();
    for (name, query) in queries {
        let response = session
            .request(&query)
            .with_context(|| format!("failed to query {name}"))?;
        let raw = RawSysexObject::from_sysex(&response)
            .with_context(|| format!("failed to validate {name}"))?;
        let file_name = format!("{name}.syx");
        fs::write(options.output_directory.join(&file_name), raw.bytes())
            .with_context(|| format!("failed to write {file_name}"))?;
        objects.push(json!({
            "name": name,
            "file": file_name,
            "sysexType": format!("{:?}", raw.metadata().object_type()?).to_ascii_lowercase(),
            "bytes": raw.bytes().len(),
            "checksum": raw.metadata().chksum,
            "dataSize": raw.metadata().data_size,
            "fingerprint": fingerprint(raw.bytes()),
        }));
    }

    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let manifest = json!({
        "schema": "rytm-rs-firmware-fixtures.v1",
        "codecTargetFirmware": "1.70",
        "observedFirmware": options.observed_firmware,
        "compatibilityStatus": "capture-complete-roundtrip-pending",
        "capturedAtUnix": captured_at,
        "device": "Analog Rytm MKII",
        "midiInput": session.input_name,
        "midiOutput": session.output_name,
        "sampleAudioIncluded": false,
        "objects": objects,
    });
    fs::write(
        options.output_directory.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&manifest)?);
    Ok(())
}

struct Options {
    output_directory: PathBuf,
    port_match: String,
    observed_firmware: String,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut arguments = std::env::args().skip(1);
        let output_directory = arguments.next().map(PathBuf::from).ok_or_else(|| {
            anyhow!(
                "usage: capture_firmware_fixtures <output-directory> [--port-match <name>] [--observed-firmware <version>]"
            )
        })?;
        let mut port_match = DEFAULT_PORT_MATCH.to_string();
        let mut observed_firmware = "unknown-not-reported-by-device-identity".to_string();
        while let Some(option) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| anyhow!("missing value for {option}"))?;
            match option.as_str() {
                "--port-match" => port_match = value,
                "--observed-firmware" => observed_firmware = value,
                _ => return Err(anyhow!("unknown option {option:?}")),
            }
        }
        Ok(Self {
            output_directory,
            port_match,
            observed_firmware,
        })
    }
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}
