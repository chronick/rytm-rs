# Song Codec

The fork models the 1,304-byte decoded Song object losslessly. The layout was independently
confirmed against a connected Analog Rytm MKII on OS 1.72 and the public `midiator` Song parser.
The user-visible semantics are checked against the official Analog Rytm MKII OS 1.72 manual.

## Confirmed layout

| Raw range | Size | Typed meaning |
| --- | ---: | --- |
| `0x000..0x017` | 24 | Header, including the 15-byte name at `0x004` |
| `0x018..0x117` | 256 | 64 four-byte row records: pattern count and repeat-minus-one |
| `0x118..0x517` | 1,024 | 256 four-byte pattern positions: 16-bit mute word and pattern index |

The typed API supports the Song name, up to 64 rows, up to 256 total pattern positions, row
repeats, pattern chains, and the 12 drum-track mute bits at each pattern position. It preserves
unidentified header bytes, two unidentified bytes in each row record, one unidentified byte in
each pattern-position record, and the upper four bits of each mute word.

The OS 1.72 Song UI does not expose tempo overrides, per-row pattern-length overrides, jumps,
loops, row labels, or a separate explicit end-row command. Playback ends after the final active
row. The codec does not infer those features from unidentified bytes; `SongCapabilities` reports
them as unsupported.

## Hardware evidence

`certify_song_codec` is read-only by default. With `--execute`, it captures the work-buffer Song,
writes a controlled two-row Song, queries and verifies the typed state, and restores the exact
captured bytes. The committed OS 1.72 receipt and dumps are in
`rytm/tests/fixtures/mkii-connected-2026-07-17`.

Sources:

- [Analog Rytm MKII User Manual, OS 1.72](https://elektron.se/wp-content/uploads/2025/01/Analog-Rytm-MKII-User-Manual_ENG_OS1.72_250130.pdf)
- [midiator](https://github.com/nerdprojects/midiator)
