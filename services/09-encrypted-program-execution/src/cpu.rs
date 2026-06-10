use rayon::iter::{IndexedParallelIterator, ParallelIterator};
use rayon::prelude::ParallelSlice;
use std::ops::{BitAnd, BitOr};
use tfhe::prelude::{
    CastFrom, CastInto, FheEq, FheTrivialEncrypt, IfThenElse, OverflowingAdd, OverflowingSub,
};
use tfhe::{set_server_key, FheBool, FheUint16, FheUint8, ServerKey};

#[allow(clippy::upper_case_acronyms)]
pub struct CPU {
    pub a: FheUint8,
    pub b: FheUint8,
    pub carry: FheBool,
    pub pc: FheUint8,
    pub memory: Vec<FheUint8>,
}

pub fn make_cpu(size: usize) -> CPU {
    let zero_enc = FheUint8::encrypt_trivial(0u8);
    let false_enc = FheBool::encrypt_trivial(false);

    CPU {
        a: zero_enc.clone(),
        b: zero_enc.clone(),
        carry: false_enc.clone(),
        pc: zero_enc.clone(),
        memory: vec![zero_enc; size],
    }
}

impl CPU {
    // specify needed additional instructions?

    pub fn execute_program(&mut self, sk: &ServerKey) {
        let (op, or) = self.fetch(sk);
        self.execute_cycle(&op, &or, sk);
    }

    fn fetch(&self, sk: &ServerKey) -> (FheUint8, FheUint8) {
        let pc_plus_1 = (&self.pc + 1u8) % self.memory.len() as u8;

        let mut chunk_results = None;
        let chunk_size = 16;

        rayon::ThreadPoolBuilder::new()
            .num_threads(rayon::current_num_threads())
            .build_scoped(
                |thread| {
                    set_server_key(sk.clone());
                    thread.run();
                },
                |pool| {
                    pool.install(|| {
                        chunk_results = Some(
                            self.memory
                                .par_chunks(chunk_size)
                                .enumerate()
                                .map(|(chunk_idx, chunk)| {
                                    let mut local_opcode = FheUint8::encrypt_trivial(0u8);
                                    let mut local_operand = FheUint8::encrypt_trivial(0u8);

                                    let mut opcode_found = FheBool::encrypt_trivial(false);
                                    let mut operand_found = FheBool::encrypt_trivial(false);

                                    for (inner_idx, cell) in chunk.iter().enumerate() {
                                        let abs_idx = (chunk_idx * chunk_size + inner_idx) as u8;

                                        let matches_op = self.pc.eq(abs_idx);
                                        let matches_or = pc_plus_1.eq(abs_idx);

                                        local_opcode = matches_op.cmux(cell, &local_opcode);
                                        local_operand = matches_or.cmux(cell, &local_operand);

                                        opcode_found = matches_op | opcode_found;
                                        operand_found = matches_or | operand_found;
                                    }

                                    (local_opcode, local_operand, opcode_found, operand_found)
                                })
                                .collect::<Vec<_>>(),
                        );
                    });
                },
            )
            .expect("");

        let chunks = chunk_results.unwrap();

        let mut final_opcode = FheUint8::encrypt_trivial(0u8);
        let mut final_operand = FheUint8::encrypt_trivial(0u8);

        for (chunk_op, chunk_or, op_found, or_found) in chunks {
            final_opcode = op_found.cmux(&chunk_op, &final_opcode);
            final_operand = or_found.cmux(&chunk_or, &final_operand);
        }

        (final_opcode, final_operand)
    }

    fn execute_cycle(&mut self, opcode: &FheUint8, operand: &FheUint8, sk: &ServerKey) {
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
        let mut res_mul_tuple = None;
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
                                let (x, y) =
                                    (&self.b).overflowing_add(&self.carry.cmux(one_u8, zero_u8));
                                let (a, c) = (&self.a).overflowing_add(&x);
                                res_addc_tuple = Some((a, y | c));
                            });
                            s.spawn(|_| res_add_i_tuple = Some((&self.a).overflowing_add(operand)));
                            s.spawn(|_| {
                                let (x, y) =
                                    operand.overflowing_add(&self.carry.cmux(one_u8, zero_u8));
                                let (a, c) = (&self.a).overflowing_add(&x);
                                res_addc_i_tuple = Some((a, y | c));
                            });

                            s.spawn(|_| res_sub_tuple = Some((&self.a).overflowing_sub(&self.b)));
                            s.spawn(|_| {
                                let (x, y) =
                                    (&self.b).overflowing_sub(&self.carry.cmux(one_u8, zero_u8));
                                let (a, c) = (&self.a).overflowing_sub(&x);
                                res_subc_tuple = Some((a, y | c));
                            });
                            s.spawn(|_| res_sub_i_tuple = Some((&self.a).overflowing_sub(operand)));
                            s.spawn(|_| {
                                let (x, y) =
                                    operand.overflowing_sub(&self.carry.cmux(one_u8, zero_u8));
                                let (a, c) = (&self.a).overflowing_sub(&x);
                                res_subc_i_tuple = Some((a, y | c));
                            });

                            s.spawn(|_| {
                                let a_16: FheUint16 = FheUint16::cast_from(self.a.clone());
                                let b_16: FheUint16 = FheUint16::cast_from(self.b.clone());
                                let mul_16 = a_16 * b_16;

                                let upper_byte: FheUint8 = (&mul_16 >> 8u8).cast_into();
                                let lower_byte: FheUint8 = mul_16.cast_into();

                                res_mul_tuple = Some((upper_byte, lower_byte));
                            });
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

        let (res_mul_upper, res_mul_lower) = res_mul_tuple.unwrap();
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
        let branch_target = operand;

        let mut next_a_final = None;
        let next_a = self.a.clone();

        rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build_scoped(
                |thread| {
                    tfhe::set_server_key(sk.clone());
                    thread.run();
                },
                |pool| {
                    pool.install(|| {
                        let ((m1, m2), (m3, m4)) = rayon::join(
                            || {
                                rayon::join(
                                    || is_lda_d.cmux(&loaded_mem_val, &self.a),
                                    || is_lda_i.cmux(operand, (&is_swp.cmux(&self.b, &next_a))),
                                )
                            },
                            || {
                                rayon::join(
                                    || is_djnz.cmux(&res_dec, (&is_add.cmux(&res_add, &next_a))),
                                    || {
                                        is_addc
                                            .cmux(&res_addc, (&is_add_i.cmux(&res_add_i, &next_a)))
                                    },
                                )
                            },
                        );

                        let ((m5, m6), (m7, m8)) = rayon::join(
                            || {
                                rayon::join(
                                    || {
                                        is_addc_i
                                            .cmux(&res_addc_i, (&is_sub.cmux(&res_sub, &next_a)))
                                    },
                                    || {
                                        is_subc
                                            .cmux(&res_subc, (&is_sub_i.cmux(&res_sub_i, &next_a)))
                                    },
                                )
                            },
                            || {
                                rayon::join(
                                    || {
                                        is_subc_i.cmux(
                                            &res_subc_i,
                                            (&is_mul.cmux(&res_mul_upper, &next_a)),
                                        )
                                    },
                                    || is_mul_i.cmux(&res_mul_i, (&is_div.cmux(&res_div, &next_a))),
                                )
                            },
                        );

                        let ((m9, m10), (m11, m12)) = rayon::join(
                            || {
                                rayon::join(
                                    || is_div_i.cmux(&res_div_i, (&is_sla.cmux(&res_sla, &next_a))),
                                    || is_sra.cmux(&res_sra, (&is_rla.cmux(&res_rla, &next_a))),
                                )
                            },
                            || {
                                rayon::join(
                                    || is_rlc.cmux(&res_rlc, (&is_rra.cmux(&res_rra, &next_a))),
                                    || is_rrc.cmux(&res_rrc, (&is_cca.cmux(&res_cca, &next_a))),
                                )
                            },
                        );

                        let (m13, m14) = rayon::join(
                            || is_and.cmux(&res_and, &is_or.cmux(&res_or, &next_a)),
                            || {
                                is_xor.cmux(
                                    &res_xor,
                                    &is_dec.cmux(&res_dec, (&is_inc.cmux(&res_inc, &next_a))),
                                )
                            },
                        );

                        let ((r1, r2), (r3, r4)): ((FheUint8, FheUint8), (FheUint8, FheUint8)) =
                            rayon::join(
                                || {
                                    rayon::join(
                                        || (&is_lda_d | &is_lda_i | &is_swp).cmux(&m1, &m2),
                                        || {
                                            (&is_djnz | &is_add | &is_addc | &is_add_i)
                                                .cmux(&m3, &m4)
                                        },
                                    )
                                },
                                || {
                                    rayon::join(
                                        || {
                                            (&is_addc_i | &is_sub | &is_subc | &is_sub_i)
                                                .cmux(&m5, &m6)
                                        },
                                        || {
                                            (&is_subc_i | &is_mul | &is_mul_i | &is_div)
                                                .cmux(&m7, &m8)
                                        },
                                    )
                                },
                            );

                        let ((r5, r6), r7) = rayon::join(
                            || {
                                rayon::join(
                                    || (&is_div_i | &is_sla | &is_sra | &is_rla).cmux(&m9, &m10),
                                    || (&is_rlc | &is_rra | &is_rrc | &is_cca).cmux(&m11, &m12),
                                )
                            },
                            || (&is_and | &is_or).cmux(&m13, &m14),
                        );

                        let ((f1, f2), (f3, f4)) = rayon::join(
                            || {
                                rayon::join(
                                    || {
                                        (&is_lda_d
                                            | &is_lda_i
                                            | &is_swp
                                            | &is_djnz
                                            | &is_add
                                            | &is_addc
                                            | &is_add_i)
                                            .cmux(&r1, &r2)
                                    },
                                    || {
                                        (&is_addc_i
                                            | &is_sub
                                            | &is_subc
                                            | &is_sub_i
                                            | &is_subc_i
                                            | &is_mul
                                            | &is_mul_i
                                            | &is_div)
                                            .cmux(&r3, &r4)
                                    },
                                )
                            },
                            || {
                                rayon::join(
                                    || {
                                        (&is_div_i
                                            | &is_sla
                                            | &is_sra
                                            | &is_rla
                                            | &is_rlc
                                            | &is_rra
                                            | &is_rrc
                                            | &is_cca)
                                            .cmux(&r5, &r6)
                                    },
                                    || r7,
                                )
                            },
                        );

                        let (final_left, final_right) = rayon::join(
                            || {
                                (&is_lda_d
                                    | &is_lda_i
                                    | &is_swp
                                    | &is_djnz
                                    | &is_add
                                    | &is_addc
                                    | &is_add_i
                                    | &is_addc_i
                                    | &is_sub
                                    | &is_subc
                                    | &is_sub_i
                                    | &is_subc_i
                                    | &is_mul
                                    | &is_mul_i
                                    | &is_div)
                                    .cmux(&f1, &f2)
                            },
                            || {
                                (&is_div_i
                                    | &is_sla
                                    | &is_sra
                                    | &is_rla
                                    | &is_rlc
                                    | &is_rra
                                    | &is_rrc
                                    | &is_cca
                                    | &is_and
                                    | &is_or)
                                    .cmux(&f3, &f4)
                            },
                        );

                        next_a_final =
                            Some((&is_xor | &is_dec | &is_inc).cmux(&final_right, &final_left));
                    });
                },
            )
            .expect("");

        let next_a = next_a_final.unwrap();

        let mut next_b = self.b.clone();
        next_b = is_swp.cmux(&self.a, &next_b);
        next_b = is_mul.cmux(&res_mul_lower, &next_b);
        let mut next_pc;
        next_pc = (&self.pc + 2u8) % (self.memory.len() as u8);

        next_pc = trigger_branch.cmux(branch_target, &next_pc);

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
        self.pc = next_pc;
        self.carry = next_carry;
    }
}
