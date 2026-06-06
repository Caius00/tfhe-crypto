use std::array;
use std::ops::{BitAnd, BitOr};
use tfhe::prelude::{
    FheEq, FheTrivialEncrypt, IfThenElse, OverflowingAdd, OverflowingSub,
};
use tfhe::{set_server_key, FheBool, FheUint8, ServerKey};

pub struct CPU {
    pub a: FheUint8,
    pub b: FheUint8,
    pub carry: FheBool,
    pub pc: FheUint8,
    pub memory: [FheUint8; 0],
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
    // specify needed additional instructions?
    pub fn execute_cycle(&mut self, opcode: &FheUint8, operand: &FheUint8, sk: &ServerKey) {
        set_server_key(sk.clone());

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

        let one_u8 = &FheUint8::encrypt_trivial(1u8);
        let zero_u8 = &FheUint8::encrypt_trivial(0u8);
        let msb_enc = &FheUint8::encrypt_trivial(0x80u8);

        let mut res_add_tuple = None;
        let mut res_addc_tuple = None;
        let mut res_add_i_tuple = None;
        let mut res_addc_i_tuple = None;
        let mut res_sub_tuple = None;
        let mut res_subc_tuple = None;
        let mut res_sub_i_tuple = None;
        let mut res_subc_i_tuple = None;
        let mut res_mul = None;
        let mut res_mul_i = None;
        let mut res_div = None;
        let mut res_div_i = None;
        let mut res_dec_tuple = None;
        let mut res_inc_tuple = None;
        let mut res_cca = None;
        let mut res_and = None;
        let mut res_or = None;
        let mut res_xor = None;
        let mut res_sla = None;
        let mut res_sra = None;
        let mut res_rla = None;
        let mut res_rlc = None;
        let mut res_rra = None;
        let mut res_rrc = None;

        rayon::ThreadPoolBuilder::new()
            .num_threads(rayon::current_num_threads())
            .build_scoped(
                |thread| {
                    set_server_key(sk.clone());
                    thread.run();
                },
                |pool| {
                    pool.install(|| {
                        rayon::scope(|s| {
                            s.spawn(|_| res_add_tuple = Some((&self.a).overflowing_add(&self.b)));
                            s.spawn(|_| {
                                res_addc_tuple =
                                    Some((&self.a).overflowing_add(
                                        &(&self.b + self.carry.cmux(one_u8, zero_u8)),
                                    ))
                            });
                            s.spawn(|_| res_add_i_tuple = Some((&self.a).overflowing_add(operand)));
                            s.spawn(|_| {
                                res_addc_i_tuple =
                                    Some((&self.a).overflowing_add(
                                        &(operand + self.carry.cmux(one_u8, zero_u8)),
                                    ))
                            });

                            s.spawn(|_| res_sub_tuple = Some((&self.a).overflowing_sub(&self.b)));
                            s.spawn(|_| {
                                res_subc_tuple =
                                    Some((&self.a).overflowing_sub(
                                        &(&self.b + self.carry.cmux(one_u8, zero_u8)),
                                    ))
                            });
                            s.spawn(|_| res_sub_i_tuple = Some((&self.a).overflowing_sub(operand)));
                            s.spawn(|_| {
                                res_subc_i_tuple =
                                    Some((&self.a).overflowing_sub(
                                        &(operand + self.carry.cmux(one_u8, zero_u8)),
                                    ))
                            });

                            s.spawn(|_| res_mul = Some(&self.a * &self.b));
                            s.spawn(|_| res_mul_i = Some(&self.a * operand));
                            s.spawn(|_| res_div = Some(&self.a / &self.b));
                            s.spawn(|_| res_div_i = Some(&self.a / operand));

                            s.spawn(|_| res_dec_tuple = Some((&self.a).overflowing_sub(1u8)));
                            s.spawn(|_| res_inc_tuple = Some((&self.a).overflowing_add(1u8)));
                            s.spawn(|_| res_cca = Some(!&self.a));
                            s.spawn(|_| res_and = Some(&self.a & &self.b));
                            s.spawn(|_| res_or = Some(&self.a | &self.b));
                            s.spawn(|_| res_xor = Some(&self.a ^ &self.b));

                            s.spawn(|_| res_sla = Some(&self.a << 1u8));
                            s.spawn(|_| res_sra = Some(&self.a >> 1u8));

                            s.spawn(|_| {
                                res_rla = Some((&self.a << 1u8) + self.carry.cmux(one_u8, zero_u8))
                            });
                            s.spawn(|_| {
                                res_rra =
                                    Some((&self.a >> 1u8) + self.carry.cmux(one_u8, zero_u8) * 128)
                            });

                            s.spawn(|_| {
                                let bit_7 = (&self.a & msb_enc).ne(0u8);
                                res_rlc = Some((&self.a << 1u8) + bit_7.cmux(one_u8, zero_u8));
                            });

                            s.spawn(|_| {
                                let bit_0 = (&self.a & one_u8).ne(0u8);
                                res_rrc =
                                    Some((&self.a >> 1u8) + bit_0.cmux(one_u8, zero_u8) * 128);
                            });
                        });
                    });
                },
            )
            .expect("");

        let bit_7 = (&self.a & msb_enc).ne(0u8);
        let bit_0 = (&self.a & one_u8).ne(0u8);

        let (res_add, carry_add) = res_add_tuple.unwrap();
        let (res_addc, carry_addc) = res_addc_tuple.unwrap();
        let (res_add_i, carry_add_i) = res_add_i_tuple.unwrap();
        let (res_addc_i, carry_addc_i) = res_addc_i_tuple.unwrap();

        let (res_sub, borrow_sub) = res_sub_tuple.unwrap();
        let (res_subc, borrow_subc) = res_subc_tuple.unwrap();
        let (res_sub_i, borrow_sub_i) = res_sub_i_tuple.unwrap();
        let (res_subc_i, borrow_subc_i) = res_subc_i_tuple.unwrap();

        let res_mul = res_mul.unwrap();
        let res_mul_i = res_mul_i.unwrap();
        let res_div = res_div.unwrap();
        let res_div_i = res_div_i.unwrap();

        let (res_dec, borrow_dec): (FheUint8, FheBool) = res_dec_tuple.unwrap();
        let (res_inc, carry_inc) = res_inc_tuple.unwrap();

        let res_cca = res_cca.unwrap();
        let res_and = res_and.unwrap();
        let res_or = res_or.unwrap();
        let res_xor = res_xor.unwrap();

        let res_sla = res_sla.unwrap();
        let res_sra = res_sra.unwrap();
        let res_rla = res_rla.unwrap();
        let res_rlc = res_rlc.unwrap();
        let res_rra = res_rra.unwrap();
        let res_rrc = res_rrc.unwrap();

        let mut loaded_mem_val: FheUint8 = zero_u8.clone();
        for (idx, cell) in self.memory.iter().enumerate() {
            let matches_idx = operand.eq(idx as u8);
            loaded_mem_val = matches_idx.cmux(cell, &loaded_mem_val);
        }

        let a_is_zero = self.a.eq(0u8);

        let take_jmp = is_jmp.clone();
        let take_jmz = (&is_jmz).bitand(&a_is_zero);
        let take_jmc = (&is_jmc).bitand(&self.carry);
        let take_djnz = (&is_djnz).bitand(&res_dec.ne(0u8));

        let trigger_branch = take_jmp.bitor(&take_jmz).bitor(&take_jmc).bitor(&take_djnz);
        let branch_target = operand - 1u8;

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
        let f_enc = FheBool::encrypt_trivial(false);
        next_carry = clears_carry.cmux(&f_enc, &next_carry);

        next_carry = is_sla.cmux(&bit_7, &next_carry);
        next_carry = is_sra.cmux(&bit_0, &next_carry);
        next_carry = is_rla.cmux(&bit_7, &next_carry);
        next_carry = is_rlc.cmux(&bit_7, &next_carry);
        next_carry = is_rra.cmux(&bit_0, &next_carry);
        next_carry = is_rrc.cmux(&bit_0, &next_carry);

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
    use tfhe::prelude::{FheDecrypt, FheTrivialEncrypt};
    use tfhe::{set_server_key, ConfigBuilder};

    #[test]
    fn add() {
        let config = ConfigBuilder::default().build();
        let (ck, sk) = tfhe::generate_keys(config);
        set_server_key(sk.clone());

        let zero_enc = FheUint8::encrypt_trivial(0u8);

        let false_enc = FheBool::encrypt_trivial(false);

        let add_opcode = FheUint8::encrypt_trivial(0b0000_1001u8);
        let operand1 = FheUint8::encrypt_trivial(13u8);
        let operand2 = FheUint8::encrypt_trivial(18u8);

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

        cpu.execute_cycle(&add_opcode, &zero_enc, &sk);

        let a_dec: u8 = cpu.a.decrypt(&ck);
        let pc_dec: u8 = cpu.pc.decrypt(&ck);
        let b_dec: u8 = cpu.b.decrypt(&ck);
        let c_dec: bool = cpu.carry.decrypt(&ck);

        assert_eq!(a_dec, 31, "13 + 18 = 31");
        assert_eq!(pc_dec, 1, "pc is 1 after execute");
        assert_eq!(b_dec, 18, "unchanged");
        assert_eq!(c_dec, false, "no carry");

        let opcode_add_immediate = FheUint8::encrypt_trivial(0b0000_1011u8);
        let operand3 = FheUint8::encrypt_trivial(225u8);
        cpu.execute_cycle(&opcode_add_immediate, &operand3, &sk);

        let a_dec: u8 = cpu.a.decrypt(&ck);
        let pc_dec: u8 = cpu.pc.decrypt(&ck);
        let b_dec: u8 = cpu.b.decrypt(&ck);
        let c_dec: bool = cpu.carry.decrypt(&ck);

        assert_eq!(a_dec, 0, "wrapped to 0");
        assert_eq!(pc_dec, 2, "second execute");
        assert_eq!(b_dec, 18, "unchanged");
        assert_eq!(c_dec, true, "overflow");
    }
}
