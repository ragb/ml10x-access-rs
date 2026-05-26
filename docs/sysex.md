# ML10X SysEx message catalog

**Status:** confirmed from the official editor's minified JS bundle
(`editor-mkii.morningstar.io/main.84d3e4b49d337515.js`, captured
2026-05-25). Behavior on the live device still needs to be checked
against MIDI-OX captures in Phase 1b, but the encoding rules below are
taken directly from the editor's `sysexBuilder` / `dataParser` / `qi`
checksum code.

## Framing

Every Morningstar SysEx message is built by `class oi` (the
`sysexBuilder`) and parsed by `class la` (the `dataParser`). It is a
**16-byte header** followed by zero or more **typed data segments**,
followed by the checksum and EOX. (The editor's `endBuild` pushes a
placeholder `0` and `247`, then overwrites the second-to-last byte
with the computed checksum — so the trailing layout is just
`[checksum, F7]`, no extra padding.)

```
index   field                       value / meaning
-----   -------------------------   --------------------------------------------
0       SYSEX_START_POS             0xF0  (always)
1       MANF_ID_1_POS               0x00  Morningstar manufacturer ID byte 1
2       MANF_ID_2_POS               0x21  Morningstar manufacturer ID byte 2
3       MANF_ID_3_POS               0x24  Morningstar manufacturer ID byte 3
4       MODEL_ID_POS                0x07  ML10X model byte (confirmed)
5       VERSION_ID_1_POS            0x00  (editor builder always passes 0)
6       FUNCTION_ID_1_POS           P1 — primary command code
7       FUNCTION_ID_2_POS           P2 — secondary command code
8       FUNCTION_ID_3_POS           P3 — depends on command
9       FUNCTION_ID_4_POS           P4 — depends on command
10      FUNCTION_ID_5_POS           P5
11      FUNCTION_ID_6_POS           P6
12      FUNCTION_ID_7_POS           P7
13      FUNCTION_ID_8_POS           P8
14      (reserved out / LENGTH_MSB in)
15      (reserved out / LENGTH_LSB in)
16..    ARRAY_START                 zero or more data segments (see below)
N-1     checksum                    XOR of bytes 0..N-2, masked to 7 bits
N       EOX                         0xF7
```

The other Morningstar device models live alongside ML10X in a single
enum that the editor uses to identify itself; the ML10X is `7`:

```
UNKNOWN_MODEL = 0
MC6MK2        = 3
MC8           = 4
MC3           = 5
MC6PRO        = 6
ML10X         = 7    <-- this project
MC8PRO        = 8
MC4PRO        = 9
ML5R          = 10
MC6PROMK2     = 11
BASE_MODEL    = 112
PRO_MODEL     = 113
LIVE_MODE     = 127
```

## Length field (inbound only)

Outbound messages built by the editor set bytes 14 and 15 to zero. **Inbound
messages (device → host) put the total message length in those two bytes**, and
the editor rejects any message whose declared length doesn't match the actual
byte count (it fires `sendRetryRequest(10)` — a `P2=126` retry message — and
loops until a correctly-framed reply arrives).

Encoding (verbatim from the editor's parser at offset ~2,372,405):

```js
const it = (64 & je) >> 6 == 1;      // top bit of byte 14 is a flag
je &= 63;                            // bottom 6 bits of byte 14
const un = je << 7 | data[15];       // declared length
if (data.length != un) sendRetryRequest(10);
```

So:

```
byte 14 = (length >> 7) & 0x3F  | (flag << 6)
byte 15 = length & 0x7F
```

The flag bit (top bit of byte 14) appears to indicate "is-last-chunk" or similar
— we'll pin its meaning when we observe a multi-chunk dump. Verified against
the 935-byte controller-data message captured 2026-05-25: bytes 14,15 = `07 27`
→ `(0x07 & 0x3F)<<7 | 0x27` = `7*128 + 39` = 935. ✓

## Checksum

Verbatim from the bundle (function `qi`):

```js
function qi(l){
    let d=l[0], i=l.length-2;
    for(let o=1; o<i; o++) d ^= l[o];
    return d &= 127, d;
}
```

Equivalent in Python:

```python
def checksum(message: bytes) -> int:
    """message is the full frame including F0 and EOX. The two trailing
    bytes (checksum slot and F7) are excluded from the XOR."""
    acc = message[0]
    for b in message[1 : len(message) - 2]:
        acc ^= b
    return acc & 0x7F
```

Note that **F0 (0xF0) is included in the XOR**. The two trailing bytes
(checksum slot + EOX) are excluded.

## Data segments

After the 16-byte header, the body is a sequence of segments. Each
segment is written by `addData(id, len, bytes)` as:

```
0x7F  <segment_id>  <length>  <length bytes, each masked to 7 bits>
```

Segments are separated by an implicit `0x7F` lead-in — the parser
(`la.getNext()`) returns `hasNext = true` when the byte immediately
following a segment is another `0x7F`.

All payload bytes are masked to 7 bits via `byte & 0x7F` at write time.
Anything that needs more than 7 bits of range must be split into
multiple bytes by the caller (see the bypass-loop bitmap below).

## Function codes (P1, P2)

The editor enumerates a set of named function codes for outbound
requests. Values not in this list are still valid and used directly
in calls (e.g. `sendSysexFunction(0, 24)` for "editor connecting" /
handshake). Only the named ones below have been recovered from source;
the unnamed numerics 16–32 are command codes whose meaning will be
pinned in Phase 1b by capture diffs.

| P2  | name                                  | notes |
| --- | ------------------------------------- | ----- |
| 0   | DUMMY                                 | no-op |
| 29  | ENGAGE_PRESET                         | activate a preset |
| 30  | ENGAGE_EXP                            | activate an expression preset |
| 35  | REQUEST_CONTROLLER_SETTINGS_ALL       | full controller dump |
| 36  | REQUEST_CONTROLLER_GENERAL_CONFIG     | |
| 37  | REQUEST_WAVEFORM_ENGINE               | not used on ML10X (MC series) |
| 38  | REQUEST_SEQUENCER_ENGINE              | not used on ML10X |
| 39  | REQUEST_SCROLL_SLOTS                  | |
| 40  | REQUEST_MIDI_CHANNEL_NAMES            | |
| 41  | REQUEST_BANK_ARRANGEMENT              | |
| 42  | REQUEST_OMNIPORT_DATA                 | |
| 43  | REQUEST_BANK_PRESET_NAMES             | names of presets in a bank |
| 44  | REQUEST_CONTROLLER_FIRMWARE_VERSION   | |
| 45  | REQUEST_EVENT_PROCESSOR               | |
| 46  | REQUEST_CONTROLLER_UUID               | device serial |
| 47  | TOGGLE_LOOPER_MODE                    | |
| 64  | REQUEST_PRESET_NAMES                  | |
| 65  | REQUEST_EXPRESSION_CALIBRATION        | |
| 66  | REQUEST_RESISTOR_LADDER_CALIBRATION   | |
| 80  | REQUEST_MIDI_CLOCK_SLOTS              | |

The editor calls these by passing `P1 = 0` (model context) and the
desired P2 from the table. P3..P8 carry parameters (bank index, preset
index, etc.) — these are command-specific and will be documented per
command in Phase 1c.

## Connect handshake (captured against real device 2026-05-25)

Editor → device (only **4** messages, ~12 ms apart):

```
F0 00 21 24 07 00 00 00 00 00 00 00 00 00 00 00 cs F7   (P2=0)   probe
F0 00 21 24 07 00 00 18 00 00 00 00 00 00 00 00 cs F7   (P2=24)  open editor
F0 00 21 24 07 00 00 13 00 00 00 00 00 00 00 00 cs F7   (P2=19)  unknown setup
F0 00 21 24 07 00 00 15 00 00 00 00 00 00 00 00 cs F7   (P2=21)  unknown setup
```

Device → host: an unsolicited stream of:

| inbound class       | f1 | f2  | size  | meaning                                  |
| ------------------- | -- | --- | ----- | ---------------------------------------- |
| Device Connected    |  1 |  0  | 18    | snackbar code 0 → fires `editor:"connected"` |
| loading_start       |  1 |  5  | 18    | snackbar code 5                          |
| loading_end         |  1 |  6  | 18    | snackbar code 6                          |
| loading_progress    |  1 |  7  | 18    | snackbar code 7; P3 is the progress value |
| Controller info     | 17 |  0  | 53    | firmware version + UUID (case 17 in `hasNewMessage`) |
| Preset data         |  6 |  0  | 219   | one preset; P3=preset_no, P4=bank_no, P5>0=advanced |
| Controller data     |  6 |  1  | 935   | global config + connector names           |
| All-preset names    |  6 |  2  | 1938  | every preset's name in one bank          |

The device streams these spontaneously once the editor sends the 4-message connect handshake. Repeats are observed (each "case 6" message arrives twice in our capture) — likely a sync mechanism the editor will deduplicate.

## Inbound message classifier

`class la` (`dataParser`) reads incoming messages by header position
constants (the `BZ` enum in `module 65128`):

```
ARRAY_START         = 16
SYSEX_START_POS     = 0
MANF_ID_1_POS       = 1
MANF_ID_2_POS       = 2
MANF_ID_3_POS       = 3
MODEL_ID_POS        = 4
VERSION_ID_1_POS    = 5
FUNCTION_ID_1_POS   = 6
FUNCTION_ID_2_POS   = 7
...
FUNCTION_ID_8_POS   = 13
```

`la.start(arr)` returns a struct with `{f1..f8, modelId}`, where each
`f*` is the byte at the corresponding position. `getNext()` then walks
data segments starting at index 16.

## Preset model (class Kb)

Per the editor's `Kb` class — what a preset stores:

- `presetName: string`
- `presetNumber: int` (0..127 within the bank)
- `bankNumber: int` (0..3 corresponding to banks A..D)
- `mutedSwitchOption: int (0..2)`
- `tipTrails: int`, `ringTrails: int`
- `bypassLoopStatus: 24-bit integer` — bitmap of which loops are bypassed.
  Encoded on the wire as three 7-bit bytes via:
  ```
  getBypassLoopStatusByte() = [
      bypassLoopStatus       & 0x7F,
      (bypassLoopStatus>>7)  & 0x7F,
      (bypassLoopStatus>>14) & 0x7F,
  ]
  setBypassLoopStatusByte(b0, b1, b2):
      bypassLoopStatus = (b2&0x7F)<<14 | (b1&0x7F)<<7 | b0&0x7F
  ```
  21 bits of state fit in three 7-bit bytes — more than the 10 loops × 2
  states (engaged vs bypassed, plus alt-pin / mute combinations) the
  ML10X needs.
- `isAdvancedMode: bool` — Advanced vs Simple preset mode.
- `matrixArray: object` — routing matrix, keyed by connector pair.
- `connectionArray: array` — list of active connections between connectors.

The 10 loop endpoints are labeled in the editor's UI as **A Tip, A Ring,
B Tip, B Ring, ..., E Tip, E Ring** (five send/return pairs × two pins
each = 10). The matching back-panel jacks on the device use the same
letters. The YAML schema in `schemas/preset.schema.json`
adopts these names verbatim — `a_tip`, `a_ring`, ..., `e_tip`, `e_ring` —
so the file reads identically to what a sighted user would see in the
official editor.

There are also four fixed endpoints (not user-bypassable):
**Input Tip, Input Ring, Output Tip, Output Ring** — these define where
the signal enters and exits the matrix.

Editor UI numbering for banks and presets: **banks 1..4**, **presets
0..127 within each bank**. The YAML schema follows the same numbering
so a printed manual page maps 1:1 to a YAML file.

The Editor's JSON export (used by save/restore) has the shape:

```json
{
  "presetNumber": ...,
  "presetName": ...,
  "tipT": ..., "ringT": ...,
  "matrixArray": {...},
  "advancedMode": true|false,
  "bypass": <24-bit int>
}
```

## Controller model (class Jb) — global device settings

Not per-preset; one of these per device.

- `midiChannel: int (0..16)` — 0 means "ignore MIDI in".
- `deviceId: int (0..16)`
- `inputSplit: bool`
- `loopBypassPersistent: bool`
- `inputRingAlternatePin: int`
- `connectors[]` — 10 SEND_RETURN + 1 Input Tip + 1 Input Ring + 1 Output Tip + 1 Output Ring = 14 entries. Each has:
  - `id`, `position`, `name`, `shortName`, `inputName`, `outputName`
  - `groupNumber`, `type` (SEND_RETURN | INPUT | OUTPUT)
  - `includeInTrails: bool`

This is richer than the simple "loop_1: on/off" the original plan
sketched — Phase 2b's YAML schema needs to model the connector graph
properly.

### Controller data segment catalog (f1=6 f2=1) — 935 bytes, 61 segments

Confirmed 2026-05-25 by reading the editor's Controller Settings page and
matching displayed values against captured bytes:

| Segment id | len | What it carries |
| ---------- | --- | --------------- |
| 0..9       | 16  | **Long names** for the 10 loops: A Tip, A Ring, ..., E Ring (ASCII, space-padded to 16) |
| 10..13     | 16  | Long names for the 4 fixed endpoints: Input Tip, Input Ring, Output Tip, Output Ring |
| 32         | 1   | **MIDI Channel** (0..16; 0 = "Ignore MIDI in"). |
| 33         | 2   | **`include_in_trails` bitmap** — `(byte_1 << 7) | byte_0`. 10 bits used (one per loop A Tip..E Ring), 4 bits reserved. Maps to the "Enable Spillover" toggles in Connector Settings. |
| 34         | 1   | **Device ID** (0..16). |
| 35         | 1   | **Input Split** (0 = Off, 1 = On). |
| 36         | 1   | **Loop Bypass Persist** (0 = Off, 1 = On). |
| 48..61     | 4   | **Short names** (4 chars each) for the 14 connectors. User's test device: `76`, `AR`, `OD`, `sd`, `at`, `mf`, `pr`, `TD`, `O5`, `EQ`, `I+`, `I-`, `O+`, `O-` |
| 64..77     | 16  | Per-connector **input-side display label** (what the editor shows when this connector is upstream) |
| 80..93     | 16  | Per-connector **output-side display label** |

Note: `inputRingSwapPin` is referenced in the editor's `Jb` class and is
present as `inputRingSwapPin: 0` in JSON device backups, but the
Controller Settings page on firmware v1.2 has no toggle for it and it
always reads back as 0. The Python model omits it; if a future firmware
exposes it we'll add it back with a confirmed segment id.

The `include_in_trails` bitmap (segment 33) has room for 14 bits but
the editor only renders 10 "Enable Spillover" toggles — one per loop.
Bits 10..13 (Input Tip/Ring, Output Tip/Ring) are unused on v1.2.

Segment ids **48..63 in *preset* context** carry the device's
`matrixArray` (per the editor's `addToMatrix(x.id, x.data)`) — these
are NOT a MIDI message list. The ML10X has no per-preset MIDI messages
at all (unlike the MC-series controllers). The CLI ignores segments
48..63 on read and emits nothing for them on write; the device rebuilds
its matrix from segments 0..13.

### Preset data segment catalog

**Simple vs Advanced** is encoded in the message *header*, not in a segment:

- **READ (device → host)**: header field `P5 > 0` means Advanced. The
  function code stays `f1=6 f2=0` either way.
- **WRITE (host → device)**: `f2=0` = write a Simple preset, `f2=2` =
  write an Advanced preset. They have *different segment layouts*; the
  parser branches on this f2 byte (`handlePresetData` vs
  `handleAdvancedPresetData` in the editor).

There is **no segment-20 mode flag** — segment 20 only appears in Simple
writes and likely encodes something else (its value was `0` in our
Empty-preset capture).

#### Simple preset WRITE (f2=0) — example: empty preset, 16 segments

| Segment id | shape         | What it carries |
| ---------- | ------------- | --------------- |
| 0          | `<a> <b> 0`   | Per-connector chain entry; appears once per connector that has any chain state. In the Simple write, the segment id IS the connector index, and the bytes are `<self> <linked_to> 0`. |
| 1..13      | `<id> 7F` or `<a> <b> <flag>` | More chain entries; connectors with no link send `<id> 7F` to mean "not linked". |
| 16, 17     | 1 b each      | **Spillover target** (Output Tip / Output Ring). WRITE form: `0x7F` = "Nothing", `0x7E` = "Last connected" (suspected), `0..9` = A Tip..E Ring. |
| 20         | 1 b           | Simple-only flag, observed value 0 (purpose unconfirmed). |
| 32         | 4..12 b       | **Preset name** (trimmed of trailing spaces). |
| 48..63     | 3 b each, optional | **MIDI message list** — 16 slots. Omitted entirely when all slots are empty. |

#### Advanced preset WRITE (f2=2) — example: empty preset, 8 segments

| Segment id | shape   | What it carries |
| ---------- | ------- | --------------- |
| varies     | 1 b     | Sparse routing matrix entries (each segment id = a connector index, single byte = routed target). |
| 16, 17     | 1 b each| Spillover target — same encoding as Simple. |
| 18         | 1 b     | Advanced-only flag, observed value 0. |
| 19         | 3 b     | Advanced-only field, observed `00 00 00`. |
| 32         | 4..12 b | Preset name (trimmed). |

#### Preset READ (device → host, f1=6 f2=0) — same payload shape as the corresponding WRITE format, but padded

The READ format pads names to 12 bytes and uses 3-byte connection records
(`<from> <to> <flag>`) where Simple WRITE uses 2 bytes. READ also includes
segment id=18 (1 byte, device-only flag).

## MIDI message type catalog (per-preset outbound)

When a preset is engaged, the ML10X can emit MIDI messages. The editor
enumerates these types (`qG` enum in module 6146):

```
M_EMPTY=0, M_PCMSG=1, M_CCMSG=2, M_NOTEON=3, M_NOTEOFF=4,
M_REALTIME=5, M_SYSEX=6, M_CLOCKTAP=7,
M_PC_SCROLL_UP=8, M_PC_SCROLL_DOWN=9,
M_BANK_UP=10, M_BANK_DOWN=11, M_BANK_CHANGE_MODE=12, M_SET_BANK=13,
M_TOGGLE_PG=14, M_SET_TOGGLE=15, M_MIDI_THRU=16, M_SELECT_EXP=17,
M_LOOPER_MODE=18, M_STRYMON_BANK_UP=19, M_STRYMON_BANK_DOWN=20,
M_AXEFX_TUNER=21, M_TOGGLE_PRESET=22, M_DELAY=23, M_MIDI_CLOCK_TAP=24,
M_MIDI_SONG_POS=25, M_CCMSG_WAVEFORM=26, M_ENGAGE_PRESET=27,
M_KEMPER_TUNER=28, M_MIDI_SONG_SELECT=29, M_SET_DISPLAY=30,
M_CC_SEQUENCER=31, M_CC_SCROLL=32, M_PC_SCROLL=33, M_PC_CHANNEL=34,
M_KEY_STROKES=35, M_UTILITY=36, M_AXEFX_INTEGRATION=37, M_NRPN=38,
M_ML10X=39, M_OMNIPORT_RELAY=40, M_MIDI_MMC=41,
M_TRIGGER_MESSAGES=42, M_FOCUS_MODE=43, M_PRESET_RENAME=44,
M_ML5R=45, M_DEVICE_ENGAGE_BYPASS=46
```

For Phase 2 MVP we will handle: `M_PCMSG`, `M_CCMSG`, `M_NOTEON`,
`M_NOTEOFF`, `M_SYSEX`. The remainder are exotic and can be deferred.

## Editor architecture facts (helpful for further reverse-engineering)

- Single-page Angular app served from `editor-mkii.morningstar.io`.
- Main bundle: `main.84d3e4b49d337515.js` (~7.8 MB unminified).
- No source map exposed.
- Uses Web MIDI API: `navigator.requestMIDIAccess({sysex: true})`.
- Demo mode is a runtime flag (`demoMode = true`), no separate URL.
- `sysexBuilder` is constructed in two flavors per session:
  - `new oi(0, 0x21, 0x24, 0)` — generic, model=0, used during identify.
  - `new oi(0, 0x21, 0x24, 7)` — ML10X-specific, used after identify.

## Write-preset (host → device)

Captured 2026-05-25 by clicking **Save Preset** on the loaded preset 0 in the
official editor (no edits — just round-tripping the in-memory state back to
the device). Captured fixture: `tests/fixtures/real-device-save-preset-0.json`.

The editor sends **one** SysEx message (98 bytes) and the device acknowledges
with `f1=1, f2=2` ("Preset Settings Saved!").

Outbound header:

```
P1 = 6   (DATA class — same class used inbound for preset data)
P2 = 0   (preset sub-type)
P3 = 127 (sentinel meaning "save to currently-selected preset")
P4 = 0   (bank — but only meaningful with explicit-target P3 values)
P5..P8 = 0
LENGTH_MSB / LSB = 0  — outbound messages don't set the length field
```

To target a *specific* preset, the editor first switches the device's current
preset via `P2=32(?, P3=preset, P4=bank)` (this is the call shape we saw in
the bundle at the `sendSysexFunction(0,32,i,o)` callsites). After navigation
the save uses `P3=127` to mean "overwrite that current preset".

The 16 segments in the save payload (compared against the corresponding READ
where the device pads everything to fixed lengths):

| Segment id | WRITE shape | READ shape  | meaning (working theory)              |
| ---------- | ----------- | ----------- | -------------------------------------- |
| 0          | `00 02 01`  | `00 02 01`  | preset header / version                |
| 3..13      | `<id> 7f`   | `<id> 7f 00`| per-connector connection state — segment id is the connector index (3..13 = D Tip..Output Ring); first data byte echoes the connector id, second is the destination connector or `0x7F` "not connected" |
| 16, 17     | `7f`        | `00 00`     | output-tip and output-ring spillover targets (`0x7F` = "Nothing") |
| 20         | `00`        | `00`        | preset mode flag (Simple vs Advanced)  |
| 32         | `Base`      | `Base    …` | preset name — outbound trims trailing spaces, device pads on storage |

Notable differences from the inbound preset format:

- READ pads the preset name to **12 bytes**; WRITE strips trailing spaces.
- READ uses 3-byte connection records (`<from> <to> <flag>`); WRITE uses
  2-byte records and only includes the connectors that have meaningful state.
- READ includes segments id 48..63 (16 × 3-byte slots — probably the MIDI
  message list). WRITE in this no-edit capture omitted them because the
  preset has no MIDI messages — they're optional/absent rather than
  zero-filled.

The device replies with one 18-byte `f1=1 f2=2` snackbar message ("Preset
Settings Saved!") within ~35 ms and does **not** echo back a refreshed
preset dump — the editor trusts its local state.

## Active preset selection and listing (host → device)

The editor uses two opcodes whenever the user clicks a different bank or preset button:

```
P2 = 22, P3 = bank      select-bank    (P1=0, P4..P8 = 0)
P2 = 18, P3 = preset    select-preset  (P1=0, P4..P8 = 0)
```

These have no acknowledgement message — fire and forget. The device updates its on-screen indicator and engages the new routing. Use both together (bank first, then preset) to fully address a slot.

In addition to changing what's playing, a **bank-select** also triggers a fresh response stream from the device. The stream includes the new preset's data (`f1=6 f2=0`) and, if the device thinks the host needs it, an all-preset-names dump (`f1=6 f2=2`). On v1.2 firmware the all-preset-names dump is **not always sent** — only as part of the initial connect handshake, or when navigating between banks the host hasn't seen yet. In particular:

- `sendSysexFunction(0, REQUEST_PRESET_NAMES=64, bank)` is defined in the editor's source but **produces no response** on the firmware as tested 2026-05-25.
- `sendSysexFunction(0, REQUEST_BANK_PRESET_NAMES=43)` likewise produces no response in our tests.
- `sendSysexFunction(0, REQUEST_CONTROLLER_FIRMWARE_VERSION=44)` likewise — none of the dedicated `REQUEST_*` opcodes appear to elicit a reply.

In practice, to enumerate every preset's name, the only reliable path is to walk each `(bank, preset)` with `select-bank` then `select-preset` and read the per-preset `f1=6 f2=0` response. That's what `ml10x list --device` and `ml10x dump --all` do.

## Open questions, to resolve in Phase 1b by live captures

1. Whether `P3=127` is the only legal sentinel for write_preset or whether
   the editor can also write by explicit `(preset, bank)`. Test by replaying
   the captured save with `P3=N` instead. Risky without device permission.
2. Exact layout of segments id=48..63 in a preset that *has* MIDI messages
   (empty in our capture). Re-capture after adding a CC to a preset.
3. Whether segment id=18 (READ-only, len 1, value 0) ever appears in WRITE —
   maybe it's a device-only "loaded-once" flag.
4. Whether segment id=0 (`00 02 01` in both directions) is a static preset-
   format version header or carries per-preset bytes we haven't varied.
5. Naming for the remaining numeric P2 ops: 16, 17, 18 (with P3 arg),
   19, 20 (with P3 arg), 21, 22 (with P3 arg), 23, 24, 25, 26, 27, 28.

## Source pointers (within `captures/editor-main.network-response`)

- `class oi` (sysexBuilder): ~offset 2,291,946
- `class la` (dataParser): ~offset 2,275,115
- `class Kb` (Preset): ~offset 2,758,907
- `class Jb` (Controller): ~offset 2,756,589
- `function qi` (checksum): one match
- `BZ` byte-position enum: ~offset 2,184,270 (module 65128)
- model enum (`MC6MK2..ML10X..`): ~offset 2,029,690 (module 82340)
- `bt` function-code enum: ~offset 2,294,290
- message-type enum (`qG`): module 6146

These offsets help future digs into the same bundle file; they will
shift if Morningstar deploys a new build (the filename hash changes).
