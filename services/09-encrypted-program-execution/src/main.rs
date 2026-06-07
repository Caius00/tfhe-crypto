use crate::cpu::*;
use tfhe::ConfigBuilder;

mod cpu;

fn main() {
    let config = ConfigBuilder::default().build();
    let (_, sk) = tfhe::generate_keys(config);
    let mut cpu: CPU<4> = make_cpu();
    cpu.execute_program(1, &sk);
}
