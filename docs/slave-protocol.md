# Mono-I SLAVE MCU protocol — reverse-engineering notes

Source: disassembly of the SLAVE firmware dump `zx405042p` ("cdi slave 2.0",
MC68HC705C8A, 8 KB) — see `slave-zx405042p.disasm.asm`, produced with
`scripts/hc05dis.py`. These are original notes documenting facts extracted
from the firmware; see NOTICE.md for the copyright basis.

Status: PARTIAL — the MAME-ch2 immediate-command dispatch, including the
disc-play `0x8A` host reset, is decoded, and the BIOS-side post-reset
`B0`/`B1` conversation is sufficient to launch a real title. The remaining
work is naming the lower-level SERVO/transport flags and replacing HLE drive
status with the complete SERVO-derived state machine.

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
Book VCD is type `4`. Bit 6 is independent retained-disc state, so a retained
bridge disc is reported as `B0 00 44 15`. The VMPEG `vcd` module tests
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

1. Trace main-loop consumers of `$55/$5A/$5D/$63` (search the disasm) to
   map flag → behavior (motor via SCI to SERVO, status byte enqueue).
2. Disassemble the matching SERVO firmware and correlate its four-byte
   `A0..A5`/`AB` traffic with the service-manual definitions.
3. Decode the lower-level SERVO transition that produces B0 bit 6, then
   replace the retained-mode HLE shortcut with the complete transport state
   machine.
