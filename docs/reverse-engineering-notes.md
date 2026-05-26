# Reverse-engineering the ML10X: techniques

Notes-to-self on how we cracked the ML10X SysEx protocol and built a
working CLI editor without docs from the vendor. This is the methodology
section — protocol facts themselves live in `docs/sysex.md`.

The project had two interleaved exploration tracks: **the editor**
(a closed-source Angular SPA that already talks the protocol) and
**the device** (a USB MIDI loop switcher that is the source of truth).
We treated each as a black box and probed them in different ways. The
two streams cross-checked each other: a hypothesis from the editor's
JS could be verified by watching the device's actual response, and a
mystery byte from the device could often be identified by reading what
the editor's UI displayed for it.

## 1. Editor exploration

The editor is `editor-mkii.morningstar.io/ml10x`, ~7.8 MB minified
Angular bundle, uses the Web MIDI API. Everything below was done from
the chrome-devtools MCP — no DevTools clicking by hand.

### 1.1 Drive the editor via the chrome-devtools MCP

We added the official `chrome-devtools` MCP at user scope:

```bash
claude mcp add -s user chrome-devtools -- npx chrome-devtools-mcp@latest
```

From there a Claude session could:

- `mcp__chrome-devtools__navigate_page` with an `initScript` that runs
  before any page code (the critical capability — see §1.2).
- `mcp__chrome-devtools__evaluate_script` for arbitrary in-page JS,
  including async functions that wait and return Promises.
- `mcp__chrome-devtools__take_snapshot` for an accessibility-tree view
  of the page (way more compact than raw DOM and good enough to find
  controls).
- `mcp__chrome-devtools__list_network_requests` and
  `mcp__chrome-devtools__get_network_request` to grab the JS bundle.

A typical interaction: `new_page → wait_for("known text") →
evaluate_script("find element by text and click") →
wait_for("loaded indicator") → evaluate_script("collect state")`.

### 1.2 Web MIDI interception via initScript

The big win was the `initScript` option on `navigate_page`. It runs
before the page's own scripts, so we could patch `navigator.requestMIDIAccess`
before the editor ever called it:

```js
const origReq = navigator.requestMIDIAccess.bind(navigator);
navigator.requestMIDIAccess = function(opts) {
  return origReq(opts).then(access => {
    for (const out of access.outputs.values()) patchOut(out);
    for (const inp of access.inputs.values()) patchIn(inp);
    access.addEventListener('statechange', e => { ... });
    return access;
  });
};
```

`patchOut` wraps `output.send` to push every outbound message into a
global array. `patchIn` uses `Object.defineProperty` to intercept the
*setter* on `onmidimessage` — the editor assigns its handler there, and
we wrap that handler so we see every inbound message too.

Crucially we didn't fake the MIDI access — we patched the real one.
Chrome still gave the device permission, the editor still talked to the
device, and we got a perfect tee of the wire.

### 1.3 Phase-tagged capture

Each captured message gets a `phase` field that we manually set before
each operation:

```js
window.__ml10x.phase = 'save_preset_2_simple';
saveButton.click();
// ...
window.__ml10x.phase = 'toggle_to_advanced';
modeButton.click();
window.__ml10x.phase = 'save_preset_2_advanced';
saveButton.click();
```

Later, slicing the timeline by phase isolated exactly what each user
action emitted — no time-window guessing.

### 1.4 Bundle source grepping

The Angular bundle is **minified but not obfuscated** — one line,
~7.8 MB, no sourcemap published (we checked). Class and variable
names are mangled (`oi`, `la`, `Kb`, `bt`, ...) but everything else
that matters is intact:

- String literals survive minification: every `console.log` message,
  every reflection lookup like `this[String(x)]`, every `"key"` used
  in `Foo["key"] = Foo[Foo.key=N]`. So `"REQUEST_PRESET_NAMES"`,
  `"Preset Settings Saved!"`, `"matrixArray"`, the entire 47-entry
  message-type enum all read in plain text.
- TypeScript's `enum` lowering is a recognisable pattern:
  `Foo[Foo.X=0]="X"`. Found one entry → regex out the whole enum.
- Constructor field initialisers are unmangled on the right-hand side:
  `this.bypassLoopStatus=0,this.matrixArray={},this.isAdvancedMode=!1,...`.
  Reading them off `class Kb` gave us the canonical preset field list
  in five seconds.

The cost of minification: no `grep -C` context (everything is on one
line). Worked around with Python: `data[max(0,j-200):j+200]` after
locating an offset. Five lines to write, gives the same effect as
context-grep.

We didn't beautify the bundle (`prettier`/`js-beautify`), partly
because the slicing approach was already in hand and partly because
the byte offsets we recorded in `docs/sysex.md` would have shifted.
If you redo this on a different project: consider beautifying once,
up front, and grepping the beautified copy — but keep the original
for byte-offset references.

We saved it via `get_network_request(reqid, responseFilePath=...)` and
then grepped. The patterns below all come from the ML10X bundle as it
shipped on 2026-04-01.

**Find by distinctive string literal.** The minifier leaves these
intact, so `"REQUEST_PRESET_NAMES"`, `"Preset Settings Saved"`,
`"matrixArray"`, `requestMIDIAccess` all locate the surrounding code
in one regex:

```python
m = re.search(r'requestMIDIAccess', data)
print(data[m.start()-100 : m.start()+300])
```
yields:
```
.isBrowserRejected&&this.refreshMidiDevices()}refreshMidiDevices(){this.clearMidiInputList(),
this.clearMidiOutputList(),this.isMidiAvailable=!1,this.isBrowserRejected=!1,
navigator.requestMIDIAccess?(navigator.requestMIDIAccess({sysex:!0}).then(...
```

**Find by byte sequence in decimal.** The bundle stores byte literals
as decimals, not hex. The Morningstar manufacturer ID `0x21 0x24` is
the string `"33,36"`; the full ML10X prefix `0x00 0x21 0x24 0x07` is
`"0,33,36,7"`:

```python
re.findall(r'0,33,36,7', data)   # locates the ML10X sysexBuilder constructor
re.findall(r'33,36',     data)   # 4 hits across the bundle
```

**Find class definitions by name once you have one method.**

```python
m = re.search(r'class Kb\{constructor', data)
print(data[m.start() : m.start()+400])
```
yields the canonical preset field list in a single line:
```js
class Kb{constructor(d,i=0){this.presetName="",this.presetNumber=0,
this.mutedSwitchOption=0,this.tipTrails=0,this.ringTrails=0,
this.hasLoadedOnce=!1,this.bankNumber=0,this.connectionArray=[],
this.matrixArray={},this.isAdvancedMode=!1,this.bypassLoopStatus=0,
this.presetNumber=d,this.bankNumber=i}
```
That's every field on the Preset class. Five seconds of grepping, no
debugger.

**Pluck whole enums via the TS-emit pattern.** TypeScript's `enum`
lowers to `Foo[Foo.X=N]="X"`. Two recognisable forms appear in the
ML10X bundle:

```js
// const-style enum (the message-type enum in module 6146):
const C = {
  M_EMPTY:0, M_PCMSG:1, M_CCMSG:2, M_NOTEON:3, M_NOTEOFF:4,
  M_REALTIME:5, M_SYSEX:6, M_CLOCKTAP:7, M_PC_SCROLL_UP:8, ...
};

// IIFE-style numeric enum (the bt function-code enum):
var bt = function(l) {
  return l[l.DUMMY=0]="DUMMY",
         l[l.ENGAGE_PRESET=29]="ENGAGE_PRESET",
         l[l.ENGAGE_EXP=30]="ENGAGE_EXP",
         l[l.REQUEST_CONTROLLER_SETTINGS_ALL=35]="REQUEST_CONTROLLER_SETTINGS_ALL",
         ...
}(bt||{});

// Same form used for the BZ header-position enum:
var mi = function(bt) {
  return bt[bt.ARRAY_START=16]="ARRAY_START",
         bt[bt.SYSEX_START_POS=0]="SYSEX_START_POS",
         bt[bt.MANF_ID_1_POS=1]="MANF_ID_1_POS",
         ...
         bt[bt.FUNCTION_ID_8_POS=13]="FUNCTION_ID_8_POS",
         bt;
}(mi||{});
```

One regex extracts all (name, value) pairs of an IIFE enum:

```python
pairs = re.findall(r'l\.(\w+)=(\d+)', enum_source)  # for the bt form
# returns [('DUMMY','0'), ('ENGAGE_PRESET','29'), ...]
```

**Look at method bodies — they're short and readable.** The whole
checksum function:

```js
function qi(l) {
  let d = l[0], i = l.length - 2;
  for (let o = 1; o < i; o++) d ^= l[o];
  return d &= 127, d;
}
```

The whole SysEx header builder (every byte position made explicit):

```js
prepareHeader(d,i,o,s,h,x,B,z,ne) {
  return d[0]=240,
         d[1]=this._id1, d[2]=this._id2, d[3]=this._id3,
         d[4]=this._modelId,
         d[5]=0,
         d[6]=127&i, d[7]=127&o, d[8]=127&s, d[9]=127&h,
         d[10]=127&x, d[11]=127&B, d[12]=127&z, d[13]=127&ne,
         d[14]=0, d[15]=0,
         this._currentPointer=16,
         d;
}
```

**The most valuable grep of the project** — locating where the editor
encodes a chain hop's bypass:

```python
m = re.search(r'addData\(x\.groupNumber,3', data)
print(data[m.start()-30 : m.start()+150])
```
yields:
```js
this.sysexBuilder.addData(x.groupNumber, 3,
    [x.groupNumber, B.groupNumber, B.isActive ? 0 : 1]
)
```

That single line told us three things at once: (a) segment id =
`x.groupNumber` (the from connector), (b) data is
`[from, to, bypass]`, (c) the bypass byte is `0` when the connection
is active, `1` when bypassed. The hardware probe (toggle one bypass,
diff bytes) only confirmed what was already in the source.

Recording the byte-offset of each find as a "source pointer" let us
revisit specific code without re-grepping. They're in `docs/sysex.md`.

### 1.5 Hooking `URL.createObjectURL` for the Device Backup JSON

The editor's Device Backup tab uses `file-saver` (which wraps Blob into
`URL.createObjectURL(blob)` + a hidden `<a>` element click). Patching
`HTMLAnchorElement.prototype.click` wasn't enough — `file-saver` does
something slightly different that bypassed our hook. So we patched the
constructor at the root:

```js
const orig = URL.createObjectURL.bind(URL);
URL.createObjectURL = (obj) => {
  const url = orig(obj);
  if (obj instanceof Blob) {
    obj.text().then(text => {
      window.__ml10x.downloads.push({ url, content: text });
    });
  }
  return url;
};
```

This caught any download the editor ever triggered. The Device Backup
JSON (160 KB, all 512 presets + controller settings in editor-native
schema) became a Rosetta Stone for the protocol — field names like
`matrixArray`, `tipT`, `ringT`, `bypassLoopStatus` confirmed exactly
what each SysEx segment was for.

### 1.6 Reading the editor's UI as an oracle

When we had bytes in hand but no idea which setting they represented,
the *displayed* value in the editor's Controller Settings tab told us
directly. E.g. segment 32 had value `09`; the page showed
"MIDI Channel: 9". Done — segment 32 is MIDI channel. No toggle-and-diff
needed. Just `evaluate_script` the page and read every form value's
displayed text, match against the bytes.

For the connector "Enable Spillover" toggles (10 per-loop switches),
we walked the DOM for `mat-slide-toggle` elements next to each loop's
"Enable Spillover" label and read the `aria-checked` attribute. The
captured 14-bit `include_in_trails` bitmap = `00 00` confirmed: all
toggles off.

This trick worked because the editor is a faithful mirror of device
state — it doesn't make values up; everything you see was decoded from
real bytes the device sent.

## 2. Device exploration

### 2.1 Capture → toggle → save → diff (one variable at a time)

The standard probe: figure out a byte's meaning by changing exactly
one thing in the editor, saving, comparing.

```
1. Connect (mock or real). Hook MIDI in+out.
2. Set phase = "baseline". Click Save Preset.
3. Set phase = "after_change". Click the ONE element you want to vary
   (toggle, dropdown, checkbox). Click Save Preset.
4. Set phase = "restore". Reverse the change. Click Save Preset.
5. Disconnect.
6. Decode the captured saves and diff segment-by-segment.
```

The capture is always tagged by phase so you can slice it later.
Steps 4-5 leave the device exactly as you found it — we used this
pattern dozens of times without damaging user data.

Example: clicking "A Tip" in the chain visualization flipped exactly
ONE byte: segment id 9 (A Ring's groupNumber) data went from
`09 04 00` → `09 04 01`. That nailed the third byte as bypass.

### 2.2 The mock-then-real pivot

Our first attempt at sending custom SysEx used a *fully fake* MIDI
device: a `navigator.requestMIDIAccess` shim that returned synthetic
input/output ports we controlled. The plan was to talk to the editor
as if we were the device, capturing its outbound and replying with
our own bytes.

It didn't work. The editor got stuck in a retry loop because our fake
replies didn't satisfy the device's expected message length encoding
(bytes 14–15 of inbound messages). We didn't yet know that field
existed.

Solution: **pivot to the real device.** Stop pretending to be the
device; let the editor talk to the actual ML10X over USB, and just
tee the wire. The mock had been useful for understanding the
*outbound* format (you can see the editor's saves whether the device
exists or not), but the *inbound* format is the device's prerogative
to define, and there's no substitute for the real thing.

Lesson: **mocks for what you control, real systems for what they
define.**

### 2.3 Mode-toggle as a free format converter

To learn the Advanced-mode WRITE format, we used this trick: take a
preset that already has a meaningful chain in Simple mode (the user's
"Clean" preset, 4-hop chain), and ask the editor to convert it to
Advanced for us:

```
1. Navigate to Clean. Save. -> captures Simple WRITE (102 bytes)
2. Click the SIMPLE/ADVANCED toggle.    -> editor converts internally
3. Save.                                 -> captures Advanced WRITE (60 bytes)
4. Toggle back to SIMPLE. Save.          -> restore
```

Same data in two formats, side by side. The diff told us:

- P2 byte changes (0 → 2)
- Connection records change from `<from> <to> <bypass>` (3 bytes,
  segment id = from) to `<source>` (1 byte, segment id = target)
- Segment 19 (bypass bitmap) emits `00 00 00` even though the Simple
  version had a bypass flag set — confirming the manual's note that
  Advanced has no per-loop bypass.

This works because the editor is a free format converter we can
script.

### 2.4 Byte-exact round-trip as the ground-truth test

Once we had any captured editor save, we made it a pytest fixture and
wrote a round-trip test:

```python
original = bytes(captured_save["bytes"])
preset = decode_preset(original, bank=..., number=...)
encoded = encode_simple_preset(preset)
assert encoded == original
```

If our decoder and encoder are correct, the encoder's output for the
decoded model is byte-identical to the editor's. This caught:

- Wrong segment IDs (off-by-one between `value` and `groupNumber`).
- Wrong segment ORDER (the device's parser is order-sensitive in
  Simple mode).
- Wrong byte VALUES (e.g. spillover encoded with the wrong sentinel).
- Wrong segment INCLUSION (we initially emitted unrouted markers for
  Output Tip and Input Ring; the editor doesn't, and the device
  rejects messages that do).

When the round-trip fails, the diff at the first mismatching byte
points right at the bug. No flakiness, no hardware-in-the-loop, runs
in 50 ms in CI.

### 2.5 Live hardware as the final check

Round-trip tests prove "our encoder matches the editor's bytes". They
don't prove "the device accepts the message." For that we send our
encoded bytes for real and watch for the `f1=1 f2=2` ack
("Preset Settings Saved!"). Twice in this project a message that
passed the round-trip test failed on the device, both because of
opcode mistakes:

1. Our "navigation" message used opcode `P2=32, P3=preset, P4=bank`.
   That turned out to be "Paste Preset to all banks" — a destructive
   operation we'd been firing repeatedly. Found by grepping the
   bundle for the actual `sendSysexFunction(0,32,...)` callsite and
   reading what the surrounding code did.
   Fix: navigation is `(P2=22, P3=bank)` then `(P2=18, P3=preset)`.
2. Even after fixing the opcode, the device wouldn't ack immediately
   after navigation. The fix: wait for the device's response stream
   to go idle (multiple seconds, not just 200 ms) before sending the
   write.

These are exactly the kind of subtleties no document captures and no
unit test catches — you have to send real bytes to a real device.

### 2.6 Walking all 512 presets robustly

The editor populates only the *active* preset over the wire; the other
511 sit on the device until you navigate to them. To dump everything
we send the navigation pair `(P2=22, P3=bank)` then
`(P2=18, P3=preset)` for each slot, wait for the preset data response,
decode, write YAML, repeat.

Naïve sequential code worked but missed ~3 presets at the start of
each bank, because the device streams a flood of data after a bank
change (controller dump, all-preset-names dump, 128 progress messages)
and the first preset-select races with it. The fixes:

- Wait for ≥6 s of idle after bank-select before sending the first
  preset-select.
- Retry each preset-select up to 2 times on timeout.

That brought us to 512/512 in ~2 minutes.

## 3. Cross-cutting techniques

### 3.1 Test fixtures as the project's memory

Every interesting capture went into `tests/fixtures/` as JSON. This
turned out to be the highest-leverage artifact in the project:

- `real-device-connect-bank0.json` — the initial 134-message dump
  including controller settings, the active preset, and the per-bank
  all-preset-names list.
- `real-device-save-preset-0.json` — the captured Save Preset on
  preset 0 (Base) unmodified. Single 98-byte message we round-trip
  against.
- `real-device-bypass-toggle-probe.json` — three saves (baseline,
  after-toggle, restore) of preset 2 with one connector's bypass
  flipped. The one-byte diff was the breakthrough.
- `real-device-advanced-mode-probe.json` — same preset saved in
  Simple, then Advanced, then Simple again.
- `real-device-clean-advanced-probe.json` — a 4-hop chain in
  Advanced format (so the matrix segments aren't trivially empty).
- `editor-device-backup.json` — the 160 KB editor JSON, schema-true
  representation of all 512 presets + controller.

Tests are written against these. The hardware is no longer in the
loop for any non-end-to-end check.

### 3.2 The bundle as a Rosetta stone

Once the manufacturer ID and a few class names were identified, the
bundle stopped being intimidating. Specific patterns we used
repeatedly:

- **Enum extraction**: TypeScript-style enums lower to
  `Foo[Foo.X=0]="X"`. We parsed all 47 entries of the message-type
  enum (`M_PCMSG=1, M_CCMSG=2, ...`) and the function-code enum
  (`REQUEST_PRESET_NAMES=64, ...`) with `re.findall(r'l\.(\w+)=(\d+)')`.
- **Class structure**: Constructor field initializers
  (`this.foo=0, this.bar=...`) revealed every property a class
  carries. `class Kb` gave us the canonical preset field list:
  `presetName, presetNumber, tipTrails, ringTrails, mutedSwitchOption,
  bypassLoopStatus, isAdvancedMode, matrixArray, connectionArray`.
- **Serialization callsites**: Searching for the SysEx builder method
  names (`addData(`, `addByte(`, `startBuild(`) showed every place
  the editor emits bytes, and the arguments told us segment ids and
  data shapes.
- **Switch-case dispatchers**: The device-message parser is a switch
  on `f1` (P1 byte). Finding it told us every inbound message class
  in one shot. Same for the preset-data sub-switch on `f2`.

### 3.3 Driving Angular state without drag-and-drop

Some editor controls are mouse-driven (drag-and-drop to wire up
connections in Advanced mode). Rather than simulating mouse events,
we found higher-level entry points:

- The mode toggle is a regular `<button>` with click handler — clickable.
- Each "A Tip" / "A Ring" / etc. button in the chain is a regular
  button that toggles the bypass — clickable.
- The Save Preset button is just a button.

Where a control was a dropdown (`mat-select`), reading the
`displayedValue` of `.mat-mdc-select-value` gave us the current
selection without needing to open the dropdown. The DOM was usually
the path of least resistance.

## 4. What we'd do differently

- **Capture more, capture earlier**. The Device Backup JSON
  (`editor-device-backup.json`) would have answered most of our early
  questions if we'd grabbed it first. Don't be afraid to ask the
  editor for an export and grep the result.
- **Read the actual handler code, not the action that triggers it.**
  We wasted time assuming `sendSysexFunction(0, 32, ...)` was
  navigation because the name was vague. Reading the surrounding code
  (it was inside the "Paste Preset to all banks" handler) would have
  saved a wrong opcode probe against the user's device.
- **Real wins early, polish late.** The earliest "this works" moment
  was getting the device to ack a save. Everything else
  (chain ordering, Advanced mode, full backup) sat on top of that
  one verified primitive. Build a small thing that elicits a real
  response, then expand.

## 5. Specific patterns worth keeping

| Need | Tool |
| --- | --- |
| Intercept Web MIDI in a third-party SPA | `initScript` patching `navigator.requestMIDIAccess`. |
| Tee `<a download>` blob downloads | Patch `URL.createObjectURL` to read the Blob into a global. |
| Read displayed UI state | `mcp__chrome-devtools__take_snapshot` (accessibility tree). |
| Find which DOM control matches a setting | Walk H4 → next .row sibling → `mat-select-value`. |
| Extract a minified TS enum | `re.findall(r'l\.(\w+)=(\d+)')` after locating one entry. |
| Pin a hypothesis | Capture both states, diff segment-by-segment. |
| Prove device acceptance | Look for the `f1=1 f2=2` ack on inbound. |
