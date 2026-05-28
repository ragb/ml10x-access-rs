# Handshake reply quirks

Things consumers of `ml10x_core` should know about what the device sends
back after the 4-message connect handshake. The codec doesn't paper over
any of this — it just decodes whatever bytes arrive — so each consumer
ends up rediscovering the same gotchas. Writing them down here.

## Each "case 6" message arrives twice

Already noted at [docs/sysex.md:199](sysex.md): the device emits the
Preset / Controller / All-preset-names dumps **twice** in its handshake
response. This is presumably a sync mechanism for the official editor.

The first copy of the preset can be a stale snapshot — sometimes
indistinguishable from an empty / default preset (no name, empty Simple
chain). The second copy is the authoritative version that matches what
the device is actually playing.

Empirically observed against firmware v1.2 (browser editor via Web MIDI,
2026-05-28): a freshly-connected device can return a stub preset on the
first reply, and the second reply doesn't always arrive within the
window a UI consumer might naively trust the first one.

## What to do about it

Three options for consumers, in increasing order of effort:

1. **Wait and dedupe.** Buffer inbound preset / controller / preset_names
   messages for a settling window (~1.5 s of quiet after the last
   lifecycle EditorEvent) and use the last one of each kind that
   arrived. This is what the official editor appears to do.

2. **Re-fetch on demand.** Once you've received the first preset reply,
   send `encode_navigate_to(bank, number)` to the same slot. The
   device's reply to an explicit navigate-to is always the
   authoritative version — never a stub. Trade-off: one extra
   round-trip per connect, plus another loading_progress sub-burst.

3. **Trust the first reply.** Works most of the time on populated
   slots. Fails silently when the first reply is a stub. Don't do this
   for an interactive editor.

The `able-midi` browser editor uses option (2) — it issues a
`navigate_to(current_bank, current_number)` ~500 ms after the first
preset reply. See `src/lib/stores/ml10x.svelte.ts`.

## Each handshake P2 code triggers its own internal sub-load

The 4 outbound P2 codes (`0, 24, 19, 21`) appear to be independent
"open editor" steps, not one atomic command. Each triggers a separate
internal load on the device, observable as a `loading_start →
loading_progress* → loading_end` lifecycle on the inbound stream. So
during a single connect the host sees multiple
start-progress-end cycles, each restarting P3 from a low value.

UI consumers showing a progress bar should treat these as one
continuous load (span the whole burst with a high-water-mark and an
idle window), not reset the bar between cycles.

## Loading lifecycle codes (P1 = EditorEvent)

Decoded in `crates/ml10x/src/commands/helpers.rs`:

| P2 | Meaning                                                |
|---:|--------------------------------------------------------|
| 0  | device connected (snackbar code 0)                     |
| 2  | preset saved — ack for a successful preset write       |
| 5  | loading start                                          |
| 6  | loading end                                            |
| 7  | loading progress; P3 carries the value (max 100)       |

These are useful for a UI's loading-state machine and for confirming
a save committed.

## EditorEvent P2 codes after the burst settles

After the handshake burst is done, the device continues to emit
EditorEvent messages occasionally — at least P2 = 7 with the same P3
value, plus the lifecycle P2 = 5 / 6. Consumers that key their
"loading done" state off the absence of EditorEvent traffic will
trip over this. The reliable signal that the load is done is having
the minimum required data (a Preset reply + a Controller reply, plus
ideally a PresetNames reply for the bank you want to display), not the
quiet of the lifecycle channel.
