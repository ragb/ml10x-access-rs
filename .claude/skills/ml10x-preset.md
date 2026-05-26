---
name: ml10x-preset
description: Turn a natural-language description of a guitar effects chain into a Morningstar ML10X preset YAML, choosing Simple or Advanced mode automatically. Use this whenever the user describes a sound, a signal path, or a routing they want — phrases like "make me a clean tone", "build a preset with X then Y", "set up a stereo rig", "route the delay in parallel", "wire compressor → drive → reverb". The skill reads the user's controller.yaml to map pedal names to physical loop letters, picks Simple mode for any linear mono chain and Advanced mode only when stereo or non-linear routing is genuinely required, writes a preset YAML, and offers to sync it to the device.
---

# ml10x-preset — describe a sound, get a YAML

The user owns a Morningstar ML10X loop switcher and has a project at the current working directory that exposes a `ml10x` CLI plus dumped YAML files of their device. Your job is to take a sound/routing description in plain English and produce a valid preset YAML that lives in their project.

## Step 1 — locate the user's controller.yaml

A controller.yaml is the source of truth for which connector letter corresponds to which physical pedal. It's produced by `ml10x dump --out DIR` (or `--all`) and lives at `DIR/controller.yaml`.

Find it. In rough order of likelihood:

1. Ask if they have a preset directory already (most users do — they ran `ml10x dump` once).
2. Look under any `controller.yaml` in or under the working directory.
3. If none exists, run `ml10x dump --out ./presets` for them (fast — ~7 s, only writes the active preset + controller).

Once located, read it with the Read tool. Build a mental map: connector slug → pedal name. Example from a real rig:

```
a_tip   = Ego 76         (compressor)
a_ring  = archer Icon    (overdrive)
b_tip   = ODR Mini       (overdrive)
b_ring  = SD1            (overdrive)
c_tip   = AT+            (drive)
c_ring  = Big Muff       (fuzz)
d_tip   = Soul Press     (expression / wah)
d_ring  = Duke of tone   (overdrive)
e_tip   = OC-5           (octaver)
e_ring  = EQ-7           (EQ)
```

If the user says "compressor" you map to `a_tip` here (Ego 76). If they say "OC-5" or "octaver" you map to `e_tip`. **Always map by the user's controller.yaml**, never by these examples — every rig is different.

## Step 2 — pick the mode

**Use Simple mode when** the routing is a single linear chain through one Output:

- "compressor → drive → reverb → output"
- "input, then loop A, then loop C, then output"
- Any description that's a sequence with one input and one output.

**Use Advanced mode when ANY of these are true:**

- **Stereo**: the user mentions stereo, left/right, two outputs, Output Tip + Output Ring with different content. (One output mono = Simple; two outputs with different signal = Advanced.)
- **Parallel processing**: "X and Y in parallel", "wet/dry split", "blend the chorus", "send the delay to a separate amp".
- **Merge / split**: signal forks and rejoins, or splits to multiple destinations.
- **Separate inputs**: the user is using Input Tip *and* Input Ring as independent inputs (e.g. stereo guitar, or guitar + bass on the same device).
- **Output Tip and Output Ring have different content**: e.g. "dry signal on output tip, with reverb on output ring".

On the ML10X firmware as of v1.2, **Advanced mode does not support per-loop bypass**. If the user wants some loops bypassed, that's Simple mode only.

When in doubt: Simple is the default. Only escalate to Advanced when a Simple chain literally cannot express the routing.

## Step 3 — build the chain

The YAML schema (full reference: `schemas/preset.schema.json`):

```yaml
preset:
  bank: 1                 # 1..4
  number: 0               # 0..127
  name: My Preset         # max 16 chars
  mode: simple            # or "advanced"
  spillover:
    output_tip: nothing   # nothing | input_tip | input_ring | a_tip..e_ring
    output_ring: nothing
  chain:
    - from_connector: input_tip   # signal source for this hop
      to_connector: a_tip         # signal destination
      bypass: false               # Simple only; ignored in Advanced
    # … more hops …
```

(There is no `midi_messages` field on ML10X presets. That's an
MC-series feature; the ML10X firmware does not support per-preset MIDI
message lists. Don't add one.)

Connector slugs (14 total):

```
a_tip   a_ring   b_tip   b_ring   c_tip   c_ring
d_tip   d_ring   e_tip   e_ring
input_tip   input_ring   output_tip   output_ring
```

### Simple chain shape

One linear path. Order matters — write hops in signal-flow order:

```yaml
chain:
  - { from_connector: input_tip, to_connector: a_tip,      bypass: false }
  - { from_connector: a_tip,     to_connector: c_tip,      bypass: false }
  - { from_connector: c_tip,     to_connector: e_tip,      bypass: false }
  - { from_connector: e_tip,     to_connector: output_tip, bypass: false }
```

`bypass: true` on a hop means signal passes *through* that loop unchanged — the user has it wired in the chain but turned off for this preset. Honour it when the user explicitly says "with the drive off" or "compressor bypassed".

### Advanced patterns

Express graph topology as a flat list of edges. Order is less important than coverage. Examples:

**Stereo (mono in, stereo out):**

```yaml
mode: advanced
chain:
  - { from_connector: input_tip, to_connector: a_tip }
  - { from_connector: a_tip,     to_connector: c_tip }
  - { from_connector: c_tip,     to_connector: output_tip }
  - { from_connector: c_tip,     to_connector: output_ring }  # same source, both outs
```

**Wet/dry split (one input fans out):**

```yaml
mode: advanced
chain:
  - { from_connector: input_tip, to_connector: output_tip }       # dry path
  - { from_connector: input_tip, to_connector: e_ring }           # send to reverb
  - { from_connector: e_ring,    to_connector: output_ring }      # wet path
```

**Two parallel effects merged back:**

```yaml
mode: advanced
chain:
  - { from_connector: input_tip, to_connector: a_tip }
  - { from_connector: a_tip,     to_connector: c_tip }     # path A
  - { from_connector: a_tip,     to_connector: d_tip }     # path B (parallel)
  - { from_connector: c_tip,     to_connector: output_tip }
  - { from_connector: d_tip,     to_connector: output_tip }  # merge back at output
```

## Step 4 — spillover

Spillover keeps a single loop's audio ringing through the preset change — useful for delay/reverb tails you don't want cut off.

**Two layers to check:**

1. **`controller.yaml` → `include_in_trails`** must be `true` for the loop you want to spill. (This is global, set once. If it's `false` the per-preset spillover field is ignored.)
2. **The preset's `spillover.output_tip` / `output_ring`** — picks which loop's trail to preserve, per-output. Values: `nothing`, `input_tip`, `input_ring`, or any of the 10 loop slugs (`a_tip..e_ring`). Output endpoint slugs are NOT valid here.

**When to set it:**

- The user explicitly mentions trails ("let the delay tail spill over when I switch", "I want reverb trails between presets") — set spillover to the loop producing the trail.
- The chain ends in a time-based effect (delay, reverb, modulation that decays) and `include_in_trails` is on for that loop — default the spillover to that loop, since that's the whole point of enabling trails in controller settings.
- The chain is dry (compressor, drive, EQ, fuzz only) — leave spillover at `nothing`.

**Stereo loops:** when the trail-producing loop is stereo (both Tip and Ring carry the wet signal), set `output_tip: <loop>_tip` AND `output_ring: <loop>_ring`. Both sides of the stereo image keep ringing.

**Pick the loop closest to the output that's still trail-bearing.** If the chain is `comp → drive → delay → reverb → output`, spill the *reverb* (last in the chain), not the delay — that's what's actually decaying when the user steps away.

The two fields are `output_tip` and `output_ring` — one per physical output. Values: `nothing`, `input_tip`, `input_ring`, or a loop slug (e.g. `e_ring`).

## Step 5 — write the file

Path convention matches what `ml10x dump` writes:

```
<presets-dir>/bank-<N>/preset-<NNN>.yaml
```

Where `<N>` is 1..4 (matches the editor) and `<NNN>` is the preset number zero-padded to 3 digits (e.g. `preset-005.yaml`).

If the user hasn't specified a bank/preset, ask. Don't guess — overwriting the wrong slot loses the user's work. If they say "any empty slot", read the existing YAMLs in `<presets-dir>/bank-N/` to find one whose `name` is `Empty` (or whose file doesn't exist).

Always include the schema header at the top of new files so VS Code attaches autocomplete:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/ragb/ml10x-access/main/src/ml10x/schema/preset.schema.json
preset:
  …
```

The `ml10x` CLI emits this header automatically when it writes a YAML; do the same when you write one by hand.

## Step 6 — offer to sync

After writing the file, show the user a one-line summary of what you wrote and offer to send it to the device:

> "Wrote `presets/bank-2/preset-005.yaml`: Simple chain Input Tip → A Tip → C Tip → Output Tip. Sync now? (`ml10x sync presets/bank-2/preset-005.yaml`)"

Don't auto-sync. The user might want to read it first or test in `--dry-run`.

`ml10x sync` also accepts `controller.yaml` (writes the connector names + global settings) and a directory (syncs both controller and every preset under it in one device connection).

## Asking clarifying questions

Use `AskUserQuestion` (single-select or multi-select) when:

- The slot to write to is ambiguous (which bank, which preset number).
- The user mentioned a pedal name that doesn't match any controller.yaml entry — offer the closest matches.
- The routing description has multiple valid interpretations (e.g. "blend the delay" — parallel mix vs serial with bypass switching?).
- You inferred Advanced mode but Simple would do — quickly confirm.

Don't ask if the answer is obvious from context. A description like "compressor into drive into delay, mono" needs no questions — just write it.

## Worked examples

### Example A — Simple chain

User: "Make me a clean tone: compressor, then a touch of overdrive from the archer, then EQ at the end."

You read controller.yaml, see `a_tip = Ego 76` (comp), `a_ring = archer Icon` (od), `e_ring = EQ-7` (EQ). Mode: Simple (linear mono).

```yaml
preset:
  bank: 1
  number: 5             # ask user if not given
  name: Clean
  mode: simple
  spillover: { output_tip: nothing, output_ring: nothing }
  chain:
    - { from_connector: input_tip, to_connector: a_tip,      bypass: false }
    - { from_connector: a_tip,     to_connector: a_ring,     bypass: false }
    - { from_connector: a_ring,    to_connector: e_ring,     bypass: false }
    - { from_connector: e_ring,    to_connector: output_tip, bypass: false }
```

### Example B — Stereo

User: "Stereo rig — guitar into compressor and drive, then split for stereo reverb."

The "stereo reverb" implies two outputs with the same wet signal. The single guitar implies one input. Mode: Advanced.

```yaml
preset:
  bank: 2
  number: 3
  name: Stereo Verb
  mode: advanced
  spillover: { output_tip: nothing, output_ring: nothing }
  chain:
    - { from_connector: input_tip, to_connector: a_tip }      # comp
    - { from_connector: a_tip,     to_connector: c_tip }      # drive
    - { from_connector: c_tip,     to_connector: output_tip } # stereo L
    - { from_connector: c_tip,     to_connector: output_ring }# stereo R
```

### Example C — Wet/dry parallel

User: "I want my drive sound dry on one output and a delay-only signal on the other so my engineer can mix them."

Two outputs with different content → Advanced. The drive path goes to Output Tip; a parallel path from the input through the delay goes to Output Ring.

```yaml
preset:
  bank: 2
  number: 7
  name: Wet Dry
  mode: advanced
  spillover: { output_tip: nothing, output_ring: nothing }
  chain:
    - { from_connector: input_tip, to_connector: a_tip }       # comp (dry side)
    - { from_connector: a_tip,     to_connector: c_tip }       # drive
    - { from_connector: c_tip,     to_connector: output_tip }  # dry to FOH
    - { from_connector: input_tip, to_connector: e_tip }       # delay branch (clean input)
    - { from_connector: e_tip,     to_connector: output_ring } # wet to FOH
```

## Hard rules — the CLI's validator will reject violations

These mirror what `ml10x lint` / `ml10x show` enforce. Honour them as
you build the YAML so the user doesn't see a wall of errors.

**Structural (from the JSON Schema):**

- `bank` is 1..4. `number` is 0..127.
- `name` is at most 16 ASCII characters.
- `mode` is exactly `simple` or `advanced`.
- Every `from_connector` and `to_connector` is one of the 14 slugs
  listed above — typos are caught instantly.
- Each `spillover.output_tip` / `output_ring` is one of:
  `nothing`, `input_tip`, `input_ring`, `a_tip`, `a_ring`, `b_tip`,
  `b_ring`, `c_tip`, `c_ring`, `d_tip`, `d_ring`, `e_tip`, `e_ring`.
  (Note: output endpoints are NOT valid spillover targets.)

**Semantic (the rules the schema can't express):**

1. **Output endpoints can't be a source.** `output_tip` and
   `output_ring` cannot appear as `from_connector` on any hop.
   Signal flows TO outputs, never FROM them.
2. **Input endpoints can't be a destination.** `input_tip` and
   `input_ring` cannot appear as `to_connector` on any hop. Signal
   flows FROM inputs, never TO them.
3. **No self-loops.** `from_connector == to_connector` is rejected.
4. **In Simple mode the chain must be linear.** Each connector can
   appear AT MOST ONCE as a `from_connector` and AT MOST ONCE as a
   `to_connector`. If the user's description needs branching or
   merging (split, parallel paths, two outputs with different signal),
   use Advanced mode.
5. **In Advanced mode, `bypass: true` is ignored by the firmware.**
   Always emit `bypass: false` (or omit it) on Advanced presets.
   Per the ML10X manual: "Loops cannot be bypassed (using CC
   messages) in Advanced mode."
6. **At least one input must reach an output.** Walk the chain
   forward from `input_tip`/`input_ring` along `to_connector` links
   — if no path lands on `output_tip` or `output_ring`, no audio
   flows. The CLI flags this as a warning, but it almost always
   means the description was incomplete.

**Soft suggestions:**

- Don't include the same `(from, to)` hop twice — duplicates are a
  warning and have no effect.
- If you reference a connector whose name in `controller.yaml` is
  blank or "Empty", the user probably doesn't have a pedal there;
  confirm before assuming.

After writing the YAML, **run `ml10x lint <path>`** (or
`ml10x show <path>` for a single file) to confirm — both layers of
validation run. If anything is reported as `[error]`, fix it before
suggesting `sync`. Warnings can be left in but should be mentioned
to the user.
