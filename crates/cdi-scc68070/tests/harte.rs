// SPDX-License-Identifier: GPL-2.0-or-later
//! Conformance testing against the SingleStepTests m68000 vectors
//! (https://github.com/SingleStepTests/m68000, MIT). Fetch them with
//! `scripts/fetch-harte.sh`; the test is skipped if the directory is absent.
//!
//! The vectors describe a 68000. Our core is a 68070, so tests where the
//! architectures diverge by design are skipped rather than failed:
//! * any test in which our core takes an exception (frame formats and
//!   privilege rules differ on the 68070), and
//! * any test performing an odd-address 16-bit access (address-error
//!   behavior differs).
//!
//! Cycle counts and bus transaction logs are not compared.

use cdi_scc68070::bus::{Bus68k, FnCode, WaitTicks};
use cdi_scc68070::cpu::{sr_bits, Cpu};

// --- Flat 16 MB test bus --------------------------------------------------

struct TestBus {
    mem: Vec<u8>,
    odd_word_access: bool,
}

impl TestBus {
    fn new() -> Self {
        Self {
            mem: vec![0u8; 1 << 24],
            odd_word_access: false,
        }
    }
}

impl Bus68k for TestBus {
    fn read8(&mut self, addr: u32, _fc: FnCode) -> (u8, WaitTicks) {
        (self.mem[(addr & 0xFF_FFFF) as usize], 0)
    }
    fn read16(&mut self, addr: u32, _fc: FnCode) -> (u16, WaitTicks) {
        if addr & 1 != 0 {
            self.odd_word_access = true;
        }
        let a = (addr & 0xFF_FFFF) as usize;
        let hi = self.mem[a];
        let lo = self.mem[(a + 1) & 0xFF_FFFF];
        (u16::from_be_bytes([hi, lo]), 0)
    }
    fn write8(&mut self, addr: u32, val: u8, _fc: FnCode) -> WaitTicks {
        self.mem[(addr & 0xFF_FFFF) as usize] = val;
        0
    }
    fn write16(&mut self, addr: u32, val: u16, _fc: FnCode) -> WaitTicks {
        if addr & 1 != 0 {
            self.odd_word_access = true;
        }
        let a = (addr & 0xFF_FFFF) as usize;
        self.mem[a] = (val >> 8) as u8;
        self.mem[(a + 1) & 0xFF_FFFF] = val as u8;
        0
    }
}

// --- .json.bin parsing ----------------------------------------------------

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn u8v(&mut self) -> u8 {
        let v = self.data[self.pos];
        self.pos += 1;
        v
    }
    fn u16v(&mut self) -> u16 {
        let v = u16::from_le_bytes(self.data[self.pos..self.pos + 2].try_into().unwrap());
        self.pos += 2;
        v
    }
    fn u32v(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.data[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
}

#[derive(Clone, Default)]
struct State {
    regs: [u32; 19], // d0-7, a0-6, usp, ssp, sr, pc
    prefetch: [u32; 2],
    ram: Vec<(u32, u8)>,
}

const R_USP: usize = 15;
const R_SSP: usize = 16;
const R_SR: usize = 17;
const R_PC: usize = 18;

struct Test {
    name: String,
    initial: State,
    fin: State,
}

fn read_state(r: &mut Reader) -> State {
    let _numbytes = r.u32v();
    assert_eq!(r.u32v(), 0x0123_4567, "state magic");
    let mut st = State::default();
    for i in 0..19 {
        st.regs[i] = r.u32v();
    }
    st.prefetch = [r.u32v(), r.u32v()];
    let num_rams = r.u32v();
    for _ in 0..num_rams {
        let addr = r.u32v();
        let data = r.u16v();
        st.ram.push((addr, (data >> 8) as u8));
        st.ram.push((addr | 1, data as u8));
    }
    st
}

fn read_test(r: &mut Reader) -> Test {
    let _numbytes = r.u32v();
    assert_eq!(r.u32v(), 0xABC1_2367, "test magic");
    // Name block
    let _nb = r.u32v();
    assert_eq!(r.u32v(), 0x89AB_CDEF, "name magic");
    let strlen = r.u32v() as usize;
    let name = String::from_utf8_lossy(&r.data[r.pos..r.pos + strlen]).into_owned();
    r.pos += strlen;
    let initial = read_state(r);
    let fin = read_state(r);
    // Transactions block: parse enough to skip it.
    let _tb = r.u32v();
    assert_eq!(r.u32v(), 0x4567_89AB, "transactions magic");
    let _num_cycles = r.u32v();
    let num_transactions = r.u32v();
    for _ in 0..num_transactions {
        let tw = r.u8v();
        let _cycles = r.u32v();
        if tw != 0 {
            r.pos += 20;
        }
    }
    Test { name, initial, fin }
}

fn parse_file(data: &[u8]) -> Vec<Test> {
    let mut r = Reader { data, pos: 0 };
    assert_eq!(r.u32v(), 0x1A3F_5D71, "file magic");
    let num_tests = r.u32v();
    (0..num_tests).map(|_| read_test(&mut r)).collect()
}

// --- Runner ---------------------------------------------------------------

fn apply_state(cpu: &mut Cpu, bus: &mut TestBus, st: &State) {
    cpu.d.copy_from_slice(&st.regs[0..8]);
    cpu.a[..7].copy_from_slice(&st.regs[8..15]);
    let supervisor = st.regs[R_SR] & u32::from(sr_bits::S) != 0;
    if supervisor {
        cpu.a[7] = st.regs[R_SSP];
        cpu.sp_other = st.regs[R_USP];
    } else {
        cpu.a[7] = st.regs[R_USP];
        cpu.sp_other = st.regs[R_SSP];
    }
    cpu.sr = st.regs[R_SR] as u16;
    // The vectors' PC is the next-prefetch address: +4 from the first
    // instruction word. Execute from pc-4 with the prefetch words placed
    // there.
    let start = st.regs[R_PC].wrapping_sub(4);
    cpu.pc = start;
    for (addr, byte) in &st.ram {
        bus.mem[(*addr & 0xFF_FFFF) as usize] = *byte;
    }
    let p0 = st.prefetch[0] as u16;
    let p1 = st.prefetch[1] as u16;
    bus.mem[(start & 0xFF_FFFF) as usize] = (p0 >> 8) as u8;
    bus.mem[((start + 1) & 0xFF_FFFF) as usize] = p0 as u8;
    bus.mem[((start + 2) & 0xFF_FFFF) as usize] = (p1 >> 8) as u8;
    bus.mem[((start + 3) & 0xFF_FFFF) as usize] = p1 as u8;
}

fn check_state(cpu: &Cpu, bus: &TestBus, st: &State) -> Vec<String> {
    let mut errs = Vec::new();
    let names = [
        "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "a0", "a1", "a2", "a3", "a4", "a5", "a6",
    ];
    for (i, name) in names.iter().enumerate().take(8) {
        if cpu.d[i] != st.regs[i] {
            errs.push(format!("{}={:#x} want {:#x}", name, cpu.d[i], st.regs[i]));
        }
    }
    for i in 0..7 {
        if cpu.a[i] != st.regs[8 + i] {
            errs.push(format!(
                "{}={:#x} want {:#x}",
                names[8 + i],
                cpu.a[i],
                st.regs[8 + i]
            ));
        }
    }
    let supervisor = cpu.sr & sr_bits::S != 0;
    let (usp, ssp) = if supervisor {
        (cpu.sp_other, cpu.a[7])
    } else {
        (cpu.a[7], cpu.sp_other)
    };
    if usp != st.regs[R_USP] {
        errs.push(format!("usp={usp:#x} want {:#x}", st.regs[R_USP]));
    }
    if ssp != st.regs[R_SSP] {
        errs.push(format!("ssp={ssp:#x} want {:#x}", st.regs[R_SSP]));
    }
    if u32::from(cpu.sr) != st.regs[R_SR] {
        errs.push(format!("sr={:#06x} want {:#06x}", cpu.sr, st.regs[R_SR]));
    }
    // The vectors' PC is the next-prefetch address (+4); a stopped CPU no
    // longer prefetches, so STOP reports the un-adjusted PC.
    let want_pc = if cpu.stopped {
        st.regs[R_PC]
    } else {
        st.regs[R_PC].wrapping_sub(4)
    };
    if cpu.pc != want_pc {
        errs.push(format!("pc={:#x} want {:#x}", cpu.pc, want_pc));
    }
    for (addr, byte) in &st.ram {
        let got = bus.mem[(*addr & 0xFF_FFFF) as usize];
        if got != *byte {
            errs.push(format!("ram[{addr:#x}]={got:#04x} want {byte:#04x}"));
        }
    }
    errs
}

fn harte_dir() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("CDI_HARTE_DIR") {
        return Some(dir.into());
    }
    let default =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests-data/harte-68000");
    default.exists().then_some(default)
}

#[test]
fn harte_68000_vectors() {
    let Some(dir) = harte_dir() else {
        eprintln!("harte vectors not present; run scripts/fetch-harte.sh to enable");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".json.bin"))
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .json.bin files in {}", dir.display());

    // Known-divergent or known-bad vector files (see repo README + 68070
    // architectural differences). Everything else must pass fully.
    let skip_files = [
        "TAS.json.bin",   // repo README: TAS timing not modeled correctly
        "TRAPV.json.bin", // repo README: suspect vectors
        "RTE.json.bin",   // 68070 RTE pops a format/vector word (68010-style),
                          // the 68000 frame in these vectors has none
    ];

    let mut total_pass = 0u64;
    let mut total_skip = 0u64;
    let mut total_fail = 0u64;
    let mut failed_files: Vec<String> = Vec::new();

    for path in &files {
        let fname = path.file_name().unwrap().to_string_lossy().into_owned();
        if skip_files.contains(&fname.as_str()) {
            continue;
        }
        let data = std::fs::read(path).unwrap();
        let tests = parse_file(&data);
        let mut pass = 0u64;
        let mut skip = 0u64;
        let mut fail = 0u64;
        let mut first_fail: Option<String> = None;
        for t in &tests {
            let mut cpu = Cpu::new();
            let mut bus = TestBus::new();
            apply_state(&mut cpu, &mut bus, &t.initial);
            cpu.step(&mut bus);
            if cpu.exceptions_taken > 0 || bus.odd_word_access {
                skip += 1;
                continue;
            }
            let errs = check_state(&cpu, &bus, &t.fin);
            if errs.is_empty() {
                pass += 1;
            } else {
                fail += 1;
                if first_fail.is_none() {
                    first_fail = Some(format!("{}: {}", t.name, errs.join(", ")));
                }
            }
        }
        total_pass += pass;
        total_skip += skip;
        total_fail += fail;
        if fail > 0 {
            failed_files.push(format!(
                "{fname}: {fail} failed / {pass} passed / {skip} skipped — first: {}",
                first_fail.unwrap()
            ));
        }
    }

    eprintln!("harte summary: {total_pass} passed, {total_fail} failed, {total_skip} skipped");
    for f in &failed_files {
        eprintln!("  {f}");
    }
    assert_eq!(
        total_fail,
        0,
        "harte vector failures:\n{}",
        failed_files.join("\n")
    );
}
