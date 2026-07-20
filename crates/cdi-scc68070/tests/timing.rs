// SPDX-License-Identifier: GPL-2.0-or-later
//! SCC68070 timing-table spot checks (Philips April 1993, section 6.2).

use cdi_scc68070::{Bus68k, Cpu, FnCode};

struct FlatBus {
    mem: Vec<u8>,
}

impl FlatBus {
    fn new(words: &[u16]) -> Self {
        let mut mem = vec![0; 0x1000];
        for (index, word) in words.iter().enumerate() {
            mem[index * 2..index * 2 + 2].copy_from_slice(&word.to_be_bytes());
        }
        Self { mem }
    }
}

impl Bus68k for FlatBus {
    fn read8(&mut self, addr: u32, _fc: FnCode) -> (u8, u32) {
        (self.mem[addr as usize], 0)
    }

    fn read16(&mut self, addr: u32, _fc: FnCode) -> (u16, u32) {
        let addr = addr as usize;
        (u16::from_be_bytes([self.mem[addr], self.mem[addr + 1]]), 0)
    }

    fn write8(&mut self, addr: u32, value: u8, _fc: FnCode) -> u32 {
        self.mem[addr as usize] = value;
        0
    }

    fn write16(&mut self, addr: u32, value: u16, _fc: FnCode) -> u32 {
        let addr = addr as usize;
        self.mem[addr..addr + 2].copy_from_slice(&value.to_be_bytes());
        0
    }
}

fn run(words: &[u16], setup: impl FnOnce(&mut Cpu)) -> u64 {
    let mut bus = FlatBus::new(words);
    let mut cpu = Cpu::new();
    cpu.pc = 0;
    setup(&mut cpu);
    cpu.step(&mut bus)
}

#[test]
fn register_and_immediate_minimums_match_tables_15_and_22() {
    assert_eq!(run(&[0x4E71], |_| {}), 7, "NOP");
    assert_eq!(run(&[0x7001], |_| {}), 7, "MOVEQ #1,D0");
    assert_eq!(run(&[0x0640, 0x0001], |_| {}), 14, "ADDI.W #1,D0");
}

#[test]
fn branch_stack_and_arithmetic_timings_match_tables_14_and_19() {
    assert_eq!(run(&[0x6002], |_| {}), 13, "BRA.B");
    assert_eq!(run(&[0x6102], |cpu| cpu.a[7] = 0x800), 21, "BSR.B");
    assert_eq!(
        run(&[0xC0C1], |cpu| {
            cpu.d[0] = 7;
            cpu.d[1] = 9;
        }),
        76,
        "MULU D1,D0"
    );
    assert_eq!(
        run(&[0x80C1], |cpu| {
            cpu.d[0] = 63;
            cpu.d[1] = 9;
        }),
        130,
        "DIVU D1,D0"
    );
}

#[test]
fn displacement_move_timings_match_tables_12_and_13() {
    assert_eq!(
        run(&[0x3368, 0x0000, 0x0000], |cpu| {
            cpu.a[0] = 0x100;
            cpu.a[1] = 0x200;
        }),
        29,
        "MOVE.W d(A0),d(A1)"
    );
    assert_eq!(
        run(&[0x2368, 0x0000, 0x0000], |cpu| {
            cpu.a[0] = 0x100;
            cpu.a[1] = 0x200;
        }),
        37,
        "MOVE.L d(A0),d(A1)"
    );
}

#[test]
fn illegal_instruction_uses_the_68070_short_exception_timing() {
    assert_eq!(
        run(&[0x4AFC], |cpu| cpu.a[7] = 0x800),
        55,
        "illegal instruction"
    );
}
