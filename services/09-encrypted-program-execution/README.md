# ⚙️ 09 · Encrypted Program Execution

![version](https://img.shields.io/badge/version-0.1.22-blue)

## instuction set

every instruction is 16 bits: 8 bit opcode and 8 bit operand
after every instruction: SP := SP + 2;

| Opcode    | Mnemonic         | Operand    | Carry  | Description                                                                    |
|-----------|------------------|------------|--------|--------------------------------------------------------------------------------|
| 0000_0000 | NOP              | 0000_0000  | -      | No operation                                                                   |
| 0000_0001 | LDA (direct)     | source_ADR | -      | $A \leftarrow M[\text{source_ADR}]$                                            |
| 0000_0010 | LDA (immediate)  | data       | -      | $A \leftarrow \text{data}$                                                     |
| 0000_0011 | LDR              | target_ADR | -      | $M[\text{target_ADR}] \leftarrow A$                                            |
| 0000_0100 | SWP              | 0000_0000  | -      | $A \leftarrow B, B \leftarrow A$                                               |
| 0000_0101 | JMP (immediate)  | ADR        | -      | $\text{SP} \leftarrow \text{ADR} - 1$                                          |
| 0000_0110 | JMZ (immediate)  | ADR        | -      | if $A = 0$ then $\text{SP} \leftarrow \text{ADR} - 1$                          |
| 0000_0111 | JMC (immediate)  | ADR        | -      | if $\text{carry}$ then $\text{SP} \leftarrow \text{ADR} - 1$                   |
| 0000_1000 | DJNZ             | ADR        | -      | $A \leftarrow A - 1$; if $A \neq 0$ then $\text{SP} \leftarrow \text{ADR} - 1$ |
| 0000_1001 | ADD              | 0000_0000  | *      | $A \leftarrow A + B$                                                           |
| 0000_1010 | ADDC             | 0000_0000  | *      | $A \leftarrow A + B + \text{carry}$                                            |
| 0000_1011 | ADD (immediate)  | data       | *      | $A \leftarrow A + \text{data}$                                                 |
| 0000_1100 | ADDC (immediate) | data       | *      | $A \leftarrow A + \text{data} + \text{carry}$                                  |
| 0000_1101 | SUB              | 0000_0000  | *      | $A \leftarrow A - B$                                                           |
| 0000_1110 | SUBC             | 0000_0000  | *      | $A \leftarrow A - B - \text{carry}$                                            |
| 0000_1111 | SUB (immediate)  | data       | *      | $A \leftarrow A - \text{data}$                                                 |
| 0001_0000 | SUBC (immediate) | data       | *      | $A \leftarrow A - \text{data} - \text{carry}$                                  |
| 0001_0001 | MUL              | 0000_0000  | 0      | $A \mathbin{\Vert} B \leftarrow A \times B$                                    |
| 0001_0010 | MUL (immediate)  | data       | 0      | $A \mathbin{\Vert} B \leftarrow A \times \text{data}$                          |
| 0001_0011 | DIV              | 0000_0000  | 0      | $A \leftarrow \lfloor A / B \rfloor$                                           |
| 0001_0100 | DIV (immediate)  | data       | 0      | $A \leftarrow \lfloor A / \text{data} \rfloor$                                 |
| 0001_0101 | SLA              | 0000_0000  | $A[7]$ | $A \leftarrow A \ll 1$                                                         |
| 0001_0110 | SRA              | 0000_0000  | $A[0]$ | $A \leftarrow A \gg 1$                                                         |
| 0001_0111 | RLA              | 0000_0000  | $A[7]$ | $A \leftarrow (A \ll 1) \lor \text{carry}$                                     |
| 0001_1000 | RLC              | 0000_0000  | $A[7]$ | $A \leftarrow (A \ll 1) \lor A[7]$                                             |
| 0001_1001 | RRA              | 0000_0000  | $A[0]$ | $A \leftarrow (A \gg 1) \lor (\text{carry} \ll 7)$                             |
| 0001_1010 | RRC              | 0000_0000  | $A[0]$ | $A \leftarrow (A \gg 1) \lor (A[0] \ll 7)$                                     |
| 0001_1011 | CCA              | 0000_0000  | -      | $A \leftarrow \overline{A}$                                                    |
| 0001_1100 | AND              | 0000_0000  | -      | $A \leftarrow A \land B$                                                       |
| 0001_1101 | OR               | 0000_0000  | -      | $A \leftarrow A \lor B$                                                        |
| 0001_1110 | XOR              | 0000_0000  | -      | $A \leftarrow A \oplus B$                                                      |
| 0001_1111 | DEC              | 0000_0000  | *      | $A \leftarrow A - 1$                                                           |
| 0010_0000 | INC              | 0000_0000  | *      | $A \leftarrow A + 1$                                                           |