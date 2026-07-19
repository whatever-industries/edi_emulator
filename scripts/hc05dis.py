#!/usr/bin/env python3
"""MC68HC05 disassembler for the CD-i SLAVE/SERVO MCU dumps."""
import sys

BRANCHES = ["BRA","BRN","BHI","BLS","BCC","BCS","BNE","BEQ",
            "BHCC","BHCS","BPL","BMI","BMC","BMS","BIL","BIH"]
RMW = {0x0:"NEG",0x3:"COM",0x4:"LSR",0x6:"ROR",0x7:"ASR",
       0x8:"LSL",0x9:"ROL",0xA:"DEC",0xC:"INC",0xD:"TST",0xF:"CLR"}
ALU = {0x0:"SUB",0x1:"CMP",0x2:"SBC",0x3:"CPX",0x4:"AND",0x5:"BIT",
       0x6:"LDA",0x7:"STA",0x8:"EOR",0x9:"ADC",0xA:"ORA",0xB:"ADD",
       0xC:"JMP",0xD:"JSR",0xE:"LDX",0xF:"STX"}
INH8 = {0x80:"RTI",0x81:"RTS",0x83:"SWI",0x8E:"STOP",0x8F:"WAIT"}
INH9 = {0x97:"TAX",0x98:"CLC",0x99:"SEC",0x9A:"CLI",0x9B:"SEI",
        0x9C:"RSP",0x9D:"NOP",0x9F:"TXA"}

def dis(mem, pc):
    op = mem[pc]
    hi, lo = op >> 4, op & 0xF
    def b(i): return mem[pc + i]
    def rel(i):
        d = b(i)
        return (pc + i + 1 + (d - 256 if d > 127 else d)) & 0xFFFF
    if hi == 0x0:
        n, w = lo >> 1, "BRSET" if lo % 2 == 0 else "BRCLR"
        return (3, f"{w}{n} ${b(1):02X},${rel(2):04X}")
    if hi == 0x1:
        n, w = lo >> 1, "BSET" if lo % 2 == 0 else "BCLR"
        return (2, f"{w}{n} ${b(1):02X}")
    if hi == 0x2:
        return (2, f"{BRANCHES[lo]} ${rel(1):04X}")
    if hi == 0x3 and lo in RMW:
        return (2, f"{RMW[lo]} ${b(1):02X}")
    if hi == 0x4:
        if op == 0x42: return (1, "MUL")
        if lo in RMW: return (1, f"{RMW[lo]}A")
    if hi == 0x5 and lo in RMW:
        return (1, f"{RMW[lo]}X")
    if hi == 0x6 and lo in RMW:
        return (2, f"{RMW[lo]} ${b(1):02X},X")
    if hi == 0x7 and lo in RMW:
        return (1, f"{RMW[lo]} ,X")
    if op in INH8: return (1, INH8[op])
    if op in INH9: return (1, INH9[op])
    if op == 0xAD: return (2, f"BSR ${rel(1):04X}")
    if hi == 0xA and lo in ALU and lo not in (0x7, 0xC, 0xD, 0xF):
        return (2, f"{ALU[lo]} #${b(1):02X}")
    if hi == 0xB and lo in ALU:
        return (2, f"{ALU[lo]} ${b(1):02X}")
    if hi == 0xC and lo in ALU:
        return (3, f"{ALU[lo]} ${b(1):02X}{b(2):02X}")
    if hi == 0xD and lo in ALU:
        return (3, f"{ALU[lo]} ${b(1):02X}{b(2):02X},X")
    if hi == 0xE and lo in ALU:
        return (2, f"{ALU[lo]} ${b(1):02X},X")
    if hi == 0xF and lo in ALU:
        return (1, f"{ALU[lo]} ,X")
    return (1, f"DC.B ${op:02X}")

def main():
    data = open(sys.argv[1], "rb").read()
    start = int(sys.argv[2], 16) if len(sys.argv) > 2 else 0
    end = int(sys.argv[3], 16) if len(sys.argv) > 3 else len(data)
    pc = start
    while pc < end:
        ln, txt = dis(data, pc)
        raw = " ".join(f"{data[pc+i]:02X}" for i in range(ln))
        print(f"{pc:04X}: {raw:<9} {txt}")
        pc += ln

if __name__ == "__main__":
    main()
