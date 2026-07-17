# Connected Analog Rytm MKII Fixture

Captured read-only over CoreMIDI from `Elektron Analog Rytm MKII` on 2026-07-17.

- Codec target: firmware 1.70.
- Observed device firmware: unknown; the available MIDI identity and object responses do not report
  an OS version, and no version was inferred.
- Objects: work-buffer Pattern, Kit, BD Sound, Global, Settings, and work-buffer Song.
- Personal sample audio: not included.
- Envelope validation: passed for all six objects, including framing, 7-bit data, encoded size, and
  checksum.
- Raw decode and re-encode: byte-exact for all six objects.
- Typed decode and re-encode: byte-exact for Pattern, Kit, Sound, Global, and Settings.
- Song: byte-exact through `RawSysexObject`; typed Song decoding remains explicitly unsupported.

Reproduce the verification with:

```sh
cargo test -p rytm-rs --test firmware_fixtures
```

The binary fixtures are disposable protocol evidence from a new project. Regenerate them when
testing a verified firmware version or when an upstream codec change affects object serialization.
