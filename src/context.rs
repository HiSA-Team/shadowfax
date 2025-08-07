#[derive(Clone, Debug)]
#[repr(C, align(4))]
pub struct Context {
    pub regs: [usize; 32],

    sstatus: usize,
    pub stvec: usize,
    sip: usize,
    scounteren: usize,
    sscratch: usize,
    satp: usize,
    senvcfg: usize,
    scontext: usize,
    pub mepc: usize,

    pub pmpcfg: usize,
    pub pmpaddr: [usize; 8],
    interrupted: usize,
}
