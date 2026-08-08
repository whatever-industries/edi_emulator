# Mono-I SLAVE MCU protocol — reverse-engineering notes

Source: disassembly of the SLAVE firmware dump `zx405042p` ("cdi slave 2.0",
MC68HC705C8A, 8 KB) — see `slave-zx405042p.disasm.asm`, produced with
`scripts/hc05dis.py`. These are original notes documenting facts extracted
from the firmware; see NOTICE.md for the copyright basis.

Status: PARTIAL — the MAME-ch2 immediate-command dispatch, including the
disc-play `0x8A` host reset, is decoded, and the BIOS-side post-reset
`B0`/`B1` conversation is sufficient to launch a real title. The four shared
SLAVE flag bytes are now cross-referenced and the matching SERVO 2.1 firmware
has an address-stable disassembly and packet map. The remaining work is
tracing physical drive transitions into the retained-mode bit and replacing
HLE drive status with the complete SERVO-derived state machine.

## Hardware interface (pin/port map)

| HC05 resource | Role |
|---|---|
| Port A (`$00`), DDRA `$04` | Bidirectional host data bus (68070 D0-D7) |
| Port B (`$01`) bit 6 | Host handshake/ack strobe (toggled around every transfer) |
| Port C (`$02`) bits 0-1 | Channel address from 68070 (A1/A2): selects channel 0-3 |
| **Port C bit 2** | **68070 RESET line** (see host-reset routine) |
| Port C bit 6 | Control output driven by ch3 cmd `0xFC` (released by `0xFD`); sets `$BA=2` first — possibly drive/CDIC power or reset |
| Port C bit 7 | Output toggled by SCI paths gated on `$5A` bit 1 (set by cmd `0x8A` family) |
| Port D (`$03`) bit 7 | Host R/W direction |
| SCI (vector `$067A`) | Serial link to the SERVO MCU (`zx405037p`) |
| Vectors | RESET=`$0100`, IRQ=`$032F` (host bus access), TIMER=`$0961`, SCI=`$067A` |

## Response queue (SLAVE → host)

- Ring buffer at `$87..$8E` (8 entries), write index `$6B`, read index
  `$6F`, pending count `$D9`, last-sent byte `$5C`.
- Enqueue routine `0x0ED0`: dedups (skips if equal to `$5C` or `0xFF`);
  variant at `0x0EF2` without dedup. Host reads pop via the IRQ read path
  (`0x0335`/`0x0359`).
- Spontaneous status byte `0x3B` enqueued at `0x0EAE` when `$58` bit 3 is
  clear (gated on `$55` bit 1 / `$5A` bit 6). BIOS module `ckeydriv` owns
  this channel-1 stream and only acts on `0xA0..0xA3`; `0x3B` is therefore
  not the disc-launch notification. It remains an unidentified input/status
  event.

## Host reset — the key to the PLAY CD-I flow

Routine `0x0F0C`: stores `#$FF → $75`, `JSR $0ABF`, then **loops forever
driving Port C bit 2 high** (`BSET2 $02; BSET2 $06`) — i.e. the SLAVE
holds the 68070 in reset. Reached from `0x0EBC` when **`$55` bit 1 is SET
and `$5A` bit 6 is CLEAR**.

The 68070 boot stub writes its final transport command, then executes
`BRA.s self` at ROM `0x400636` — it is literally waiting for the SLAVE to
reset it. The SLAVE's own RAM (mode flags below) survives the host reset;
the freshly-booted ROM queries the SLAVE and, seeing "play disc" state,
boots the disc instead of the shell. **Emulator implication: the SLAVE HLE
must expose a host-reset request; the Machine performs a full reset of
CPU+devices while PRESERVING SlaveHle state.**

## Channel dispatch (host writes, IRQ `$032F`)

`$03` bit7 selects read/write; `$02` bits 1:0 select the handler:

| `$02` bits (A2,A1) | MAME channel | Handler | Notes |
|---|---|---|---|
| 00 | ch0 | `0x038F` | table `0x0624` (below) + buffered `0xC0+` (mouse etc.) |
| 01 | ch1 | `0x03C0` | not yet decoded |
| 10 | ch2 | `0x048D` | payloads at `$75`; commands `0x80..0x93` dispatch at `0x0512` (decoded below); `0xC0+` starts a four-byte payload |
| 11 | ch3 | `0x03DA` | matches MAME's known ch3 command set (see below) |

### ch0 immediate commands (BRA table `0x0624`, cmds `0x80-0x8C`)

All are RAM flag toggles consumed by the main loop:

| Cmd | Effect |
|---|---|
| 0x80 | `$55` bits 4,5 := 0,0 (3-state mode select with 0x81/0x82) |
| 0x81 | `$55` bits 4,5 := 0,1 |
| 0x82 | `$55` bits 4,5 := 1,0 |
| 0x83 / 0x84 | set / clear `$63` bit 4 |
| 0x85 | clr `$55.3`, set `$55.2` (pair with 0x86) |
| 0x86 | clr `$55.2`, set `$55.3` |
| 0x87 / 0x88 | clear / **set `$55` bit 1 — arms the host-reset condition** |
| 0x89 | set `$5A.0`, clr `$5A.1`, clr DDRC.7 |
| 0x8A | set `$5A.1`, clr `$5A.0`, set DDRC.7 (drives PC7) |
| 0x8B / 0x8C | **clear / set `$5A` bit 6 — reset gate** (bit 6 clear ⇒ reset can fire) |

(Note: these are the *ch0* semantics; the boot flow's observed `0x8A` went
to MAME-ch2, whose semantics are different.)

### ch2 immediate commands (`0x0512`, cmds `0x80-0x93`)

The handler first shifts the command left.  Commands `0x80..0x93` therefore
index the two-byte BRA table at `0x051D`; commands `0xC0+` branch to the
payload decoder at `0x04AD`.  Values `0x94..0xBF` are ignored.  The BRA
table contains a second layer of absolute JMPs at `0x058C..0x05B3`.

| Cmd | Target | Firmware effect |
|---|---:|---|
| 0x80 / 0x81 | `0x0545` / `0x0548` | set / clear `$56.1` |
| 0x82 | `0x054B` | if `$56.0` is clear, set it and latch timer byte `$1A` into `$64`; `$53.4` marks the latch valid (with modulo-`0x34` adjustment on an existing latch) |
| 0x83 / 0x84 | `0x0564` / `0x0578` | if `$56.0` is set, clear it, optionally drive Port C bit 7 low / high when `$5A.1` is set, then update the `$1A`/`$64` timing latch |
| 0x85 | `0x05B6` | when `$59.1` is set, select the decrement direction in `$D6` (`bit6=1, bit7=0`), decrement its low-six-bit position unless already zero, copy `$D8→$D7`, clear `$55.7`, set work flag `$54.2` |
| 0x86 | `0x05C7` | when `$59.1` is set, select the increment direction in `$D6` (`bit7=1, bit6=0`), increment its low-six-bit position up to `0x2D`, then the same `$D8→$D7`/work scheduling as `0x85` |
| 0x87 | `0x05E1` | clear both direction bits `$D6.6/.7`; set query/status flag `$5D.5` |
| 0x88 | `0x05F5` | toggle `$59.1` |
| 0x89 | `0x05E8` | set `$58.6` and `$57.5` (consumed by the main-loop/SERVO paths) |
| **0x8A** | **`0x0604`** | **unconditional `JMP $0F0C`: request/hold the 68070 in reset; no response byte** |
| 0x8B / 0x8C | `0x0607` / `0x060A` | set / clear `$56.4` |
| 0x8D / 0x8E | `0x060D` / `0x0616` | enable / disable response-queue notification via `$63.5`; enabling with bytes already queued also sets `$54.5` |
| 0x8F | `0x0619` | set work flag `$54.2` and toggle `$55.7` |
| 0x90 | `0x05ED` | if `$59.1` is set, set packet-receive state `$59.0` and clear byte count `$6C`; the next two ch2 bytes are accepted and the second (unless `0xFF`) is stored in `$D8` |
| 0x91 | `0x0563` | no-op |
| 0x92 / 0x93 | `0x05FE` / `0x0601` | set `$59.2` / `$59.3`; main-loop consumers at `0x0D5A` / `0x0D6B` clear these and drive the associated SERVO/status sequence |

This resolves the shell launch endpoint: the observed MAME-ch2 `0x8A`
does not merely arm the older `$55.1 && !$5A.6` reset condition.  It calls
the same reset routine directly.  An HLE should latch a host-reset request
on receipt of ch2 `0x8A`, retain its SLAVE mode/flag state, and let the
machine reset the 68070 and the other host-side devices at an instruction
boundary.

The CD-i 220 service manual's MMC signal list independently confirms the
reset topology and active levels:

- `RSTOUT` is the SLAVE processor's reset output; high starts the reset
  sequence;
- `RESETN` is the active-low reset for the other host ICs;
- `NRESET` is the active-low video-system/VSR reset; and
- `HALTN` is active-low, open-drain and bidirectional; asserted together with
  reset it places the 68070 in the reset state.

The same manual's block diagram keeps the SLAVE CPU outside the host reset
domain. This corroborates a full host-device reset that preserves SLAVE RAM
and protocol state, rather than a CPU-register-only reset or reconstruction of
`SlaveHle`.

## Post-reset boot-mode conversation (BIOS `cdapdriv`)

The launch mode is not returned by the `F4` test-plug query. Dynamic tracing
and disassembly of `cdapdriv` in the CD-i 220 F2 BIOS (`0x42D3D2` response
handler) show this sequence:

1. On an ordinary native CD-i boot, the BIOS sends four bytes
   `B0 00 00 00` on ch3. The HLE reply is `B0 00 02 15`.
2. ch2 `0x8A` resets the host while the SLAVE retains its launch state.
3. The restarted BIOS repeats the B0 query. A retained-mode reply of
   `B0 00 42 15` has bit 6 set in the third byte.
4. If the CDAP driver is in its initial `$00000200` state, that bit makes it
   issue `B1 00 00 00` on ch3.
5. The SLAVE replies `B1 00 00 00`; the low 24 bits are the disc base
   (`00:00:00` here). The BIOS then reads the disc application from LBA 17
   onward and transfers control to it.

The low three bits of B0 byte 2 are the disc-type code later returned by
`cdapdriv` GetStat `$55`. Native CD-i is type `2`; CD-ROM XA Bridge/White
Book VCD is type `4`. A retained bridge disc is observed as `B0 00 44 15`.
The physical origin of bit 6 is narrower than the earlier HLE name
`disc_boot_mode` implied: SERVO sets it from a latched valid lower-drive
status, as detailed in the SERVO packet map below. Its persistence across the
host-only reset makes it the BIOS-visible retained-mode signal, but it is not
itself a PLAY-command flag. The VMPEG `vcd` module tests
GetStat `$55` for bit `0x400` and writes `1` to `$E01000` when type 4 is
present. Hardware review identifies this register as the control for a
separate 13.5 MHz output-clock converter on the VMPEG cartridge, downstream
of MCD251. It is not an MCD251 phase register.

`DiscImage` derives type 4 from the disc itself rather than its filename: the
Mode-2 Form-1 primary volume descriptor at LBA 16 must have the ISO header
`01 CD001 01`, system identifier `CD-RTOS CD-BRIDGE`, and CD-XA application
signature `CD-XA001`. Accused and Addams Family Values VCD traces confirmed
that type 4 makes both select the White Book 13.5 MHz output-converter path.
Automatic type-4 exposure on insertion is enabled and covered by a synthetic
XA-Bridge test. The accepted converter model expands horizontal output by
`15/13.5` (`10/9`) at the cartridge boundary. Remaining title-specific
position differences are evidence about guest-programmed MCD251 coordinates,
not evidence for another hidden phase register or a host crop.

Read-only type-4 traces further show that Accused Netherlands and Addams
Family Values UK program the same MCD251 origin/active tuple
`Xo=65, Yo=26, Xa=384, Ya=280`; their `Xd`, `Xw`, and `Ww` display/window
commands differ. This rules out treating `Xo` as a per-title decoded-image
crop. A synchronized real-hardware register/output trace is still required
before changing the accepted coordinate mapping.

This exchange was verified with CD Shoot: after the reset, the BIOS consumes
the retained `B0`, sends `B1`, loads the disc modules, starts Mode-2 streaming,
and produces title ADPCM audio. During that trace a separate SCC68070 issue
also surfaced: DMA channel status is write-one-to-clear and software-start
sets completion (`COC`). Modeling it as plain register RAM lets the label
sector DMA succeed once but leaves every later CDIC DMA waiting forever.

`F4` remains a two-byte ch2 response `[F4, status]` formed by the firmware
from `$55.1..0`; ch0 commands `0x87/0x88` clear/set bit 1. It is a test-plug
status, not the retained disc-launch selector.

The local ICDIA archive (`Philips CD-i - icdia-site-documents-2026-07-18`)
corroborates the higher-level Green Book behavior: the ROM player shell
deallocates its plane and loads the initial disc application. It does not,
in the material assessed so far, document this private SLAVE/CDAP byte
protocol; the values above come from firmware and BIOS disassembly plus the
runtime trace.

The current archive contains 185 PDFs. The historical source map is
`docs/icdia-archive-assessment.md`; current requirements and evidence status
are tracked in `docs/specification-research.md` and
`data/compatibility/compliance-matrix.json`. In particular,
`docs/pointing_devices.pdf` specifies a relative mouse as signed deltas with
packets emitted only for motion or button transitions. This corroborates the
relative frontend/HLE integration used here, while the SLAVE's host-side
four-byte accumulated-coordinate response remains firmware-derived behavior.

## Live media-change delivery

The firmware's SERVO receive path copies a complete four-byte packet to
`$99..$9C` and sets work flag `$54.7` when ch3 command `0xFA` has enabled
`$63.7`. The host IRQ read path at `0x02D8` then exposes those four bytes on
channel 3. Consequently, a live drive transition is an unsolicited `B0`
packet on the same channel and in the same form as the explicit `B0` query;
it is not the unrelated channel-1 byte `0x3B`.

`SlaveHle` now keeps media presence separately from the disc-type code.
Power-on attachment only establishes the state used by an explicit query.
Changing media in a running machine queues an asynchronous drive-status
packet until the BIOS enables X-Bus notifications and the preceding ch3
response has been consumed. Replacing one mounted image queues an empty-drive
packet followed by the new-medium packet, modeling the physical remove/insert
transition without resetting the 68070 from the frontend. CDIC drive-backed
transport is stopped at the same boundary while its host-visible register
interface remains intact.

The settled HLE packets retain the BIOS-verified Mono-I encoding:
`B0 00 00 15` for no medium and `B0 00 02 15` for native CD-i media. A
retained PLAY launch still adds bit 6 (`B0 00 42 15`). The lower-level
open/close/spin-up phases observed on real SERVO links remain future
transport-state work.

### SLAVE shared-flag cross-reference

These bytes are bitfields shared by the host-command IRQ, SCI interrupt and
main loop. A set bit is not necessarily a persistent device state. In
particular, `$5D` is mostly a one-shot response-request byte. The tables below
name only behavior demonstrated by reachable firmware paths.

#### `$55` — host-selected modes and SCI work state

| Bit | Producers | Consumers and observable result |
|---:|---|---|
| 0 | Power-on handshake at `$019A` | Included as bit 0 of the ch3 `F4` reply at `$0F4E`; no SCI traffic. |
| 1 | ch0 `87/88` at `$065F/$0662` | Included as bit 1 of `F4`; gates the host-reset paths at `$0EB3` and `$10AC`. It arms reset but emits no reply by itself. |
| 2 | ch0 `85` at `$0655` | Makes the main loop sample Port C status at `$0287`; changes can enqueue `2A`, `2C`, or `3B` through `$0E7B`. Also marks matching SERVO `B0` status at `$0C95`. |
| 3 | ch0 `86` at `$065A` | Selects the alternate incoming-position path at `$0830`; together with `$5A.4`, runs the timing/state routine `$0FDE`. |
| 4 | ch0 `80..82` at `$0640..$064C` | Selects the first signed lookup transform at `$0FB0`; no SCI or host response. |
| 5 | ch0 `80..82` at `$0640..$064C` | Selects the second signed lookup transform at `$0FC7`; no SCI or host response. |
| 6 | SCI ISR error/completion paths at `$074C/$0758` | Cleared by the main loop at `$0257`, which acknowledges/re-arms the SCI registers. It is an internal SCI work latch, not a host response. |
| 7 | ch2 `8F` at `$0619`, plus incoming-position paths `$083E..$0884` | Alternates/schedules the six-bit position update through `$54.2`; it may lead to later SERVO work but has no direct host packet. |

#### `$5A` — serial routing, reset gates and drive-packet state

| Bit | Producers | Consumers and observable result |
|---:|---|---|
| 0 | ch0 `89` at `$0665` | Enables ch3 `F6` to sample Port D bit 7 at `$0460`; mutually exclusive with bit 1. |
| 1 | ch0 `8A` at `$066C` | Selects Port C bit 7 output behavior in ch2 `83/84` at `$0569/$057D`; mutually exclusive with bit 0. |
| 2 | ch3 `F6` at `$0460` | Encoded as reply value 1 or 2 at `$0F9E`; this is the complete direct host-visible lifetime of the bit. |
| 3 | Matching incoming SERVO `B0` at `$0C95` | Alters Port C change filtering at `$028E/$0E85`, can enqueue `3B`, and participates in reset/transition paths. No direct response byte. |
| 4 | Buffered ch3 packet `81` at `$0419` | With `$55.3`, makes the main loop call `$0FDE`; also calls `$112D` from that routine. No direct reply. |
| 5 | Incoming SERVO `B0` state machine at `$0CA8/$0CCB/$0CE4` | Tracks the firmware's intermediate drive-state range and gates reset decisions. It is cleared when the low status nibble reaches 5. |
| 6 | ch0 `8B/8C` at `$0673/$0676` | Host-reset gate consumed at `$0EB6`, `$0CBC` and `$10AF`; clear permits the armed reset paths. |
| 7 | Set before outgoing SERVO `A6/A8` at `$0D4C/$0D70`; cleared by incoming `B0` at `$0CBF` | Internal command/drive-state latch used to suppress repeated transition handling. |

#### `$5D` — one-shot host-response requests

| Bit | Reachable producer | Consumer/result |
|---:|---|---|
| 0 | ch3 `F0` at `$044E` | `$0F19` builds `F0 20 $52 FF`. |
| 1 | None. `$0451` sets it, but no dispatch entry reaches `$0451`. | No consumer in `$0F19`; retained as unreachable firmware code. |
| 2 | None found | No consumer found. |
| 3 | ch3 `F3` at `$0457` | Builds `F3 $B6 FF`. |
| 4 | ch3 `F4` at `$045A` | Builds `F4 (($55.1 << 1) | $55.0) FF`. |
| 5 | ch3 `F5` at `$045D`, ch2 `87` and payload/position paths | Builds `F5 ($D6 & $3F) FF FF`. |
| 6 | ch3 `F6` at `$046C` | Builds `F6 (($5A.2 >> 2) + 1) FF FF`. |
| 7 | No normal producer found | `$0F60` would copy `$66..$69` into the host response staging area. Normal raw SERVO forwarding instead uses `$63.7`; this path is unreachable in the decoded revision. |

#### `$63` — host delivery gates

| Bit | Producers | Consumer/result |
|---:|---|---|
| 4 | ch0 `83/84` at `$064F/$0652` | Enables the alternate four-byte coordinate/position decoder at `$08AD`, which schedules the existing input update path. |
| 5 | ch2 `8D/8E` at `$060D/$0616` | Enables response-ring notification. `$0EE8` sets `$54.5` when a byte is queued. |
| 6 | ch3 `F8/F9` at `$0475/$0478` | Gates host-ready assertion for staged ch3 replies: the main loop combines `$63.6` with `$54.6` at `$0248`. |
| 7 | ch3 `FA/FB` at `$046F/$0472` | Enables transparent four-byte SERVO delivery. `$0C36` copies `$66..$69` to `$99..$9C` and sets `$54.7` for host channel 3. |

Bits 0-3 of `$63` have no decoded reads or writes. This completes the
reachable read/set/clear audit for `$55`, `$5A`, `$5D`, and `$63`; recovery
routine `$10D1` bulk-restores the bytes from its reset table and does not add
a separate operational producer.

### Service-manual drive/SERVO contract

The CD-i 205 service manual independently documents the lower-level topology
at PDF pp. 79-81. The host MC68070 talks to the SLAVE MC68HC05 over its
address/data bus; the SLAVE talks to a second MC68HC05 drive processor over
SPI (`SCK`, `MOSI`, `MISO`, `SPISS`). For drive traffic the SLAVE enters a
“transparent mode”: it validates neither direction and forwards the entire
four-byte message.

Each message contains one command byte followed by three data bytes. The
command byte has its high bit set; each data byte has its high bit clear. The
documented request/reply families are:

| Command | Meaning |
|---|---|
| `A0` | CD status |
| `A1` | absolute time as BCD minute/second/frame |
| `A2` | BCD track/index |
| `A3` | drive software version |
| `A4` | echo the received command |
| `A5` | drive error, including focus and radial errors |
| `AB` | service mode command |

This does not replace the firmware-derived channel-3 `B0` CDAP conversation:
it constrains the missing physical-drive side behind it. A future complete
transport state machine should preserve the four-byte forwarding boundary,
model the second drive MCU's status transitions, and let the SLAVE transform
or expose them exactly as its firmware does. It should not synthesize
open/close/spin-up timing directly in the frontend.

### SERVO 2.1 firmware packet map

`docs/servo-zx405037p.disasm.asm` is the complete linear disassembly of the
matching ZX405037P SERVO 2.1 image. Its source firmware SHA-256 is
`b467fb3e9bb9e38d195c55116aecd6bc4c8023cdfe4b4d50fd9699139f221b9b`; the
generated assembly SHA-256 is
`ef389e98800957584a3811403a454f55d93984f1dd3a355a2e61f3a4f7fdc03c`.
The image begins with a test-ROM prefix, so the linear decode overlaps the
real reset entry at `$0100` by one zero-byte instruction. Interpret routines
from their vector and call targets, not by treating the prefix as one
continuous control-flow graph.

The vectors establish these entry points:

| Vector | Target | Role |
|---:|---:|---|
| `$1FFE` | `$0100` | Reset |
| `$1FFC` | `$0100` | SWI/fallback |
| `$1FFA` | `$18CA` | External/front-panel IRQ |
| `$1FF8` | `$0F8B` | Timer |
| `$1FF6` | `$0100` | Unused/fallback |
| `$1FF4` | `$159B` | SPI packet transport |

The SPI ISR stores four receive bytes in `$CA..$CD`, uses `$D3` as its byte
index, and transmits from `$CE..$D1`. A packet is accepted only when its first
byte has bit 7 set; exactly three following bytes complete it. No checksum is
performed. Completion dispatches through `$1698`. `$76.1` means a response is
ready, `$76.5` means transmission is active, `$1686` begins transmission, and
`$1634` serializes the reply.

The decoded command behavior is:

| Request | Firmware behavior | Reply/source |
|---|---|---|
| `A0` | No direct comparison is present. The operational scheduler builds status at `$178B`. | Periodic/request-independent `B0`; bytes come from current drive/status fields. |
| `A1` | Explicit dispatch at `$16B9`. | Time response from the current address/time state. |
| `A2` | Sets `$78.6`. | No scheduler consumer or reply builder was found in this revision. Do not synthesize one. |
| `A3` | Sets `$78.7`. | `$1833` emits `A3 20 00 00` (software version 2.0). |
| `A4` | Sets `$79.0`. | `$1863` echoes the three received data bytes `$CB..$CD`. |
| `A5` | Sets `$79.1`; internal error paths can set it too. | No scheduler consumer or reply builder was found in this revision. Do not synthesize one. |
| `AB 55 xx xx` | Enters service mode. | Service-mode state change. |
| `AB 77 xx xx` | Exits service mode. | This firmware uses `77`, while the older service-manual text says `AA`. |

The scheduler at `$175B` emits operational `B0`, `B1`, `A3`, `A4`, and
service response `AA`; it does not consume the `A2` or `A5` request latches.
`$181E` builds the `B1` echo. Service subcommands `$DD=1..3` produce
`AA 01 00 00`, `AA 02 00 00`, and `AA 03 00 00` at `$1878`. This is a
revision-specific firmware result: the service manual still supplies the
physical meaning of the command families, but is not evidence that every
documented reply is implemented by this exact ROM.

#### B0 byte-2 bit 6 provenance

The end-to-end firmware path is now identified:

- SERVO `$178B..$181D` builds `B0`. `$D0` is byte 2; `$17BD..$17C2`
  copies RAM latch `$77.4` into `$D0.6`.
- The ordinary receive path reaches `$08CC` only when the lower status nibble
  in `$9A` is `5`. If the received command in `$9C` is `B0`, it copies the
  three payload bytes `$A1..$A3` into `$BC..$BE`, marks the response valid at
  `$7A.1`, and sets `$77.4` at `$08E0`.
- The downstream `D1` status paths at `$1CBE..$1CDB` likewise store their
  three status bytes in `$B4..$B6` before setting `$77.4`. This makes the bit
  a valid/latching lower-drive-controller-status indication, not a direct
  tray, spin, or PLAY state.
- Sending B0 does not clear `$77.4`. It is cleared only with broader drive
  state resets that clear all of `$77`, including `$1568`, `$1ACD`, and
  `$1B0B`. A host-only 68070 reset does not reset the SERVO or SLAVE MCU, so
  this state can survive into the restarted BIOS query.
- SLAVE SCI receive `$0BF4..$0C2C` stores the four bytes in `$66..$69`.
  With channel-3 forwarding enabled, `$0C36..$0C49` copies them unchanged to
  `$99..$9C` and schedules the host response. The B0 interpreter at
  `$0C55..$0CE8` observes status but does not rewrite the packet.

The current HLE retains `disc_boot_mode` as a behavioral abstraction for the
proven `B0 00 42/44 15` conversation. Read-only diagnostics separately expose
the currently modeled SERVO B0 packet, the oldest queued channel-3 packet,
and the B0/B1 packet already present in the channel-3 window for the BIOS to
consume. This lets a capture locate the forwarding boundary without claiming
that the low-level SERVO state machine executed. Replacing the abstraction
still requires the physical evidence identified in checkpoint 5.

#### Checkpoint 5 transition matrix

The deterministic HLE matrix below is now protected by a device test. It is a
description of current observable behavior, not evidence that a physical
player uses the same intermediate states or durations.

| Scenario | Packet order visible to the host | Reset boundary | Current HLE duration | Evidence status |
| --- | --- | --- | --- | --- |
| Power-on, empty | No unsolicited packet; an explicit `B0` query returns `B0 00 00 15` | Whole machine starts reset | Query response after `15 MHz / 4` cycles (250 ms) | Packet is deterministic; duration is inherited HLE behavior, not a measured drive time. |
| Power-on, disc already mounted | No unsolicited packet; explicit `B0` returns `B0 00 02 15` for the native/XA classification currently used by `SlaveHle` | Whole machine starts reset | 250 ms query response | Packet is deterministic; physical insertion/spin-up history is absent. |
| Live insertion | After host command `FA` enables forwarding, one unsolicited ready `B0 00 02 15` becomes BIOS-visible | None | 100 us forwarding delay | Ordering is tested HLE behavior. Physical caddy, focus, TOC, and ready substates are unknown. |
| PLAY and retained boot | Host command `8A` requests a 68070 reset while SLAVE state survives; restarted BIOS reads `B0 00 42 15`, then can request `B1 00 00 00` | Host/68070 only; SLAVE state preserved | B0 query 250 ms; B1 100 us | Reset topology, retained bit-6 behavior, and title boot are supported by firmware and observed software behavior. Exact timing is unmeasured. |
| Live removal | One unsolicited empty `B0 00 00 15` becomes BIOS-visible | None | 100 us forwarding delay | Deterministic HLE behavior; physical tray timing unmeasured. |
| Atomic replacement | Empty `B0 00 00 15` is delivered first; after it drains, ready `B0 00 02 15` follows | None | 100 us per forwarded packet | Deliberate HLE ordering with a regression test; it still needs hardware confirmation. |
| Failed/not-ready media | Not modeled | Unknown | Unknown | SERVO RAM `$77.5` can select B0 byte-2 type code 5, but the available firmware and service text do not prove its physical meaning, packet sequence, or duration. |

The CD-i 205 service manual confirms the physical routing needed for a
capture: caddy-switch and drive-status information enters the drive
processor, which returns four-byte status/error messages to SLAVE over SPI
and then to the 68070. It does not define the transition timing or fully map
the operational B0 bit field. Therefore successful title boot, HLE delay
constants, or the existence of the caddy switch cannot fill the final row.

To complete the physical matrix, record the drive-processor-to-SLAVE SPI
traffic, SLAVE channel-3 traffic, 68070 reset, caddy switch, and drive busy
signal where accessible. For each of the seven rows above, begin before the
stimulus and continue until status remains stable or a bounded five-second
timeout expires. Preserve raw timestamps and identify:

1. every four-byte packet in order;
2. the first packet exposed to the BIOS;
3. assertion and release of the host reset line;
4. duration of every distinct intermediate status; and
5. the exact physical condition for any B0 type-5 packet.

Only independently validated firmware simulation or a real-player capture is
sufficient to promote those timings and ambiguous bits into emulation.

### ch2 payload forms (`0x048D..0x0511`)

- Bytes below `0x80` are collected as a four-byte payload in `$76..$79`,
  with `$75` holding the command; after four payload bytes the main-loop
  work flag `$54.2` is set.  This includes the `0xC0..0xCF` audio
  attenuation packets used by the host driver.
- For a first byte `0xC0..0xFF`, `0x04AD` saves the low nibble in `$CC` when
  bits 4-5 are clear; the other encodings update the six-bit transport
  position/direction field `$D6` and request a status response through
  `$5D.5`.
- When command `0x90` has armed `$59.0`, the handler instead receives two
  bytes; its second byte is stored in `$D8` unless it is `0xFF`.

### AD7528 output trace: Hotel Mario score screen

An external Mono-I Saleae SPI decode captures 55 writes to the two AD7528
devices while Hotel Mario fades its score-screen audio. The local text export
has SHA-256
`0cd157158a3dd637fa3661e1840a04f799491b1da58cc82c3477335709ea2830` and is
diagnostic-only; it is not stored in this repository.

- Each 32-bit MOSI word repeats one value four times, for example
  `A1 A1 A1 A1`. In this scene all four matrix inputs therefore move together.
- The rising ramp has 29 writes from 0 to 255 over 27.132 ms. The falling
  ramp has 26 writes from 252 to 0 over 24.031 ms.
- Ordinary write spacing clusters around 0.775-0.825 ms. A few 2.2-2.9 ms
  gaps account for the workload-dependent fade duration and support the
  reported approximately 791 microsecond SLAVE scheduling step.

This trace is a timing oracle, not yet an audible-gain implementation. It does
not by itself establish command-byte polarity, the AD7528 analog transfer
curve, mute behavior, or whether CDIC and VMPEG audio share the same final
analog boundary. Those must be tied to simultaneous ch2 command and audio
measurements before replacing the current digital attenuation model.

### ch3 commands (`0x03DA`, matches MAME's known set)

- First byte < 0xF0: buffered at `$7A` (count `$6D`).
  - Packet `0x80 a b c`: stores → `$D2/$BB/$D3`.
  - Packet `0x81 a b`: stores → `$D4/$D5`, sets `$5A.4`.
- `0xF0-0xFF` immediate, BRA table `0x042E`:
  - 0xF0/0xF3/0xF4/0xF5 → set `$5D` bits 0/3/4/5 = query-request flags
    (revision / pointer type / test plug / +?) — main loop enqueues the
    responses (matches MAME's F0/F3/F4 replies)
  - 0xF6 → if `$5A.0`: latch port D bit7 into `$5A.2`; set `$5D.6`
    (NTSC/PAL query)
  - 0xF7 → clr `$58.3`; 0xFE → set `$58.3`. This is a firmware input-mode
    flag, not an off switch for the HLE pointer poller: Alien Gate sends FE
    while taking over from the player shell, then continues consuming pointer
    packets. The HLE therefore starts pointer reporting at F7 and keeps it
    active through FE until reset (matching MAME's operational behavior).
  - 0xF8 / 0xF9 → set / clear `$63` bit 6
  - 0xFA / 0xFB → set / clear `$63` bit 7
  - 0xFC → `$BA=2`, drive PC6 high; 0xFD → release PC6
  - 0xF1/0xF2/0xFF → no-op

## Next steps

The remaining work is deliberately split into research checkpoints. The
working retained-mode HLE must not change during checkpoints 1-4: those steps
produce names, call graphs, and trace points, not guessed transport behavior.

1. **Completed: name the remaining SLAVE flag consumers.**
   Search `slave-zx405042p.disasm.asm` for every read, clear, and set of
   `$55`, `$5A`, `$5D`, and `$63`, then follow each reachable main-loop path.
   Record, for every bit, its producer command, consumer routine, whether it
   sends SCI traffic, and whether it enqueues a host response. Treat unknown
   bits as unknown rather than naming them from one call site. Completion
   means the document contains a flag table with firmware addresses and no
   unexamined main-loop consumer for those four bytes.

2. **Completed: generate and annotate the matching SERVO disassembly.** Use
   the same firmware generation as the decoded SLAVE:

   ```sh
   python3 scripts/hc05dis.py \
     firmware/mcu/zx405037p__cdi_servo_2.1__b43t__llek9215.mc68hc705c8a_withtestrom.7201 \
     0x0000 0x2000 > docs/servo-zx405037p.disasm.asm
   ```

   Verify reset/IRQ/timer/serial vectors first, then identify the four-byte
   receive/transmit buffers, packet-valid flag, checksum or framing checks,
   and command dispatch. Keep generated assembly separate from interpretive
   notes so it can be reproduced byte-for-byte.

3. **Completed: correlate the `A0..A5`/`AB` command families.** For
   each request and reply, identify the SERVO dispatch target and the source of
   all three data bytes. Cross-reference the service-manual meanings above:
   CD status, absolute time, track/index, version, echo, drive error, and
   service mode. Note BCD conversion, error/status latches, and any periodic
   versus request-driven packets. Completion means a packet table links each
   byte to both a firmware address and a documented physical meaning.

4. **Completed: trace B0 byte-2 bit 6 end-to-end.** SERVO B0 builder
   `$178B..$181D` copies `$77.4` into byte 2 bit 6. The latch is set only after
   valid lower-drive status reception and cleared with broader drive-state
   resets, while the SLAVE SCI/channel-3 path forwards the four bytes
   unchanged. Read-only snapshots and bounded events now distinguish the
   HLE-modeled B0 from queued forwarded packets. This proves the transport
   path and persistence mechanism; exact tray/medium/spin-up timing belongs
   to checkpoint 5.

5. **In progress: capture a bounded transition matrix (high evidence
   requirement).** The current HLE matrix now has deterministic device tests,
   and diagnostics distinguish modeled, queued, and BIOS-visible B0/B1
   packets. Power-on empty/mounted, insertion, PLAY/reset/retained boot,
   removal, and replacement are covered at that boundary. The checkpoint is
   not complete: real hardware or independently validated firmware must still
   establish mechanical timing, intermediate states, replacement ordering,
   and the physical meaning of the SERVO type-5 failed/not-ready path. A
   successful title boot alone remains insufficient.

6. **Replace the shortcut only after checkpoints 1-5 (high implementation and
   regression risk).** Implement the smallest device-level SLAVE/SERVO
   transport state machine supported by the collected evidence. Preserve the
   four-byte forwarding boundary and the SLAVE-survives-host-reset topology.
   Remove `disc_boot_mode` only when the new state machine reproduces its
   proven `B0 00 42/44 15` behavior naturally. Add device tests for every
   captured transition, then re-test native CD-i, VCD, Photo CD, first
   insertion, replacement, and the PLAY-triggered host reset.

Checkpoint rule: a failed implementation does not invalidate a firmware
hypothesis unless the experiment exercised the same decoded prerequisites.
Record the base revision, flag/packet evidence, and changed dependency before
repeating an earlier transport approach.
