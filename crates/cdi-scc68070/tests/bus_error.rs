// SPDX-License-Identifier: GPL-3.0-or-later

use cdi_scc68070::bus::{Bus68k, BusAccessSize, BusError, FnCode, WaitTicks};
use cdi_scc68070::Cpu;

struct FaultBus {
    memory: Vec<u8>,
    fault_address: u32,
    pending: Option<BusError>,
}

impl FaultBus {
    fn new() -> Self {
        let mut memory = vec![0; 0x2000];
        memory[8..12].copy_from_slice(&0x0000_0100u32.to_be_bytes());
        memory[0x100..0x102].copy_from_slice(&0x4E73u16.to_be_bytes()); // RTE
        memory[0x200..0x202].copy_from_slice(&0x1010u16.to_be_bytes()); // MOVE.B (A0),D0
        Self {
            memory,
            fault_address: 0x0050_0000,
            pending: None,
        }
    }

    fn read_word(&self, address: usize) -> u16 {
        u16::from_be_bytes([self.memory[address], self.memory[address + 1]])
    }

    fn write_word(&mut self, address: usize, value: u16) {
        self.memory[address..address + 2].copy_from_slice(&value.to_be_bytes());
    }
}

impl Bus68k for FaultBus {
    fn read8(&mut self, address: u32, function_code: FnCode) -> (u8, WaitTicks) {
        if address == self.fault_address {
            self.pending = Some(BusError {
                address,
                function_code,
                size: BusAccessSize::Byte,
                read: true,
                write_data: 0,
            });
            return (0xFF, 0);
        }
        (self.memory[address as usize], 0)
    }

    fn read16(&mut self, address: u32, _function_code: FnCode) -> (u16, WaitTicks) {
        (self.read_word(address as usize), 0)
    }

    fn write8(&mut self, address: u32, value: u8, _function_code: FnCode) -> WaitTicks {
        self.memory[address as usize] = value;
        0
    }

    fn write16(&mut self, address: u32, value: u16, _function_code: FnCode) -> WaitTicks {
        self.write_word(address as usize, value);
        0
    }

    fn take_bus_error(&mut self) -> Option<BusError> {
        self.pending.take()
    }
}

#[test]
fn recoverable_bus_error_builds_long_frame_and_rr_suppresses_rerun() {
    let mut bus = FaultBus::new();
    let mut cpu = Cpu::new();
    cpu.pc = 0x200;
    cpu.a[0] = bus.fault_address;
    cpu.a[7] = 0x1000;

    assert_eq!(cpu.step(&mut bus), 158);
    assert_eq!(cpu.pc, 0x100);
    assert_eq!(cpu.a[7], 0x0FDE);
    assert_eq!(bus.read_word(0x0FDE), 0x2700); // SR
    assert_eq!(bus.read_word(0x0FE0), 0x0000); // PC high
    assert_eq!(bus.read_word(0x0FE2), 0x0200); // PC low
    assert_eq!(bus.read_word(0x0FE4), 0xF008); // long format, vector 2
    assert_eq!(bus.read_word(0x0FE6), 0x1705); // DF/BY/HB/RW, supervisor data
    assert_eq!(bus.read_word(0x0FEC), 0x0202); // internal resume PC low
    assert_eq!(bus.read_word(0x0FF2), 0x0050); // failed address high
    assert_eq!(bus.read_word(0x0FF4), 0x0000); // failed address low
    assert_eq!(bus.read_word(0x0FFA), 0x1010); // current instruction

    // A handler suppresses rerunning the failed cycle by setting SSW.RR.
    bus.write_word(0x0FE6, 0x9705);
    assert_eq!(cpu.step(&mut bus), 140);
    assert_eq!(cpu.pc, 0x202);
    assert_eq!(cpu.a[7], 0x1000);
}
