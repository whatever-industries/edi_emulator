// SPDX-License-Identifier: GPL-3.0-or-later
//! Effective-address calculation and operand access.

use crate::bus::Bus68k;
use crate::cpu::Cpu;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Byte,
    Word,
    Long,
}

impl Size {
    pub fn bytes(self) -> u32 {
        match self {
            Size::Byte => 1,
            Size::Word => 2,
            Size::Long => 4,
        }
    }

    /// Decode the standard 2-bit size field (00=B, 01=W, 10=L).
    pub fn from_bits(bits: u16) -> Option<Size> {
        match bits {
            0 => Some(Size::Byte),
            1 => Some(Size::Word),
            2 => Some(Size::Long),
            _ => None,
        }
    }

    pub fn mask(self) -> u32 {
        match self {
            Size::Byte => 0xFF,
            Size::Word => 0xFFFF,
            Size::Long => 0xFFFF_FFFF,
        }
    }

    pub fn msb(self) -> u32 {
        match self {
            Size::Byte => 0x80,
            Size::Word => 0x8000,
            Size::Long => 0x8000_0000,
        }
    }

    /// Sign-extend a value of this size to 32 bits.
    pub fn sext(self, v: u32) -> u32 {
        match self {
            Size::Byte => v as u8 as i8 as u32,
            Size::Word => v as u16 as i16 as u32,
            Size::Long => v,
        }
    }
}

/// A resolved operand location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ea {
    Dn(usize),
    An(usize),
    Mem(u32),
    /// Immediate value already fetched from the instruction stream.
    Imm(u32),
}

impl Cpu {
    /// Address-register increment/decrement step: byte accesses through A7
    /// move by 2 to keep the stack word-aligned.
    fn step_size(size: Size, reg: usize) -> u32 {
        if reg == 7 && size == Size::Byte {
            2
        } else {
            size.bytes()
        }
    }

    /// Decode a brief extension word (d8(An,Xn) / d8(PC,Xn)).
    fn indexed_addr<B: Bus68k>(&mut self, bus: &mut B, base: u32) -> u32 {
        let ext = self.fetch16(bus);
        let disp = ext as u8 as i8 as u32;
        let reg = ((ext >> 12) & 7) as usize;
        let idx_raw = if ext & 0x8000 != 0 {
            self.a[reg]
        } else {
            self.d[reg]
        };
        let idx = if ext & 0x0800 != 0 {
            idx_raw
        } else {
            idx_raw as u16 as i16 as u32
        };
        base.wrapping_add(disp).wrapping_add(idx)
    }

    /// Resolve an effective address, performing any pre/post-inc/dec side
    /// effects and extension-word fetches.
    pub fn ea<B: Bus68k>(&mut self, bus: &mut B, mode: u16, reg: u16, size: Size) -> Ea {
        let r = reg as usize;
        match mode {
            0 => Ea::Dn(r),
            1 => Ea::An(r),
            2 => Ea::Mem(self.a[r]),
            3 => {
                let addr = self.a[r];
                self.a[r] = addr.wrapping_add(Self::step_size(size, r));
                Ea::Mem(addr)
            }
            4 => {
                let addr = self.a[r].wrapping_sub(Self::step_size(size, r));
                self.a[r] = addr;
                Ea::Mem(addr)
            }
            5 => {
                let disp = self.fetch16(bus) as i16 as u32;
                Ea::Mem(self.a[r].wrapping_add(disp))
            }
            6 => {
                let base = self.a[r];
                Ea::Mem(self.indexed_addr(bus, base))
            }
            7 => match reg {
                0 => Ea::Mem(self.fetch16(bus) as i16 as u32),
                1 => Ea::Mem(self.fetch32(bus)),
                2 => {
                    let base = self.pc;
                    let disp = self.fetch16(bus) as i16 as u32;
                    Ea::Mem(base.wrapping_add(disp))
                }
                3 => {
                    let base = self.pc;
                    Ea::Mem(self.indexed_addr(bus, base))
                }
                4 => {
                    let v = match size {
                        Size::Byte => u32::from(self.fetch16(bus)) & 0xFF,
                        Size::Word => u32::from(self.fetch16(bus)),
                        Size::Long => self.fetch32(bus),
                    };
                    Ea::Imm(v)
                }
                _ => {
                    log::warn!("invalid EA mode 7/{reg} at pc={:#x}", self.pc);
                    Ea::Imm(0)
                }
            },
            _ => unreachable!("EA mode field is 3 bits"),
        }
    }

    /// Read an operand (masked to `size`).
    pub fn read_ea<B: Bus68k>(&mut self, bus: &mut B, ea: Ea, size: Size) -> u32 {
        match ea {
            Ea::Dn(r) => self.d[r] & size.mask(),
            Ea::An(r) => self.a[r] & size.mask(),
            Ea::Imm(v) => v & size.mask(),
            Ea::Mem(addr) => {
                let fc = self.data_fc();
                match size {
                    Size::Byte => u32::from(self.read8(bus, addr, fc)),
                    Size::Word => u32::from(self.read16(bus, addr, fc)),
                    Size::Long => self.read32(bus, addr, fc),
                }
            }
        }
    }

    /// Write an operand. Data-register writes merge into the low bits;
    /// address-register writes always store the full sign-extended value.
    pub fn write_ea<B: Bus68k>(&mut self, bus: &mut B, ea: Ea, size: Size, v: u32) {
        match ea {
            Ea::Dn(r) => self.d[r] = (self.d[r] & !size.mask()) | (v & size.mask()),
            Ea::An(r) => self.a[r] = size.sext(v),
            Ea::Imm(_) => log::warn!("write to immediate EA at pc={:#x}", self.pc),
            Ea::Mem(addr) => {
                let fc = self.data_fc();
                match size {
                    Size::Byte => self.write8(bus, addr, v as u8, fc),
                    Size::Word => self.write16(bus, addr, v as u16, fc),
                    Size::Long => self.write32(bus, addr, v, fc),
                }
            }
        }
    }

    /// Control-mode address for LEA/PEA/JMP/JSR/MOVEM: like `ea` but never
    /// performs inc/dec side effects and yields the address itself.
    pub fn control_addr<B: Bus68k>(&mut self, bus: &mut B, mode: u16, reg: u16) -> u32 {
        let r = reg as usize;
        match mode {
            2 => self.a[r],
            5 => {
                let disp = self.fetch16(bus) as i16 as u32;
                self.a[r].wrapping_add(disp)
            }
            6 => {
                let base = self.a[r];
                self.indexed_addr(bus, base)
            }
            7 => match reg {
                0 => self.fetch16(bus) as i16 as u32,
                1 => self.fetch32(bus),
                2 => {
                    let base = self.pc;
                    let disp = self.fetch16(bus) as i16 as u32;
                    base.wrapping_add(disp)
                }
                3 => {
                    let base = self.pc;
                    self.indexed_addr(bus, base)
                }
                _ => {
                    log::warn!("invalid control EA 7/{reg} at pc={:#x}", self.pc);
                    0
                }
            },
            _ => {
                log::warn!("invalid control EA mode {mode} at pc={:#x}", self.pc);
                0
            }
        }
    }
}
