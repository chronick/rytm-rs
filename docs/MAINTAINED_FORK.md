# Maintained Fork Policy

This repository is the maintained `rytm-rs` fork used by
`analog-rytm-agent-bridge`.

## Repository roles

- `origin`: `https://github.com/chronick/rytm-rs.git`, the writable fork.
- `upstream`: `https://github.com/alisomay/rytm-rs.git`, the original project.
- `agent-control`: the integration branch pinned by the bridge at an immutable commit.

Reusable device-model and SysEx codec work belongs here. Agent tools, durable queues,
musical-boundary scheduling, transport policy, audio capture, and bridge-specific state do not.
Changes should remain small enough to propose upstream without depending on the bridge.

## Synchronizing upstream

Before starting a codec milestone:

```sh
git fetch upstream
git rebase upstream/main agent-control
cargo test --workspace
```

Resolve protocol changes in this fork, rerun the connected-device fixtures, then push
`agent-control`. Update the bridge only after the fork commit exists on `origin`; the bridge must
pin that exact commit with Cargo's `rev` field. Do not pin a moving branch or a local path.

## Firmware policy

The public codec target remains Analog Rytm firmware 1.70, matching upstream. A fixture captured
from a connected device is additional compatibility evidence, not a declaration that every field
on newer firmware is understood. The capture manifest records the target and observed firmware
separately. When the OS version cannot be queried over the available MIDI identity or object
responses, it remains explicit as unknown rather than inferred.

Unknown or unsupported objects must use `RawSysexObject` so validation and reserialization retain
every response byte. Typed models may only claim compatibility when decode followed by encode is
byte-exact against the committed fixture.

## Fixture workflow

Connect an Analog Rytm MKII over USB in Audio/MIDI mode, enable SysEx send and receive, and run:

```sh
cargo run -p rytm-rs --example capture_firmware_fixtures -- \
  rytm/tests/fixtures/mkii-connected-YYYY-MM-DD \
  --observed-firmware <version>
cargo test -p rytm-rs --test firmware_fixtures
```

Omit `--observed-firmware` when the version has not been verified on the device. Captures contain
Pattern, Kit, Sound, Global, Settings, and Song objects only. They must never include personal
sample audio, project backups, or unrelated user data.

Scene and Performance codec certification is query-only unless --execute is present:

    cargo run -p rytm-rs --example certify_macro_codecs -- \
      rytm/tests/fixtures/mkii-connected-YYYY-MM-DD \
      --observed-firmware <version>

    cargo run -p rytm-rs --example certify_macro_codecs -- \
      rytm/tests/fixtures/mkii-connected-YYYY-MM-DD \
      --observed-firmware <version> \
      --execute

The execute path snapshots the work-buffer Kit, writes controlled Scene and Performance
definitions, verifies typed readback, and restores the exact baseline. It reports an emergency
rollback failure separately. Definition writes never activate a Scene.

Song codec certification is also query-only unless `--execute` is present:

    cargo run -p rytm-rs --example certify_song_codec -- \
      rytm/tests/fixtures/mkii-connected-YYYY-MM-DD \
      --observed-firmware <version>

    cargo run -p rytm-rs --example certify_song_codec -- \
      rytm/tests/fixtures/mkii-connected-YYYY-MM-DD \
      --observed-firmware <version> \
      --execute

The execute path snapshots the work-buffer Song, writes controlled rows, a pattern chain,
repeats, and song mutes, verifies typed readback, and restores the exact baseline. See
[`SONG_CODEC.md`](SONG_CODEC.md) for the supported fields and preserved unknown regions.

Run before publishing a fork revision:

    cargo fmt --all -- --check
    cargo test -p rytm-rs --lib
    cargo test -p rytm-rs --test firmware_fixtures
    cargo test -p rytm-rs-macro

The reverse_engineering.rs test target is an interactive hardware laboratory, not an unattended
workspace test. Run its individual procedures only while intentionally operating a connected
device. Strict Clippy status is tracked separately from codec milestones because the upstream
crate currently emits warnings under newer Rust toolchains.
