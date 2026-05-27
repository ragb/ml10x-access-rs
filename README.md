# ml10x-access-rs

A command-line editor for the **Morningstar ML10X** loop switcher.
Presets and global settings live as YAML files you edit in any editor;
the `ml10x` CLI syncs them to and from the device over USB MIDI.

Status: working against firmware **v1.2**. Verified end-to-end on a real
ML10X: Simple-mode sync, Advanced-mode sync, full 512-preset backup,
byte-exact round-trips against the official editor.

## Why

The official web editor (`editor-mkii.morningstar.io/ml10x`) is a
drag-and-drop SPA. This project gives you a scriptable, text-based
alternative — useful for diffing presets in git, batch-editing in your
favourite editor, or anywhere a GUI is in the way.

## Install

Requires a Rust toolchain (1.95 or newer).

```bash
git clone https://github.com/ragb/ml10x-access-rs
cd ml10x-access-rs
cargo build --release
```

The binary lands at `target/release/ml10x` (or `ml10x.exe` on Windows).
Copy it onto your `PATH`, or run via `cargo run --release -- <args>`.

`cargo install --path .` installs into `~/.cargo/bin`.

## Quick start

```bash
# 1. Confirm the device is detected.
ml10x ports

# 2. Pull all 512 presets + global settings into ./presets/.
ml10x dump --out ./presets --all

# 3. Edit a preset YAML in your text editor (VS Code gets autocomplete
#    from the JSON Schema — see "Editor support" below).
$EDITOR ./presets/bank-1/preset-002.yaml

# 4. Push your changes back. Device acks "Preset Settings Saved".
ml10x sync ./presets/bank-1/preset-002.yaml

# 5. Compare local YAML against the live device any time.
ml10x diff ./presets/bank-1/preset-002.yaml
```

## Commands

### `ml10x ports`

Lists MIDI input/output ports. Ports matching `ML10X` or `Morningstar`
are flagged.

```
ml10x ports
ml10x --json ports
```

### `ml10x dump --out DIR [--all] [--port SUBSTRING]`

Read presets from the device into YAML files.

- Without `--all`: dumps the currently-active preset plus
  `controller.yaml`. Fast (~7 s).
- With `--all`: walks every (bank, preset) and writes one YAML per
  slot. Takes ~2 minutes against the hardware.

```
ml10x dump --out ./presets
ml10x dump --out ./presets --all --verbose
ml10x --json dump --out ./presets --all
```

### `ml10x sync TARGET`

`TARGET` can be a preset YAML, a `controller.yaml`, or a directory.
A directory is walked recursively: `controller.yaml` is synced first
(so connector names land before any preset references them) and then
every `preset-*.yaml` over a single MIDI connection.

```
ml10x sync ./presets/bank-1/preset-002.yaml
ml10x sync ./preset.yaml --dry-run
ml10x sync ./presets/controller.yaml
ml10x sync ./presets
ml10x sync ./presets/bank-1 --strict   # fail-fast on first ack miss
```

### `ml10x diff PRESET_FILE`

Connects, fetches the device's current copy of the same `bank`/`number`,
prints field-by-field differences. Useful pre-flight before `sync`.

### `ml10x show PRESET_FILE`

Re-emits a YAML file through the schema (canonical field order,
trimmed strings). Sanity check that a hand-edited file parses cleanly.
Runs the full validator (schema + semantics).

### `ml10x lint TARGET`

Validates one preset YAML or every preset under a directory, without
touching the device. Errors exit 3; warnings exit 0 unless `--strict`.

```
ml10x lint ./preset.yaml
ml10x lint ./presets
ml10x lint ./presets --strict
ml10x --json lint ./presets
```

### `ml10x select BANK PRESET`

Activate a preset on the device (like pressing a footswitch).

### `ml10x list [DIR | --device]`

List presets — either from a directory of YAML files (fast, read-only)
or from the device (walks every preset, slow). `--bank N` limits the
output to one bank; `--empty` includes slots named "Empty".

## Preset YAML

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/ragb/ml10x-access-rs/main/schemas/preset.schema.json
preset:
  bank: 1                # 1..4 (matches the editor's "Bank 1".."Bank 4")
  number: 0              # 0..127
  name: My Preset        # up to 16 ASCII chars
  spillover:
    output_tip: nothing  # nothing | input_tip | input_ring | a_tip..e_ring
    output_ring: nothing
  body:                  # mode-specific routing — Simple or Advanced
    mode: simple
    chain:               # linear signal flow with per-loop bypass
      - { from_connector: input_tip, to_connector: a_tip,      bypass: false }
      - { from_connector: a_tip,     to_connector: output_tip, bypass: false }
```

For Advanced mode, the body uses a `connections:` graph (no `bypass`,
because the firmware ignores per-loop bypass in Advanced presets):

```yaml
preset:
  bank: 1
  number: 0
  name: Stereo Out
  spillover:
    output_tip: nothing
    output_ring: nothing
  body:
    mode: advanced
    connections:
      - { from_connector: input_tip, to_connector: output_tip }
      - { from_connector: input_tip, to_connector: output_ring }
```

The 14 connector slugs:

```
a_tip  a_ring  b_tip  b_ring  c_tip  c_ring  d_tip  d_ring  e_tip  e_ring
input_tip  input_ring  output_tip  output_ring
```

Each of the 10 loops has a Tip and a Ring jack (effectively two
independent mono loops per physical port). The 4 fixed endpoints
(`input_tip`, `input_ring`, `output_tip`, `output_ring`) are the
device's main I/O.

### Simple vs Advanced

The two modes have different YAML shapes — Simple has a `chain:` of
linear hops with per-loop `bypass`, Advanced has a `connections:` graph
with no `bypass` field. Mixing them is a parse error (caught by the
schema and by serde), not a lint warning.

In **Simple mode**, the chain is linear; each hop can be bypassed
independently.

In **Advanced mode**, the routing is a graph (split, merge, multiple
outputs allowed) and **per-loop bypass is not supported** by the
current firmware. The encoder always sends segment 19 as `[0, 0, 0]`.

Default to Simple. Use Advanced only when the routing genuinely needs
split/merge or different content on the two outputs.

## Configuration

Put per-user defaults in `~/.ml10x.toml` (or pass `--config PATH`,
or set `$ML10X_CONFIG`):

```toml
[device]
port = "ML10X"
```

Command-line `--port` overrides the config; the config overrides the
built-in default of `"ML10X"`.

## Editor support

The YAML files include a `# yaml-language-server: $schema=...` header
pointing at the JSON Schema in this repo. The VS Code **YAML**
extension (Red Hat) reads this and gives autocomplete + validation on
connector slugs, modes, and field names with no extra setup.

If you'd rather configure schemas globally, add this to
`settings.json`:

```jsonc
{
  "yaml.schemas": {
    "./schemas/preset.schema.json": "**/bank-*/preset-*.yaml",
    "./schemas/controller.schema.json": "**/controller.yaml"
  }
}
```

## Output modes

All commands honour these global flags:

- `--verbose, -v`: extra detail in human output.
- `--quiet, -q`: errors only.
- `--json`: structured output on stdout. Useful for scripting; pipe
  to `jq`. Implies quiet for the prose lines.

## Exit codes

- `0`: success
- `1`: generic error
- `2`: usage error (bad flags, missing args)
- `3`: input file error (YAML invalid, file not found)
- `4`: device unavailable (port missing or ambiguous)
- `5`: encode error (preset invalid for its mode)
- `6`: sent successfully but no acknowledgement within timeout
- `7`: device sent a retry/error response

## Protocol

See `docs/sysex.md` for the wire format. In brief:

- 16-byte SysEx header: `F0 00 21 24 07 00 P1 P2 P3 P4 P5 P6 P7 P8 0 0`,
  then segments, then a checksum byte and `F7`.
- Inbound messages encode their total length in bytes 14, 15.
- Connect handshake is four outbound messages (`P2=0, 24, 19, 21`),
  then the device streams the active preset + controller dump
  unprompted.
- `P2=22, P3=bank` selects a bank; `P2=18, P3=preset` selects a preset.
- Simple preset write: `P1=6 P2=0 P3=127`, segments 0..13 carry 2- or
  3-byte `<from> <to> <bypass>` chain records.
- Advanced preset write: `P1=6 P2=2 P3=127`, segments are 1-byte
  `id=target.gn, data=[source.gn]` per connection.
- Device acks with `f1=1 f2=2` ("Preset Settings Saved!").

## Acknowledgements

- Morningstar Engineering for the ML10X and for publishing the MC6/MC8
  SysEx specification (the framing rules are largely shared with the
  ML10X).
- [`guyburton/morningstarmidi`](https://github.com/guyburton/morningstarmidi)
  for the original MC6/MC8 tooling that proved the YAML-over-SysEx
  pattern.

## License

GPL-3.0-or-later (inherited from upstream `morningstarmidi`, GPLv3).
