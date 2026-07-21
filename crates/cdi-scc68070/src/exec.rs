// SPDX-License-Identifier: GPL-3.0-or-later
//! Instruction execution: full 68000 user ISA plus the 68070's 68010-style
//! additions (MOVE from CCR, format/vector exception frames).
//!
//! Semantics follow the M68000 Programmer's Reference Manual; SCC68070
//! deviations are marked. Cycle counts follow Philips SCC68070 (April 1993),
//! section 6.2. Bus transfers and effective-address internal clocks are
//! charged by [`Cpu`] and `ea`; instruction-specific internal clocks live
//! beside the corresponding operation below.

use crate::bus::Bus68k;
use crate::cpu::{sr_bits, Cpu, Vector};
use crate::ea::Size;

const SR_C: u16 = sr_bits::C;
const SR_V: u16 = sr_bits::V;
const SR_Z: u16 = sr_bits::Z;
const SR_N: u16 = sr_bits::N;
const SR_X: u16 = sr_bits::X;

/// Complete an instruction whose datasheet timing is easiest expressed as a
/// total. `start` is sampled after the common seven clocks (opcode transfer
/// plus internal minimum), so any future bus wait states remain additive.
fn finish_total_timing(cpu: &mut Cpu, start: u64, total: u64) {
    let elapsed_after_common = cpu.cycles - start;
    let nominal_after_common = total.saturating_sub(7);
    if elapsed_after_common < nominal_after_common {
        cpu.cycles += nominal_after_common - elapsed_after_common;
    }
}

pub fn execute<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    // With the four-clock opcode fetch, this is the seven-clock minimum in
    // Tables 12-22 (MOVEQ, NOP, register ALU operations, and so on).
    cpu.cycles += 3;
    match op >> 12 {
        0x0 => line_0(cpu, bus, op),
        0x1 => move_op(cpu, bus, op, Size::Byte),
        0x2 => move_op(cpu, bus, op, Size::Long),
        0x3 => move_op(cpu, bus, op, Size::Word),
        0x4 => line_4(cpu, bus, op),
        0x5 => line_5(cpu, bus, op),
        0x6 => line_6(cpu, bus, op),
        0x7 => moveq(cpu, op),
        0x8 => line_8(cpu, bus, op),
        0x9 => add_sub(cpu, bus, op, false),
        0xA => cpu.exception(bus, Vector::LineA as u8),
        0xB => line_b(cpu, bus, op),
        0xC => line_c(cpu, bus, op),
        0xD => add_sub(cpu, bus, op, true),
        0xE => line_e(cpu, bus, op),
        0xF => cpu.exception(bus, Vector::LineF as u8),
        _ => unreachable!(),
    }
}

fn illegal<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    log::debug!(
        "illegal opcode {op:#06x} at pc={:#010x}",
        cpu.pc.wrapping_sub(2)
    );
    // PC of the faulting instruction is what gets stacked.
    cpu.pc = cpu.pc.wrapping_sub(2);
    cpu.exception(bus, Vector::IllegalInstruction as u8);
}

fn privileged<B: Bus68k>(cpu: &mut Cpu, bus: &mut B) -> bool {
    if cpu.supervisor() {
        true
    } else {
        cpu.pc = cpu.pc.wrapping_sub(2);
        cpu.exception(bus, Vector::Privilege as u8);
        false
    }
}

// --- Flag computation ----------------------------------------------------

fn set_nz(cpu: &mut Cpu, val: u32, size: Size) {
    cpu.set_flag(SR_N, val & size.msb() != 0);
    cpu.set_flag(SR_Z, val & size.mask() == 0);
}

fn set_logic_flags(cpu: &mut Cpu, val: u32, size: Size) {
    set_nz(cpu, val, size);
    cpu.set_flag(SR_V, false);
    cpu.set_flag(SR_C, false);
}

/// dst + src (+carry_in). Sets XNZVC (Z cleared-only when `extend`).
fn do_add(cpu: &mut Cpu, dst: u32, src: u32, size: Size, extend: bool) -> u32 {
    let carry_in = u32::from(extend && cpu.flag(SR_X));
    let mask = size.mask();
    let (d, s) = (dst & mask, src & mask);
    let result = d.wrapping_add(s).wrapping_add(carry_in) & mask;
    let msb = size.msb();
    let carry = (d & s) | (!result & (d | s));
    let overflow = (d & s & !result) | (!d & !s & result);
    cpu.set_flag(SR_C, carry & msb != 0);
    cpu.set_flag(SR_X, carry & msb != 0);
    cpu.set_flag(SR_V, overflow & msb != 0);
    cpu.set_flag(SR_N, result & msb != 0);
    if extend {
        if result != 0 {
            cpu.set_flag(SR_Z, false);
        }
    } else {
        cpu.set_flag(SR_Z, result == 0);
    }
    result
}

/// dst - src (-borrow_in). Sets XNZVC (Z cleared-only when `extend`).
fn do_sub(cpu: &mut Cpu, dst: u32, src: u32, size: Size, extend: bool, write_x: bool) -> u32 {
    let borrow_in = u32::from(extend && cpu.flag(SR_X));
    let mask = size.mask();
    let (d, s) = (dst & mask, src & mask);
    let result = d.wrapping_sub(s).wrapping_sub(borrow_in) & mask;
    let msb = size.msb();
    let borrow = (s & !d) | (result & (!d | s));
    let overflow = (!d & s & result) | (d & !s & !result);
    cpu.set_flag(SR_C, borrow & msb != 0);
    if write_x {
        cpu.set_flag(SR_X, borrow & msb != 0);
    }
    cpu.set_flag(SR_V, overflow & msb != 0);
    cpu.set_flag(SR_N, result & msb != 0);
    if extend {
        if result != 0 {
            cpu.set_flag(SR_Z, false);
        }
    } else {
        cpu.set_flag(SR_Z, result == 0);
    }
    result
}

fn do_cmp(cpu: &mut Cpu, dst: u32, src: u32, size: Size) {
    do_sub(cpu, dst, src, size, false, false);
}

// --- Condition codes -----------------------------------------------------

pub fn condition(cpu: &Cpu, cc: u16) -> bool {
    let c = cpu.flag(SR_C);
    let v = cpu.flag(SR_V);
    let z = cpu.flag(SR_Z);
    let n = cpu.flag(SR_N);
    match cc {
        0 => true,            // T
        1 => false,           // F
        2 => !c && !z,        // HI
        3 => c || z,          // LS
        4 => !c,              // CC
        5 => c,               // CS
        6 => !z,              // NE
        7 => z,               // EQ
        8 => !v,              // VC
        9 => v,               // VS
        10 => !n,             // PL
        11 => n,              // MI
        12 => n == v,         // GE
        13 => n != v,         // LT
        14 => !z && (n == v), // GT
        15 => z || (n != v),  // LE
        _ => unreachable!(),
    }
}

// --- Line 0: immediates, bit ops, MOVEP ----------------------------------

fn line_0<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    if op & 0x0100 != 0 {
        // Dynamic bit ops, or MOVEP when EA mode is 001.
        if (op >> 3) & 7 == 1 {
            return movep(cpu, bus, op);
        }
        let bitnum = cpu.d[((op >> 9) & 7) as usize];
        return bit_op(cpu, bus, op, bitnum, false);
    }
    let kind = (op >> 9) & 7;
    if kind == 4 {
        // Static bit ops: bit number in extension word.
        let bitnum = u32::from(cpu.fetch16(bus));
        return bit_op(cpu, bus, op, bitnum, true);
    }
    let size_bits = (op >> 6) & 3;
    let Some(size) = Size::from_bits(size_bits) else {
        return illegal(cpu, bus, op);
    };
    let mode = (op >> 3) & 7;
    let reg = op & 7;

    // ORI/ANDI/EORI to CCR/SR (EA field = immediate).
    if mode == 7 && reg == 4 && matches!(kind, 0 | 1 | 5) {
        let imm = cpu.fetch16(bus);
        cpu.cycles += 3;
        match size {
            Size::Byte => {
                let ccr = cpu.sr & sr_bits::CCR_MASK;
                let new = match kind {
                    0 => ccr | imm,
                    1 => ccr & imm,
                    _ => ccr ^ imm,
                };
                cpu.set_ccr(new);
            }
            Size::Word => {
                if !privileged(cpu, bus) {
                    return;
                }
                let new = match kind {
                    0 => cpu.sr | imm,
                    1 => cpu.sr & imm,
                    _ => cpu.sr ^ imm,
                };
                cpu.set_sr(new);
            }
            Size::Long => return illegal(cpu, bus, op),
        }
        return;
    }

    let imm = match size {
        Size::Byte => u32::from(cpu.fetch16(bus)) & 0xFF,
        Size::Word => u32::from(cpu.fetch16(bus)),
        Size::Long => cpu.fetch32(bus),
    };
    // Table 15: immediate arithmetic/logical instructions spend three
    // internal clocks beyond their opcode/immediate/operand transfers.
    cpu.cycles += 3;
    let ea = cpu.ea(bus, mode, reg, size);
    let dst = cpu.read_ea(bus, ea, size);
    match kind {
        0 => {
            let r = dst | imm;
            set_logic_flags(cpu, r, size);
            cpu.write_ea(bus, ea, size, r);
        }
        1 => {
            let r = dst & imm;
            set_logic_flags(cpu, r, size);
            cpu.write_ea(bus, ea, size, r);
        }
        2 => {
            let r = do_sub(cpu, dst, imm, size, false, true);
            cpu.write_ea(bus, ea, size, r);
        }
        3 => {
            let r = do_add(cpu, dst, imm, size, false);
            cpu.write_ea(bus, ea, size, r);
        }
        5 => {
            let r = dst ^ imm;
            set_logic_flags(cpu, r, size);
            cpu.write_ea(bus, ea, size, r);
        }
        6 => do_cmp(cpu, dst, imm, size),
        _ => illegal(cpu, bus, op),
    }
}

fn bit_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, bitnum: u32, static_bit: bool) {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let kind = (op >> 6) & 3; // 0=BTST 1=BCHG 2=BCLR 3=BSET
    cpu.cycles += match (static_bit, kind) {
        (false, 0) => 0,
        (false, _) | (true, 0) => 3,
        (true, _) => 6,
    };
    if mode == 0 {
        let bit = bitnum % 32;
        let r = reg as usize;
        let val = cpu.d[r];
        cpu.set_flag(SR_Z, val & (1 << bit) == 0);
        cpu.d[r] = match kind {
            0 => val,
            1 => val ^ (1 << bit),
            2 => val & !(1 << bit),
            _ => val | (1 << bit),
        };
    } else {
        let bit = bitnum % 8;
        let ea = cpu.ea(bus, mode, reg, Size::Byte);
        let val = cpu.read_ea(bus, ea, Size::Byte);
        cpu.set_flag(SR_Z, val & (1 << bit) == 0);
        if kind != 0 {
            let new = match kind {
                1 => val ^ (1 << bit),
                2 => val & !(1 << bit),
                _ => val | (1 << bit),
            };
            cpu.write_ea(bus, ea, Size::Byte, new);
        }
    }
}

fn movep<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let dreg = ((op >> 9) & 7) as usize;
    let areg = (op & 7) as usize;
    let opmode = (op >> 6) & 7; // 100/101 mem->reg, 110/111 reg->mem
    let long = opmode & 1 != 0;
    let disp = cpu.fetch16(bus) as i16 as u32;
    let addr = cpu.a[areg].wrapping_add(disp);
    let fc = cpu.data_fc();
    let count = if long { 4 } else { 2 };
    if opmode & 2 == 0 {
        let mut val: u32 = 0;
        for i in 0..count {
            let b = cpu.read8(bus, addr.wrapping_add(i * 2), fc);
            val = (val << 8) | u32::from(b);
        }
        if long {
            cpu.d[dreg] = val;
        } else {
            cpu.d[dreg] = (cpu.d[dreg] & 0xFFFF_0000) | val;
        }
        cpu.cycles += 3 * u64::from(count - 1);
    } else {
        let val = cpu.d[dreg];
        for i in 0..count {
            let shift = 8 * (count - 1 - i);
            cpu.write8(bus, addr.wrapping_add(i * 2), (val >> shift) as u8, fc);
        }
        cpu.cycles += 3 * u64::from(count);
    }
}

// --- MOVE / MOVEA --------------------------------------------------------

fn move_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, size: Size) {
    let src_mode = (op >> 3) & 7;
    let src_reg = op & 7;
    let dst_reg = (op >> 9) & 7;
    let dst_mode = (op >> 6) & 7;
    let src = {
        let ea = cpu.ea(bus, src_mode, src_reg, size);
        cpu.read_ea(bus, ea, size)
    };
    if dst_mode == 1 {
        // MOVEA: word source sign-extends, no flags. Byte size is illegal.
        if size == Size::Byte {
            return illegal(cpu, bus, op);
        }
        cpu.a[dst_reg as usize] = size.sext(src);
        return;
    }
    let dst_ea = cpu.ea(bus, dst_mode, dst_reg, size);
    set_logic_flags(cpu, src, size);
    cpu.write_ea(bus, dst_ea, size, src);
}

// --- Line 4: miscellaneous -----------------------------------------------

fn line_4<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let mode = (op >> 3) & 7;
    let reg = op & 7;

    match op {
        0x4E70 => {
            // RESET: asserts the reset output; no internal effect.
            if privileged(cpu, bus) {
                cpu.cycles += 147;
            }
            return;
        }
        0x4E71 => return, // NOP
        0x4E72 => {
            if privileged(cpu, bus) {
                let imm = cpu.fetch16(bus);
                cpu.cycles += 6;
                cpu.set_sr(imm);
                cpu.stopped = true;
            }
            return;
        }
        0x4E73 => {
            // RTE with 68070 format/vector word (68010-style short frame).
            if privileged(cpu, bus) {
                let sr = cpu.pop16(bus);
                let pc = cpu.pop32(bus);
                let format = cpu.pop16(bus);
                // Short-form RTE is 39 clocks (Table 22). Opcode and stack
                // transfers account for 23 of them.
                cpu.cycles += 16;
                if format & 0xF000 != 0 {
                    log::warn!("RTE long frame (format {:#06x}) not implemented", format);
                }
                cpu.set_sr(sr);
                cpu.set_pc_checked(bus, pc);
            }
            return;
        }
        0x4E75 => {
            let ret = cpu.pop32(bus);
            cpu.set_pc_checked(bus, ret);
            return;
        }
        0x4E76 => {
            if cpu.flag(SR_V) {
                cpu.exception(bus, Vector::TrapV as u8);
            } else {
                cpu.cycles += 3;
            }
            return;
        }
        0x4E77 => {
            let ccr = cpu.pop16(bus);
            cpu.set_ccr(ccr);
            let ret = cpu.pop32(bus);
            cpu.cycles += 3;
            cpu.set_pc_checked(bus, ret);
            return;
        }
        0x4AFC => return illegal(cpu, bus, op),
        _ => {}
    }

    if op & 0xFFF0 == 0x4E40 {
        // TRAP #n
        let vector = 32 + (op & 0xF) as u8;
        return cpu.exception(bus, vector);
    }
    match op & 0xFFF8 {
        0x4E50 => {
            // LINK An,#disp
            let r = reg as usize;
            let disp = cpu.fetch16(bus) as i16 as u32;
            let val = cpu.a[r];
            cpu.push32(bus, val);
            cpu.cycles += 6;
            cpu.a[r] = cpu.a[7];
            cpu.a[7] = cpu.a[7].wrapping_add(disp);
            return;
        }
        0x4E58 => {
            // UNLK An
            let r = reg as usize;
            cpu.a[7] = cpu.a[r];
            cpu.a[r] = cpu.pop32(bus);
            return;
        }
        0x4E60 => {
            // MOVE An,USP
            if privileged(cpu, bus) {
                cpu.sp_other = cpu.a[reg as usize];
            }
            return;
        }
        0x4E68 => {
            // MOVE USP,An
            if privileged(cpu, bus) {
                cpu.a[reg as usize] = cpu.sp_other;
            }
            return;
        }
        0x4840 if mode == 0 => {
            // SWAP Dn
            let r = reg as usize;
            let v = cpu.d[r].rotate_left(16);
            cpu.d[r] = v;
            set_logic_flags(cpu, v, Size::Long);
            return;
        }
        0x4880 | 0x48C0 if mode == 0 => {
            // EXT.W / EXT.L
            let r = reg as usize;
            if op & 0x0040 == 0 {
                let v = cpu.d[r] as u8 as i8 as u32;
                cpu.d[r] = (cpu.d[r] & 0xFFFF_0000) | (v & 0xFFFF);
                set_nz(cpu, v, Size::Word);
            } else {
                let v = cpu.d[r] as u16 as i16 as u32;
                cpu.d[r] = v;
                set_nz(cpu, v, Size::Long);
            }
            cpu.set_flag(SR_V, false);
            cpu.set_flag(SR_C, false);
            return;
        }
        _ => {}
    }

    match op & 0xFFC0 {
        0x40C0 => {
            // MOVE from SR (privileged on the 68070, unlike the 68000).
            if privileged(cpu, bus) {
                let ea = cpu.ea(bus, mode, reg, Size::Word);
                let sr = cpu.sr;
                cpu.write_ea(bus, ea, Size::Word, u32::from(sr));
                if mode != 0 {
                    cpu.cycles += 4;
                }
            }
            return;
        }
        0x42C0 => {
            // MOVE from CCR (68010/68070 addition).
            let ea = cpu.ea(bus, mode, reg, Size::Word);
            let ccr = u32::from(cpu.sr & sr_bits::CCR_MASK);
            cpu.write_ea(bus, ea, Size::Word, ccr);
            if mode != 0 {
                cpu.cycles += 4;
            }
            return;
        }
        0x44C0 => {
            // MOVE to CCR
            let ea = cpu.ea(bus, mode, reg, Size::Word);
            let v = cpu.read_ea(bus, ea, Size::Word) as u16;
            cpu.cycles += 3;
            cpu.set_ccr(v);
            return;
        }
        0x46C0 => {
            // MOVE to SR
            if privileged(cpu, bus) {
                let ea = cpu.ea(bus, mode, reg, Size::Word);
                let v = cpu.read_ea(bus, ea, Size::Word) as u16;
                cpu.cycles += 3;
                cpu.set_sr(v);
            }
            return;
        }
        0x4800 => {
            // NBCD
            let ea = cpu.ea(bus, mode, reg, Size::Byte);
            let dst = cpu.read_ea(bus, ea, Size::Byte);
            let r = nbcd(cpu, dst);
            cpu.write_ea(bus, ea, Size::Byte, r);
            cpu.cycles += 3;
            return;
        }
        0x4840 => {
            // PEA
            let addr = cpu.control_addr(bus, mode, reg);
            cpu.push32(bus, addr);
            cpu.cycles += 3;
            return;
        }
        0x4AC0 => {
            // TAS
            let ea = cpu.ea(bus, mode, reg, Size::Byte);
            let v = cpu.read_ea(bus, ea, Size::Byte);
            set_nz(cpu, v, Size::Byte);
            cpu.set_flag(SR_V, false);
            cpu.set_flag(SR_C, false);
            cpu.write_ea(bus, ea, Size::Byte, v | 0x80);
            cpu.cycles += if mode == 0 { 3 } else { 4 };
            return;
        }
        0x4E80 => {
            // JSR
            let addr = cpu.control_addr(bus, mode, reg);
            let ret = cpu.pc;
            cpu.push32(bus, ret);
            cpu.cycles += 3;
            cpu.set_pc_checked(bus, addr);
            return;
        }
        0x4EC0 => {
            // JMP
            let addr = cpu.control_addr(bus, mode, reg);
            cpu.set_pc_checked(bus, addr);
            return;
        }
        _ => {}
    }

    // MOVEM
    if op & 0xFB80 == 0x4880 {
        return movem(cpu, bus, op);
    }

    // LEA / CHK
    match (op >> 6) & 7 {
        7 => {
            // LEA
            let addr = cpu.control_addr(bus, mode, reg);
            cpu.a[((op >> 9) & 7) as usize] = addr;
            return;
        }
        6 => {
            // CHK.W — Z/V/C modeling of the undefined flags follows MAME.
            let ea = cpu.ea(bus, mode, reg, Size::Word);
            let bound = cpu.read_ea(bus, ea, Size::Word) as u16 as i16;
            let val = cpu.d[((op >> 9) & 7) as usize] as u16 as i16;
            cpu.set_flag(SR_Z, val == 0);
            cpu.set_flag(SR_V, false);
            cpu.set_flag(SR_C, false);
            let trapped = if val < 0 {
                cpu.set_flag(SR_N, true);
                cpu.exception(bus, Vector::Chk as u8);
                true
            } else if val > bound {
                cpu.set_flag(SR_N, false);
                cpu.exception(bus, Vector::Chk as u8);
                true
            } else {
                cpu.set_flag(SR_N, val < 0);
                false
            };
            if !trapped {
                cpu.cycles += 12;
            }
            return;
        }
        _ => {}
    }

    // NEGX / CLR / NEG / NOT / TST (single-operand, sized)
    let Some(size) = Size::from_bits((op >> 6) & 3) else {
        return illegal(cpu, bus, op);
    };
    match op & 0xFF00 {
        0x4000 => {
            let ea = cpu.ea(bus, mode, reg, size);
            let dst = cpu.read_ea(bus, ea, size);
            let r = do_sub(cpu, 0, dst, size, true, true);
            cpu.write_ea(bus, ea, size, r);
        }
        0x4200 => {
            let ea = cpu.ea(bus, mode, reg, size);
            // CLR performs a read cycle before writing on the 68000 family.
            let _ = cpu.read_ea(bus, ea, size);
            set_logic_flags(cpu, 0, size);
            cpu.write_ea(bus, ea, size, 0);
        }
        0x4400 => {
            let ea = cpu.ea(bus, mode, reg, size);
            let dst = cpu.read_ea(bus, ea, size);
            let r = do_sub(cpu, 0, dst, size, false, true);
            cpu.write_ea(bus, ea, size, r);
        }
        0x4600 => {
            let ea = cpu.ea(bus, mode, reg, size);
            let dst = cpu.read_ea(bus, ea, size);
            let r = !dst & size.mask();
            set_logic_flags(cpu, r, size);
            cpu.write_ea(bus, ea, size, r);
        }
        0x4A00 => {
            let ea = cpu.ea(bus, mode, reg, size);
            let v = cpu.read_ea(bus, ea, size);
            set_logic_flags(cpu, v, size);
        }
        _ => illegal(cpu, bus, op),
    }
}

fn movem<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let timing_start = cpu.cycles;
    let to_regs = op & 0x0400 != 0;
    let size = if op & 0x0040 != 0 {
        Size::Long
    } else {
        Size::Word
    };
    let mode = (op >> 3) & 7;
    let reg = (op & 7) as usize;
    let mask = cpu.fetch16(bus);
    let register_count = u64::from(mask.count_ones());
    let fc = cpu.data_fc();
    let step = size.bytes();
    let base = if to_regs {
        match (mode, op & 7) {
            (2 | 3, _) => 26,
            (5, _) | (7, 0 | 2) => 30,
            (6, _) | (7, 3) => 33,
            (7, 1) => 34,
            _ => 26,
        }
    } else {
        match (mode, op & 7) {
            (2 | 4, _) => 23,
            (5, _) | (7, 0) => 27,
            (6, _) => 30,
            (7, 1) => 31,
            _ => 23,
        }
    };
    let per_register = if size == Size::Long { 11 } else { 7 };
    let total_timing = base + per_register * register_count;

    let read_reg = |cpu: &Cpu, idx: usize| -> u32 {
        if idx < 8 {
            cpu.d[idx]
        } else {
            cpu.a[idx - 8]
        }
    };

    if mode == 4 && !to_regs {
        // Registers to memory, predecrement: mask bit 0 = A7 … bit 15 = D0.
        // The initial value of the base register is what gets stored if it
        // appears in the list.
        let initial = cpu.a[reg];
        let mut addr = initial;
        for bit in 0..16 {
            if mask & (1 << bit) == 0 {
                continue;
            }
            let idx = 15 - bit;
            addr = addr.wrapping_sub(step);
            let val = if idx == reg + 8 {
                initial
            } else {
                read_reg(cpu, idx)
            };
            match size {
                Size::Word => cpu.write16(bus, addr, val as u16, fc),
                _ => cpu.write32(bus, addr, val, fc),
            }
        }
        cpu.a[reg] = addr;
        finish_total_timing(cpu, timing_start, total_timing);
        return;
    }

    let mut addr = if mode == 3 {
        cpu.a[reg]
    } else {
        cpu.control_addr(bus, mode, op & 7)
    };
    for bit in 0..16 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let idx = bit;
        if to_regs {
            let val = match size {
                Size::Word => cpu.read16(bus, addr, fc) as i16 as u32,
                _ => cpu.read32(bus, addr, fc),
            };
            if idx < 8 {
                cpu.d[idx] = val;
            } else {
                cpu.a[idx - 8] = val;
            }
        } else {
            let val = read_reg(cpu, idx);
            match size {
                Size::Word => cpu.write16(bus, addr, val as u16, fc),
                _ => cpu.write32(bus, addr, val, fc),
            }
        }
        addr = addr.wrapping_add(step);
    }
    if mode == 3 && to_regs {
        cpu.a[reg] = addr;
    }
    finish_total_timing(cpu, timing_start, total_timing);
}

// --- Line 5: ADDQ/SUBQ/Scc/DBcc ------------------------------------------

fn line_5<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if (op >> 6) & 3 == 3 {
        let cc = (op >> 8) & 0xF;
        if mode == 1 {
            // DBcc
            let disp_pc = cpu.pc;
            let disp = cpu.fetch16(bus) as i16 as u32;
            if condition(cpu, cc) {
                cpu.cycles += 3;
            } else {
                let r = reg as usize;
                let counter = (cpu.d[r] as u16).wrapping_sub(1);
                cpu.d[r] = (cpu.d[r] & 0xFFFF_0000) | u32::from(counter);
                if counter != 0xFFFF {
                    cpu.set_pc_checked(bus, disp_pc.wrapping_add(disp));
                }
                cpu.cycles += 6;
            }
        } else {
            // Scc
            let ea = cpu.ea(bus, mode, reg, Size::Byte);
            let v = if condition(cpu, cc) { 0xFF } else { 0x00 };
            cpu.write_ea(bus, ea, Size::Byte, v);
            cpu.cycles += if mode == 0 { 6 } else { 10 };
        }
        return;
    }
    let Some(size) = Size::from_bits((op >> 6) & 3) else {
        return illegal(cpu, bus, op);
    };
    let mut imm = u32::from((op >> 9) & 7);
    if imm == 0 {
        imm = 8;
    }
    if mode == 1 {
        // ADDQ/SUBQ to An: whole register, no flags; byte size illegal.
        if size == Size::Byte {
            return illegal(cpu, bus, op);
        }
        let r = reg as usize;
        cpu.a[r] = if op & 0x0100 == 0 {
            cpu.a[r].wrapping_add(imm)
        } else {
            cpu.a[r].wrapping_sub(imm)
        };
        return;
    }
    let ea = cpu.ea(bus, mode, reg, size);
    let dst = cpu.read_ea(bus, ea, size);
    let r = if op & 0x0100 == 0 {
        do_add(cpu, dst, imm, size, false)
    } else {
        do_sub(cpu, dst, imm, size, false, true)
    };
    cpu.write_ea(bus, ea, size, r);
}

// --- Line 6: Bcc/BRA/BSR -------------------------------------------------

fn line_6<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let cc = (op >> 8) & 0xF;
    let disp8 = op as u8;
    let base_pc = cpu.pc;
    let disp = if disp8 == 0 {
        cpu.fetch16(bus) as i16 as u32
    } else {
        disp8 as i8 as u32
    };
    let target = base_pc.wrapping_add(disp);
    // Table 19: Bcc/BRA are 13 clocks for an 8-bit displacement and 14
    // for a fetched word; BSR's stack writes bring it to 21/25.
    if cc == 1 || disp8 != 0 {
        cpu.cycles += 6;
    } else {
        cpu.cycles += 3;
    }
    match cc {
        0 => cpu.set_pc_checked(bus, target), // BRA
        1 => {
            // BSR
            let ret = cpu.pc;
            cpu.push32(bus, ret);
            cpu.set_pc_checked(bus, target);
        }
        _ => {
            if condition(cpu, cc) {
                cpu.set_pc_checked(bus, target);
            }
        }
    }
}

fn moveq(cpu: &mut Cpu, op: u16) {
    if op & 0x0100 != 0 {
        // 0111 rrr 1 ... is illegal; handled lazily as MOVEQ pattern only.
        log::debug!("line-7 opcode {op:#06x} with bit 8 set");
    }
    let r = ((op >> 9) & 7) as usize;
    let v = op as u8 as i8 as u32;
    cpu.d[r] = v;
    set_logic_flags(cpu, v, Size::Long);
}

// --- Lines 8/9/B/C/D: two-operand ALU ------------------------------------

fn line_8<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let opmode = (op >> 6) & 7;
    match opmode {
        3 => return div_op(cpu, bus, op, false),
        7 => return div_op(cpu, bus, op, true),
        4 if (op >> 4) & 3 == 0 => return bcd_op(cpu, bus, op, false),
        _ => {}
    }
    logic_op(cpu, bus, op, |a, b| a | b);
}

fn line_c<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let opmode = (op >> 6) & 7;
    match opmode {
        3 => return mul_op(cpu, bus, op, false),
        7 => return mul_op(cpu, bus, op, true),
        4 if (op >> 4) & 3 == 0 => return bcd_op(cpu, bus, op, true),
        _ => {}
    }
    // EXG patterns: C140 (Dn,Dn), C148 (An,An), C188 (Dn,An).
    if matches!(op & 0x01F8, 0x0140 | 0x0148 | 0x0188) {
        let rx = ((op >> 9) & 7) as usize;
        let ry = (op & 7) as usize;
        match op & 0x01F8 {
            0x0140 => cpu.d.swap(rx, ry),
            0x0148 => cpu.a.swap(rx, ry),
            _ => std::mem::swap(&mut cpu.d[rx], &mut cpu.a[ry]),
        }
        cpu.cycles += 6;
        return;
    }
    logic_op(cpu, bus, op, |a, b| a & b);
}

/// AND/OR with direction bit: opmode 0-2 = <ea> op Dn -> Dn, 4-6 = Dn op <ea> -> <ea>.
fn logic_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, f: fn(u32, u32) -> u32) {
    let dn = ((op >> 9) & 7) as usize;
    let opmode = (op >> 6) & 7;
    let Some(size) = Size::from_bits(opmode & 3) else {
        return illegal(cpu, bus, op);
    };
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let ea = cpu.ea(bus, mode, reg, size);
    if opmode & 4 == 0 {
        let src = cpu.read_ea(bus, ea, size);
        let r = f(cpu.d[dn] & size.mask(), src);
        set_logic_flags(cpu, r, size);
        cpu.d[dn] = (cpu.d[dn] & !size.mask()) | (r & size.mask());
    } else {
        let dst = cpu.read_ea(bus, ea, size);
        let r = f(dst, cpu.d[dn] & size.mask());
        set_logic_flags(cpu, r, size);
        cpu.write_ea(bus, ea, size, r);
    }
}

fn div_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, signed: bool) {
    let dn = ((op >> 9) & 7) as usize;
    let ea = cpu.ea(bus, (op >> 3) & 7, op & 7, Size::Word);
    let src = cpu.read_ea(bus, ea, Size::Word);
    if src & 0xFFFF == 0 {
        cpu.exception(bus, Vector::ZeroDivide as u8);
        return;
    }
    cpu.cycles += if signed { 162 } else { 123 };
    let dst = cpu.d[dn];
    if signed {
        let divisor = src as u16 as i16 as i32;
        let dividend = dst as i32;
        let quotient = dividend.wrapping_div(divisor);
        let remainder = dividend.wrapping_rem(divisor);
        if quotient > i32::from(i16::MAX) || quotient < i32::from(i16::MIN) {
            // Overflow: operands unchanged; N set, Z clear, V set (MAME's
            // modeling of the undefined flags).
            cpu.set_flag(SR_V, true);
            cpu.set_flag(SR_C, false);
            cpu.set_flag(SR_N, true);
            cpu.set_flag(SR_Z, false);
            return;
        }
        cpu.d[dn] = ((remainder as u32 & 0xFFFF) << 16) | (quotient as u32 & 0xFFFF);
        set_nz(cpu, quotient as u32, Size::Word);
        cpu.set_flag(SR_V, false);
        cpu.set_flag(SR_C, false);
    } else {
        let divisor = src & 0xFFFF;
        let quotient = dst / divisor;
        let remainder = dst % divisor;
        if quotient > 0xFFFF {
            cpu.set_flag(SR_V, true);
            cpu.set_flag(SR_C, false);
            cpu.set_flag(SR_N, true);
            cpu.set_flag(SR_Z, false);
            return;
        }
        cpu.d[dn] = (remainder << 16) | quotient;
        set_nz(cpu, quotient, Size::Word);
        cpu.set_flag(SR_V, false);
        cpu.set_flag(SR_C, false);
    }
}

fn mul_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, signed: bool) {
    let dn = ((op >> 9) & 7) as usize;
    let ea = cpu.ea(bus, (op >> 3) & 7, op & 7, Size::Word);
    let src = cpu.read_ea(bus, ea, Size::Word);
    let dst = cpu.d[dn] & 0xFFFF;
    let result = if signed {
        ((src as u16 as i16 as i32) * (dst as u16 as i16 as i32)) as u32
    } else {
        src * dst
    };
    cpu.d[dn] = result;
    set_logic_flags(cpu, result, Size::Long);
    cpu.cycles += 69;
}

fn bcd_op<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, add: bool) {
    // ABCD/SBCD: register or -(Ay),-(Ax) form.
    let rx = ((op >> 9) & 7) as usize;
    let ry = (op & 7) as usize;
    let mem_form = op & 0x0008 != 0;
    if mem_form {
        let src_ea = cpu.ea(bus, 4, ry as u16, Size::Byte);
        let src = cpu.read_ea(bus, src_ea, Size::Byte);
        let dst_ea = cpu.ea(bus, 4, rx as u16, Size::Byte);
        let dst = cpu.read_ea(bus, dst_ea, Size::Byte);
        let r = if add {
            bcd_add(cpu, dst, src)
        } else {
            bcd_sub(cpu, dst, src)
        };
        cpu.write_ea(bus, dst_ea, Size::Byte, r);
        cpu.cycles += 6;
    } else {
        let src = cpu.d[ry] & 0xFF;
        let dst = cpu.d[rx] & 0xFF;
        let r = if add {
            bcd_add(cpu, dst, src)
        } else {
            bcd_sub(cpu, dst, src)
        };
        cpu.d[rx] = (cpu.d[rx] & !0xFF) | r;
        cpu.cycles += 3;
    }
}

// BCD flag behavior (including the officially-undefined N/V/C bits) follows
// MAME's microcoded 68000 ALU (`alu_abcd8`/`alu_sbcd8` in
// src/devices/cpu/m68000/m68000.h), which the conformance vectors were
// generated from. Z is sticky (only cleared by a non-zero result).

fn bcd_add(cpu: &mut Cpu, dst: u32, src: u32) -> u32 {
    let x = u32::from(cpu.flag(SR_X));
    let hr = (dst & 0xF) + (src & 0xF) + x;
    let lcor = hr > 9;
    let r1 = (dst & 0xFF) + (src & 0xFF) + x;
    let mut r = r1 + if lcor { 6 } else { 0 };
    if r > 0x9F {
        r += 0x60;
    }
    let carry = r & 0x300 != 0;
    cpu.set_flag(SR_C, carry);
    cpu.set_flag(SR_X, carry);
    cpu.set_flag(SR_V, r & 0x80 != 0 && r1 & 0x80 == 0);
    cpu.set_flag(SR_N, r & 0x80 != 0);
    if r & 0xFF != 0 {
        cpu.set_flag(SR_Z, false);
    }
    r & 0xFF
}

fn bcd_sub(cpu: &mut Cpu, dst: u32, src: u32) -> u32 {
    let x = u32::from(cpu.flag(SR_X));
    let hr = (dst & 0xF).wrapping_sub(src & 0xF).wrapping_sub(x);
    let lcor = hr & 0x10 != 0;
    let r1 = ((dst & 0xFF) as u16)
        .wrapping_sub((src & 0xFF) as u16)
        .wrapping_sub(x as u16);
    let mut r = r1.wrapping_sub(if lcor { 6 } else { 0 });
    if r1 & 0x100 != 0 {
        r = r.wrapping_sub(0x60);
    }
    let borrow = r & 0x300 != 0;
    cpu.set_flag(SR_C, borrow);
    cpu.set_flag(SR_X, borrow);
    cpu.set_flag(SR_V, r & 0x80 == 0 && r1 & 0x80 != 0);
    cpu.set_flag(SR_N, r & 0x80 != 0);
    if r & 0xFF != 0 {
        cpu.set_flag(SR_Z, false);
    }
    u32::from(r & 0xFF)
}

fn nbcd(cpu: &mut Cpu, dst: u32) -> u32 {
    bcd_sub(cpu, 0, dst)
}

// --- Lines 9/D: SUB/ADD (+A, +X forms) -----------------------------------

fn add_sub<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16, add: bool) {
    let rn = ((op >> 9) & 7) as usize;
    let opmode = (op >> 6) & 7;
    if opmode == 3 || opmode == 7 {
        // ADDA/SUBA
        let size = if opmode == 3 { Size::Word } else { Size::Long };
        let ea = cpu.ea(bus, (op >> 3) & 7, op & 7, size);
        let src = size.sext(cpu.read_ea(bus, ea, size));
        cpu.a[rn] = if add {
            cpu.a[rn].wrapping_add(src)
        } else {
            cpu.a[rn].wrapping_sub(src)
        };
        return;
    }
    let Some(size) = Size::from_bits(opmode & 3) else {
        return illegal(cpu, bus, op);
    };
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if opmode & 4 != 0 && mode <= 1 {
        // ADDX/SUBX (register form mode=0, memory form mode=1)
        if mode == 0 {
            let ry = reg as usize;
            let src = cpu.d[ry];
            let dst = cpu.d[rn];
            let r = if add {
                do_add(cpu, dst, src, size, true)
            } else {
                do_sub(cpu, dst, src, size, true, true)
            };
            cpu.d[rn] = (cpu.d[rn] & !size.mask()) | (r & size.mask());
        } else {
            let src_ea = cpu.ea(bus, 4, reg, size);
            let src = cpu.read_ea(bus, src_ea, size);
            let dst_ea = cpu.ea(bus, 4, rn as u16, size);
            let dst = cpu.read_ea(bus, dst_ea, size);
            let r = if add {
                do_add(cpu, dst, src, size, true)
            } else {
                do_sub(cpu, dst, src, size, true, true)
            };
            cpu.write_ea(bus, dst_ea, size, r);
            cpu.cycles += 3;
        }
        return;
    }
    let ea = cpu.ea(bus, mode, reg, size);
    if opmode & 4 == 0 {
        // <ea> op Dn -> Dn
        let src = cpu.read_ea(bus, ea, size);
        let dst = cpu.d[rn];
        let r = if add {
            do_add(cpu, dst, src, size, false)
        } else {
            do_sub(cpu, dst, src, size, false, true)
        };
        cpu.d[rn] = (cpu.d[rn] & !size.mask()) | (r & size.mask());
    } else {
        // Dn op <ea> -> <ea>
        let dst = cpu.read_ea(bus, ea, size);
        let src = cpu.d[rn];
        let r = if add {
            do_add(cpu, dst, src, size, false)
        } else {
            do_sub(cpu, dst, src, size, false, true)
        };
        cpu.write_ea(bus, ea, size, r);
    }
}

// --- Line B: CMP/CMPA/CMPM/EOR -------------------------------------------

fn line_b<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let rn = ((op >> 9) & 7) as usize;
    let opmode = (op >> 6) & 7;
    if opmode == 3 || opmode == 7 {
        // CMPA
        let size = if opmode == 3 { Size::Word } else { Size::Long };
        let ea = cpu.ea(bus, (op >> 3) & 7, op & 7, size);
        let src = size.sext(cpu.read_ea(bus, ea, size));
        do_cmp(cpu, cpu.a[rn], src, Size::Long);
        return;
    }
    let Some(size) = Size::from_bits(opmode & 3) else {
        return illegal(cpu, bus, op);
    };
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    if opmode & 4 == 0 {
        // CMP <ea>,Dn
        let ea = cpu.ea(bus, mode, reg, size);
        let src = cpu.read_ea(bus, ea, size);
        do_cmp(cpu, cpu.d[rn], src, size);
    } else if mode == 1 {
        // CMPM (Ay)+,(Ax)+
        let src_ea = cpu.ea(bus, 3, reg, size);
        let src = cpu.read_ea(bus, src_ea, size);
        let dst_ea = cpu.ea(bus, 3, rn as u16, size);
        let dst = cpu.read_ea(bus, dst_ea, size);
        do_cmp(cpu, dst, src, size);
        cpu.cycles += 3;
    } else {
        // EOR Dn,<ea>
        let ea = cpu.ea(bus, mode, reg, size);
        let dst = cpu.read_ea(bus, ea, size);
        let r = dst ^ (cpu.d[rn] & size.mask());
        set_logic_flags(cpu, r, size);
        cpu.write_ea(bus, ea, size, r);
    }
}

// --- Line E: shifts and rotates ------------------------------------------

fn line_e<B: Bus68k>(cpu: &mut Cpu, bus: &mut B, op: u16) {
    let size_bits = (op >> 6) & 3;
    if size_bits == 3 {
        // Memory shift: word, count 1. Kind in bits 10-9, direction bit 8.
        let kind = (op >> 9) & 3;
        let left = op & 0x0100 != 0;
        let ea = cpu.ea(bus, (op >> 3) & 7, op & 7, Size::Word);
        let val = cpu.read_ea(bus, ea, Size::Word);
        let r = shift_val(cpu, kind, left, val, 1, Size::Word);
        cpu.write_ea(bus, ea, Size::Word, r);
        cpu.cycles += 3;
        return;
    }
    let Some(size) = Size::from_bits(size_bits) else {
        return illegal(cpu, bus, op);
    };
    let reg = (op & 7) as usize;
    let kind = (op >> 3) & 3;
    let left = op & 0x0100 != 0;
    let count = if op & 0x0020 != 0 {
        cpu.d[((op >> 9) & 7) as usize] % 64
    } else {
        let c = u32::from((op >> 9) & 7);
        if c == 0 {
            8
        } else {
            c
        }
    };
    let val = cpu.d[reg] & size.mask();
    let r = shift_val(cpu, kind, left, val, count, size);
    cpu.d[reg] = (cpu.d[reg] & !size.mask()) | (r & size.mask());
    cpu.cycles += 6 + 3 * u64::from(count);
}

/// kind: 0=AS, 1=LS, 2=ROX, 3=RO
fn shift_val(cpu: &mut Cpu, kind: u16, left: bool, val: u32, count: u32, size: Size) -> u32 {
    let bits = size.bytes() * 8;
    let msb = size.msb();
    let mask = size.mask();
    let mut v = val & mask;

    if count == 0 {
        // Flags for zero count: N/Z from value, V=0, C=0 (C=X for ROX).
        cpu.set_flag(SR_V, false);
        cpu.set_flag(SR_C, if kind == 2 { cpu.flag(SR_X) } else { false });
        set_nz(cpu, v, size);
        return v;
    }

    let mut carry = false;
    let mut overflow = false;
    for _ in 0..count {
        match (kind, left) {
            (0, true) | (1, true) => {
                carry = v & msb != 0;
                let new = (v << 1) & mask;
                if kind == 0 && (new & msb) != (v & msb) {
                    overflow = true;
                }
                v = new;
            }
            (0, false) => {
                carry = v & 1 != 0;
                v = ((v >> 1) | (v & msb)) & mask;
            }
            (1, false) => {
                carry = v & 1 != 0;
                v >>= 1;
            }
            (2, true) => {
                let x = u32::from(cpu.flag(SR_X));
                carry = v & msb != 0;
                v = ((v << 1) | x) & mask;
                cpu.set_flag(SR_X, carry);
            }
            (2, false) => {
                let x = u32::from(cpu.flag(SR_X));
                carry = v & 1 != 0;
                v = (v >> 1) | (x << (bits - 1));
                cpu.set_flag(SR_X, carry);
            }
            (3, true) => {
                carry = v & msb != 0;
                v = ((v << 1) | u32::from(carry)) & mask;
            }
            (3, false) => {
                carry = v & 1 != 0;
                v = (v >> 1) | (u32::from(carry) << (bits - 1));
            }
            _ => unreachable!(),
        }
    }
    cpu.set_flag(SR_C, carry);
    if kind <= 1 {
        cpu.set_flag(SR_X, carry);
    }
    cpu.set_flag(SR_V, if kind == 0 { overflow } else { false });
    set_nz(cpu, v, size);
    v
}
