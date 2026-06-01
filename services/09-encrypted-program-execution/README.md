# ⚙️ 09 · Encrypted Program Execution

![version](https://img.shields.io/badge/version-0.1.21-blue)

## instuction set

every instruction is 16 bits: 8 bit opcode and 8 bit operand
after every instruction: SP := SP + 1;

| Opcode    | Mnemonic         | Operand    | Carry | Description                       |
|-----------|------------------|------------|-------|-----------------------------------|
| 0000_0000 | NOP              | 0000_0000  | —     |                                   |
| 0000_0001 | LDA (direct)     | source_ADR | —     | A := &ADR                         |
| 0000_0010 | LDA (immediate)  | data       | —     | A := data                         |
| 0000_0011 | LDR              | target_ADR | —     | &ADR := A                         |
| 0000_0100 | SWP              | 0000_0000  | —     | A := B, B := A                    |
| 0000_0101 | JMP (immediate)  | ADR        | —     | sp := ADR - 1                     |
| 0000_0110 | JMZ (immediate)  | ADR        | —     | if A = 0 then sp := ADR - 1       |
| 0000_0111 | JMC (immediate)  | ADR        | —     | if carry then sp := ADR - 1       |
| 0000_1000 | DJNZ             | ADR        | —     | A--; if A != 0 then SP := ADR - 1 |
| 0000_1001 | ADD              | 0000_0000  | *     | A := A + B                        |
| 0000_1010 | ADDC             | 0000_0000  | *     | A := A + B + carry                |
| 0000_1011 | ADD (immediate)  | data       | *     | A := A + data                     |
| 0000_1100 | ADDC (immediate) | data       | *     | A := A + data + carry             |
| 0000_1101 | SUB              | 0000_0000  | *     | A := A - B                        |
| 0000_1110 | SUBC             | 0000_0000  | *     | A := A - B - carry                |
| 0000_1111 | SUB (immediate)  | data       | *     | A := A - data                     |
| 0001_0000 | SUBC (immediate) | data       | *     | A := A - data - carry             |
| 0001_0001 | MUL              | 0000_0000  | 0     | AB := A * B                       |
| 0001_0010 | MUL (immediate)  | data       | 0     | AB := A * data                    |
| 0001_0011 | DIV              | 0000_0000  | 0     | A := A / data (integer div)       |
| 0001_0100 | DIV (immediate)  | data       | 0     | A := A / data (integer div)       |
| 0001_0101 | SLA              | 0000_0000  | A[7]  | A << 1                            |
| 0001_0110 | SRA              | 0000_0000  | A[0]  | A >> 1                            |
| 0001_0111 | RLA              | 0000_0000  | A[7]  | A << 1; A[0] := carry             |
| 0001_1000 | RLC              | 0000_0000  | A[7]  | A << 1; A[0] := A[7]              |
| 0001_1001 | RRA              | 0000_0000  | A[0]  | A >> 1; A[7] carry                |
| 0001_1010 | RRC              | 0000_0000  | A[0]  | A >> 1; A[7] := A[0]              |
| 0001_1011 | CCA              | 0000_0000  | -     | A := ~A                           |
| 0001_1100 | AND              | 0000_0000  | -     | A := A & B (bitwise)              |
| 0001_1101 | OR               | 0000_0000  | -     | A := A \| B                       |
| 0001_1110 | XOR              | 0000_0000  | -     | A := A ^ B                        |
| 0001_1111 | DEC              | 0000_0000  | *     | A := A - 1                        |
| 0010_0000 | INC              | 0000_0000  | *     | A := A + 1                        |
