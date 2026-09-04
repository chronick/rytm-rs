# Maintained Fork Policy

This repository is the maintained `rytm-rs` fork used by
`analog-rytm-agent-bridge`.

## Repository roles

- `origin`: `https://github.com/algonormative/rytm-rs.git`, the writable fork.
- `upstream`: `https://github.com/alisomay/rytm-rs.git`, the original project.
- `upstream-codecs`: the branch offered upstream as a pull request. It is `upstream/main` plus
  the codec work as a clean commit series and nothing fork-specific.
- `agent-control`: the integration branch pinned by the bridge at an immutable commit. It is
  `upstream-codecs` plus this document.

Reusable device-model and SysEx codec work belongs here, on `upstream-codecs`. Agent tools,
durable queues, musical-boundary scheduling, transport policy, audio capture, bridge-specific
state, and the hardware certification tooling (the `certify_*` examples and their receipts) live
in the bridge repository. Changes here should remain small enough to propose upstream without
depending on the bridge.

## Synchronizing upstream

Before starting a codec milestone:

```sh
git fetch upstream
git rebase upstream/main upstream-codecs
git rebase upstream-codecs agent-control
cargo test -p rytm-rs --lib
cargo test -p rytm-rs --test firmware_fixtures
```

Resolve protocol changes on `upstream-codecs`, rerun the connected-device fixtures, then push
both branches. Update the bridge only after the fork commit exists on `origin`; the bridge must
pin that exact commit with Cargo's `rev` field. Do not pin a moving branch or a local path. A
force-push of `agent-control` is preceded by a tag on the old head so previously pinned revisions
stay fetchable.

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

Write/readback/rollback certification of the Scene, Performance, and Song codecs is the bridge's
workflow; see `docs/CODEC_CERTIFICATION.md` in `analog-rytm-agent-bridge`.

Run before publishing a fork revision:

    cargo fmt --all -- --check
    cargo test -p rytm-rs --lib
    cargo test -p rytm-rs --test firmware_fixtures
    cargo test -p rytm-rs-macro

The reverse_engineering.rs test target is an interactive hardware laboratory, not an unattended
workspace test. Run its individual procedures only while intentionally operating a connected
device. Strict Clippy status is tracked separately from codec milestones because the upstream
crate currently emits warnings under newer Rust toolchains.
