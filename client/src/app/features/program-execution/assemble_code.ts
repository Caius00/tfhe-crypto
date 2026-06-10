export function assemble_code(programme: string): number[] {
    const lines = programme.split("\n").map((line) => {
        const comment_index = line.indexOf(";");

        if (comment_index >= 0) {
            return line.substring(0, comment_index).trim();
        }


        return line.trim();
    });

    const bytes: number[] = [];

    for (const l of lines) {
        let [op_s, or_s] = l.split(" ");
        if (!or_s) {
            or_s = "00";
        }

        if (op_s.startsWith("#")) {
            bytes.push(Number.parseInt(op_s.slice(1), 16), Number.parseInt(or_s.slice(1), 16))
        } else {
            const [op, or] = mnemonic_to_opcode(op_s, or_s);
            bytes.push(op, or);
        }
    }

    return bytes;
}

const mnemonic_to_opcode = (mnemonic: string, operand: string): [number, number] => {
    let op: number = -1;
    let or: number;
    let oprx = operand.slice(1);

    let radix = 10;
    if (oprx.toLowerCase().startsWith("0b")) {
        radix = 2;
        oprx = oprx.slice(2);
    } else if (oprx.toLowerCase().startsWith("0x")) {
        radix = 16;
        oprx = oprx.slice(2);
    }

    or = Number.parseInt(oprx, radix);

    switch (mnemonic.toUpperCase()) {
        case "NOP":
            op = 0;
            break;
        case "LDA":
            if (operand.startsWith("$")) {
                op = 1;
            } else if (operand.startsWith("#")) {
                op = 2;
            }

            break;
        case "LDR":
            op = 3;
            break;
        case "SWP":
            op = 4;
            break;
        case "JMP":
            op = 5;
            break;
        case "JMZ":
            op = 6;
            break;
        case "JMC":
            op = 7
            break;
        case "DJNZ":
            op = 8;
            break;
        case "ADD":
            if (operand.startsWith("#")) {
                op = 11;
            } else {
                op = 9;
            }
            break;
        case "ADDC":
            if (operand.startsWith("#")) {
                op = 12;
            } else {
                op = 10;
            }
            break;
        case "SUB":
            if (operand.startsWith("#")) {
                op = 15;
            } else {
                op = 13;
            }
            break;
        case "SUBC":
            if (operand.startsWith("#")) {
                op = 16;
            } else {
                op = 14;
            }
            break;
        case "MUL":
            if (operand.startsWith("#")) {
                op = 18;
            } else {
                op = 17;
            }
            break;
        case "DIV":
            if (operand.startsWith("#")) {
                op = 20;
            } else {
                op = 19;
            }
            break;
        case "SLA":
            op = 21;
            break;
        case "SRA":
            op = 22;
            break;
        case "RLA":
            op = 23;
            break;
        case "RLC":
            op = 24;
            break;
        case "RRA":
            op = 25;
            break;
        case "RRC":
            op = 26;
            break;
        case "CCA":
            op = 27;
            break;
        case "AND":
            op = 28;
            break;
        case "OR":
            op = 29;
            break;
        case "XOR":
            op = 30;
            break;
        case "DEC":
            op = 31;
            break;
        case "INC":
            op = 32;
            break;
    }

    return [op, or % 256];
}

/*
LDA $95
SWP
LDA $87
ADD
LDR $103
LDA $94
SWP
LDA $86
ADDC
LDR $102
LDA $93
SWP
LDA $85
ADDC
LDR $101
LDA $92
SWP
LDA $84
ADDC
LDR $100
LDA $91
SWP
LDA $83
ADDC
LDR $99
LDA $90
SWP
LDA $82
ADDC
LDR $98
LDA $89
SWP
LDA $81
ADDC
LDR $97
LDA $88
SWP
LDA $80
ADDC
LDR $96
*/

/*
LDA $95
SWP
LDA $87
ADD
LDR $103
LDA $94
SWP
LDA $86
ADDC
LDR $102
LDA $93
SWP
LDA $85
ADDC
LDR $101
LDA $92
SWP
LDA $84
ADDC
LDR $100
LDA $91
SWP
LDA $83
ADDC
LDR $99
LDA $90
SWP
LDA $82
ADDC
LDR $98
LDA $89
SWP
LDA $81
ADDC
LDR $97
LDA $88
SWP
LDA $80
ADDC
LDR $96
#DE #AD; A
#BE #EF
#CA #FE
#DE #AF
#AF #FE; B
#DA #DA
#BA #DD
#DA #ED
NOP
NOP
NOP
NOP
 */