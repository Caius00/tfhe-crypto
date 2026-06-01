use std::array;
use std::ops::{BitAnd, BitOr};
use std::time::Instant;
use tfhe::prelude::{FheDecrypt, FheEncrypt, FheEq, IfThenElse, OverflowingAdd, OverflowingSub};
use tfhe::{set_server_key, ClientKey, FheBool, FheUint2, FheUint8, ServerKey};

pub struct CPU {
    pub a: FheUint8,
    pub b: FheUint8,
    pub carry: FheBool,
    pub pc: FheUint8,
    pub memory: [FheUint8; 1],
}

pub fn make_cpu(zero_enc: &FheUint8, false_enc: &FheBool) -> CPU {
    let a = |_: usize| -> FheUint8 { zero_enc.clone() };

    CPU {
        a: zero_enc.clone(),
        b: zero_enc.clone(),
        carry: false_enc.clone(),
        pc: zero_enc.clone(),
        memory: array::from_fn(a),
    }
}

impl CPU {
    pub fn execute_cycle(
        &mut self,
        opcode: &FheUint8,
        operand: &FheUint8,
        sk: &ServerKey,
        zero_u8: &FheUint8,
        one_u8: &FheUint8,

        f_enc: &FheBool,
        msb_enc: &FheUint8,
        ck: &ClientKey,
    ) {
        set_server_key(sk.clone());

        println!("start decode at {:?}", Instant::now());

        let is_lda_d = opcode.eq(0x01u8);
        let is_lda_i = opcode.eq(0x02u8);
        let is_ldr = opcode.eq(0x03u8);
        let is_swp = opcode.eq(0x04u8);
        let is_jmp = opcode.eq(0x05u8);
        let is_jmz = opcode.eq(0x06u8);
        let is_jmc = opcode.eq(0x07u8);
        let is_djnz = opcode.eq(0x08u8);
        let is_add = opcode.eq(0x09u8);
        let is_addc = opcode.eq(0x0Au8);
        let is_add_i = opcode.eq(0x0Bu8);
        let is_addc_i = opcode.eq(0x0Cu8);
        let is_sub = opcode.eq(0x0Du8);
        let is_subc = opcode.eq(0x0Eu8);
        let is_sub_i = opcode.eq(0x0Fu8);
        let is_subc_i = opcode.eq(0x10u8);
        let is_mul = opcode.eq(0x11u8);
        let is_mul_i = opcode.eq(0x12u8);
        let is_div = opcode.eq(0x13u8);
        let is_div_i = opcode.eq(0x14u8);
        let is_sla = opcode.eq(0x15u8);
        let is_sra = opcode.eq(0x16u8);
        let is_rla = opcode.eq(0x17u8);
        let is_rlc = opcode.eq(0x18u8);
        let is_rra = opcode.eq(0x19u8);
        let is_rrc = opcode.eq(0x1Au8);
        let is_cca = opcode.eq(0x1Bu8);
        let is_and = opcode.eq(0x1Cu8);
        let is_or = opcode.eq(0x1Du8);
        let is_xor = opcode.eq(0x1Eu8);
        let is_dec = opcode.eq(0x1Fu8);
        let is_inc = opcode.eq(0x20u8);
        println!("done decode at {:?}", Instant::now());

        println!("start add at {:?}", Instant::now());
        let (res_add, carry_add) = (&self.a).overflowing_add(&self.b);
        let (res_addc, carry_addc) =
            (&self.a).overflowing_add(&(&self.b + self.carry.cmux(one_u8, zero_u8)));
        let (res_add_i, carry_add_i) = (&self.a).overflowing_add(operand);
        let (res_addc_i, carry_addc_i) =
            (&self.a).overflowing_add(&(operand + self.carry.cmux(one_u8, zero_u8)));
        println!("done add at {:?}", Instant::now());

        println!("start sub at {:?}", Instant::now());
        let (res_sub, borrow_sub) = (&self.a).overflowing_sub(&self.b);
        let (res_subc, borrow_subc) =
            (&self.a).overflowing_sub(&(&self.b + self.carry.cmux(one_u8, zero_u8)));
        let (res_sub_i, borrow_sub_i) = (&self.a).overflowing_sub(operand);
        let (res_subc_i, borrow_subc_i) =
            (&self.a).overflowing_sub(&(operand + self.carry.cmux(one_u8, zero_u8)));
        println!("done sub at {:?}", Instant::now());

        println!("start mul at {:?}", Instant::now());
        let res_mul = &self.a * &self.b;
        let res_mul_i = &self.a * operand;
        println!("done mul at {:?}", Instant::now());

        println!("start div at {:?}", Instant::now());

        let res_div = &self.a / &self.b;
        let res_div_i = &self.a / operand;
        println!("done div at {:?}", Instant::now());

        println!("start logic and bit at {:?}", Instant::now());
        let (res_dec, borrow_dec) = (&self.a).overflowing_sub(1u8);
        let (res_inc, carry_inc) = (&self.a).overflowing_add(1u8);

        let res_cca = !&self.a;
        let res_and = &self.a & &self.b;
        let res_or = &self.a | &self.b;
        let res_xor = &self.a ^ &self.b;

        let bit_7 = (&self.a & msb_enc).ne(0u8);
        let bit_0 = (&self.a & one_u8).ne(0u8);

        let res_sla = &self.a << 1u8;
        let res_sra = &self.a >> 1u8;
        let res_rla = (&self.a << 1u8) + self.carry.cmux(one_u8, zero_u8);
        let res_rlc = (&self.a << 1u8) + bit_7.cmux(one_u8, zero_u8);
        let res_rra = (&self.a >> 1u8) + self.carry.cmux(one_u8, zero_u8) * 128;
        let res_rrc = (&self.a >> 1u8) + bit_0.cmux(one_u8, zero_u8) * 128;
        println!("done logic and bit at {:?}", Instant::now());

        println!("start read ram at {:?}", Instant::now());
        let mut loaded_mem_val: FheUint8 = zero_u8.clone();
        for (idx, cell) in self.memory.iter().enumerate() {
            let matches_idx = operand.eq(idx as u8);
            loaded_mem_val = matches_idx.cmux(cell, &loaded_mem_val);
        }
        println!("done read ram at {:?}", Instant::now());

        println!("start next adr at {:?}", Instant::now());
        let a_is_zero = self.a.eq(0u8);

        let take_jmp = is_jmp.clone();
        let take_jmz = (&is_jmz).bitand(&a_is_zero);
        let take_jmc = (&is_jmc).bitand(&self.carry);
        let take_djnz = (&is_djnz).bitand(&res_dec.ne(0u8));

        let trigger_branch = take_jmp.bitor(&take_jmz).bitor(&take_jmc).bitor(&take_djnz);
        let branch_target = operand - 1u8;
        println!("done next adr at {:?}", Instant::now());

        println!("start mux a at {:?}", Instant::now());
        let mut next_a = self.a.clone();
        next_a = is_lda_d.cmux(&loaded_mem_val, &next_a);
        next_a = is_lda_i.cmux(operand, &next_a);
        next_a = is_swp.cmux(&self.b, &next_a);
        next_a = is_djnz.cmux(&res_dec, &next_a);
        next_a = is_add.cmux(&res_add, &next_a);
        next_a = is_addc.cmux(&res_addc, &next_a);
        next_a = is_add_i.cmux(&res_add_i, &next_a);
        next_a = is_addc_i.cmux(&res_addc_i, &next_a);
        next_a = is_sub.cmux(&res_sub, &next_a);
        next_a = is_subc.cmux(&res_subc, &next_a);
        next_a = is_sub_i.cmux(&res_sub_i, &next_a);
        next_a = is_subc_i.cmux(&res_subc_i, &next_a);
        next_a = is_mul.cmux(&res_mul, &next_a);
        next_a = is_mul_i.cmux(&res_mul_i, &next_a);
        next_a = is_div.cmux(&res_div, &next_a);
        next_a = is_div_i.cmux(&res_div_i, &next_a);
        next_a = is_sla.cmux(&res_sla, &next_a);
        next_a = is_sra.cmux(&res_sra, &next_a);
        next_a = is_rla.cmux(&res_rla, &next_a);
        next_a = is_rlc.cmux(&res_rlc, &next_a);
        next_a = is_rra.cmux(&res_rra, &next_a);
        next_a = is_rrc.cmux(&res_rrc, &next_a);
        next_a = is_cca.cmux(&res_cca, &next_a);
        next_a = is_and.cmux(&res_and, &next_a);
        next_a = is_or.cmux(&res_or, &next_a);
        next_a = is_xor.cmux(&res_xor, &next_a);
        next_a = is_dec.cmux(&res_dec, &next_a);
        next_a = is_inc.cmux(&res_inc, &next_a);
        println!("done mux a at {:?}", Instant::now());

        let mut next_b = self.b.clone();
        next_b = is_swp.cmux(&self.a, &next_b);

        let mut next_sp = &self.pc + 1u8;
        next_sp = trigger_branch.cmux(&branch_target, &next_sp);

        let mut next_carry = self.carry.clone();

        next_carry = is_add.cmux(&carry_add, &next_carry);
        next_carry = is_addc.cmux(&carry_addc, &next_carry);
        next_carry = is_add_i.cmux(&carry_add_i, &next_carry);
        next_carry = is_addc_i.cmux(&carry_addc_i, &next_carry);

        next_carry = is_sub.cmux(&borrow_sub, &next_carry);
        next_carry = is_subc.cmux(&borrow_subc, &next_carry);
        next_carry = is_sub_i.cmux(&borrow_sub_i, &next_carry);
        next_carry = is_subc_i.cmux(&borrow_subc_i, &next_carry);

        next_carry = is_dec.cmux(&borrow_dec, &next_carry);
        next_carry = is_inc.cmux(&carry_inc, &next_carry);

        let clears_carry = is_mul | is_mul_i | is_div | is_div_i;
        next_carry = clears_carry.cmux(&f_enc, &next_carry);

        next_carry = is_sla.cmux(&bit_7, &next_carry);
        next_carry = is_sra.cmux(&bit_0, &next_carry);
        next_carry = is_rla.cmux(&bit_7, &next_carry);
        next_carry = is_rlc.cmux(&bit_7, &next_carry);
        next_carry = is_rra.cmux(&bit_0, &next_carry);
        next_carry = is_rrc.cmux(&bit_0, &next_carry);
        println!("carry after shift: {}", next_carry.decrypt(ck));

        for (idx, cell) in self.memory.iter_mut().enumerate() {
            let matches_target = operand.eq(idx as u8);

            let write_enable = (&is_ldr).bitand(&matches_target);
            *cell = write_enable.cmux(&self.a, cell);
        }

        self.a = next_a;
        self.b = next_b;
        self.pc = next_sp;
        self.carry = next_carry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfhe::prelude::FheDecrypt;
    use tfhe::ConfigBuilder;

    #[test]
    fn add() {
        let config = ConfigBuilder::default().build();
        let (ck, sk) = tfhe::generate_keys(config);
        println!("have keys");

        let zero_enc = FheUint8::encrypt(0u8, &ck);
        let zero_enc1 = FheUint8::encrypt(0u8, &ck);
        let one_enc = FheUint8::encrypt(1u8, &ck);

        let msb_enc = FheUint8::encrypt(0x80u8, &ck);

        let false_enc = FheBool::encrypt(false, &ck);

        let add_opcode = FheUint8::encrypt(0b0000_1001u8, &ck);
        let operand1 = FheUint8::encrypt(13u8, &ck);
        let operand2 = FheUint8::encrypt(18u8, &ck);

        let mut cpu = make_cpu(&zero_enc, &false_enc);

        let a_dec: u8 = cpu.a.decrypt(&ck);
        let pc_dec: u8 = cpu.pc.decrypt(&ck);
        let b_dec: u8 = cpu.b.decrypt(&ck);
        let c_dec: bool = cpu.carry.decrypt(&ck);

        assert_eq!(a_dec, 0, "init 0");
        assert_eq!(pc_dec, 0, "init 0");
        assert_eq!(b_dec, 0, "init 0");
        assert_eq!(c_dec, false, "init false");

        cpu.a = operand1;
        cpu.b = operand2;

        println!("start exec");

        cpu.execute_cycle(
            &add_opcode,
            &zero_enc,
            &sk,
            &zero_enc1,
            &one_enc,
            &false_enc,
            &msb_enc,
            &ck,
        );

        let a_dec: u8 = cpu.a.decrypt(&ck);
        let pc_dec: u8 = cpu.pc.decrypt(&ck);
        let b_dec: u8 = cpu.b.decrypt(&ck);
        let c_dec: bool = cpu.carry.decrypt(&ck);

        assert_eq!(a_dec, 31, "13 + 18 = 31");
        assert_eq!(pc_dec, 1, "pc is 1 after execute");
        assert_eq!(b_dec, 18, "unchanged");
        assert_eq!(c_dec, false, "no carry");

        let opcode_add_immediate = FheUint8::encrypt(0b0000_1011u8, &ck);
        let operand3 = FheUint8::encrypt(225u8, &ck);
        cpu.execute_cycle(
            &opcode_add_immediate,
            &operand3,
            &sk,
            &zero_enc,
            &one_enc,
            &false_enc,
            &msb_enc,
            &ck,
        );

        let a_dec: u8 = cpu.a.decrypt(&ck);
        let pc_dec: u8 = cpu.pc.decrypt(&ck);
        let b_dec: u8 = cpu.b.decrypt(&ck);
        let c_dec: bool = cpu.carry.decrypt(&ck);

        assert_eq!(a_dec, 0, "wrapped to 0");
        assert_eq!(pc_dec, 2, "second execute");
        assert_eq!(b_dec, 18, "unchanged");
        assert_eq!(c_dec, true, "overflow");
    }

    #[test]
    fn reality_check() {
        let config = ConfigBuilder::default().build();
        let (ck, sk) = tfhe::generate_keys(config);
        set_server_key(sk);
        let add_opcode = FheUint8::encrypt(0b0000_1001u8, &ck);
        let is_add = add_opcode.eq(0x09u8);
        let dec = is_add.decrypt(&ck);
        assert!(dec);
    }
}
