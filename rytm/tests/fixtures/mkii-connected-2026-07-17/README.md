# Connected Analog Rytm MKII Fixture

Captured read-only over CoreMIDI from `Elektron Analog Rytm MKII` on 2026-07-17.

- Codec target: firmware 1.70.
- Observed device firmware: 1.72, verified on the device by the operator. The MIDI identity and
  object responses do not report the OS version.
- Objects: work-buffer Pattern, Kit, BD Sound, Global, Settings, and work-buffer Song.
- Personal sample audio: not included.
- Envelope validation: passed for all six objects, including framing, 7-bit data, encoded size, and
  checksum.
- Raw decode and re-encode: byte-exact for all six objects.
- Typed decode and re-encode: byte-exact for Pattern, Kit, Sound, Global, and Settings.
- Scene and Performance definitions: controlled one-lock and multi-track/multi-page definitions
  were written to the work-buffer Kit, read back through the typed codec, and rolled back to an
  exact baseline. Definition writes preserved the device's 0xFF inactive-Scene state.
- Song: byte-exact through `RawSysexObject`; typed Song decoding remains explicitly unsupported.

Reproduce the verification with:

```sh
cargo test -p rytm-rs --test firmware_fixtures
```

The binary fixtures are disposable protocol evidence from a new project. Regenerate them when
testing a verified firmware version or when an upstream codec change affects object serialization.
The macro certification JSON records the write/readback/rollback evidence; the corresponding
baseline, defined, and restored Kit SysEx files contain no sample audio.
