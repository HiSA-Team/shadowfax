use core::arch::asm;

#[no_mangle]
#[repr(align(4))]
pub unsafe extern "C" fn hstrap_vector() -> ! {
    // Save context
    asm!(
        ".align 4
        fence.i

        // swap original mode sp for HS-mode sp
        csrrw sp, sscratch, sp
        addi sp, sp, -256  // Reserve space for context

        // save registers
        sd ra, 1*8(sp)
        sd gp, 3*8(sp)
        sd tp, 4*8(sp)
        sd t0, 5*8(sp)
        sd t1, 6*8(sp)
        sd t2, 7*8(sp)
        sd s0, 8*8(sp)
        sd s1, 9*8(sp)
        sd a0, 10*8(sp)
        sd a1, 11*8(sp)
        sd a2, 12*8(sp)
        sd a3, 13*8(sp)
        sd a4, 14*8(sp)
        sd a5, 15*8(sp)
        sd a6, 16*8(sp)
        sd a7, 17*8(sp)
        sd s2, 18*8(sp)
        sd s3, 19*8(sp)
        sd s4, 20*8(sp)
        sd s5, 21*8(sp)
        sd s6, 22*8(sp)
        sd s7, 23*8(sp)
        sd s8, 24*8(sp)
        sd s9, 25*8(sp)
        sd s10, 26*8(sp)
        sd s11, 27*8(sp)
        sd t3, 28*8(sp)
        sd t4, 29*8(sp)
        sd t5, 30*8(sp)
        sd t6, 31*8(sp)",
        options(noreturn)
    );

    // Dispatch trap
    handle_trap();

    // We should never reach here
    loop {}
}

fn handle_trap() {
    let cause: usize;
    let epc: usize;

    unsafe {
        asm!("csrr {}, scause", out(reg) cause);
        asm!("csrr {}, sepc", out(reg) epc);
    }

    // Check if it's an interrupt or exception
    if cause & (1 << 63) != 0 {
        // Handle interrupt
        handle_interrupt(cause & 0xff);
    } else {
        // Handle exception
        handle_exception(cause & 0xff, epc);
    }

    // Return to guest
    hstrap_exit();
}

fn handle_interrupt(interrupt_num: usize) {
    // Simple interrupt handling
    match interrupt_num {
        1 => {
            // Supervisor software interrupt
            // Forward to guest
            unsafe {
                asm!("csrs hvip, 1 << 1");
            }
        }
        5 => {
            // Supervisor timer interrupt
            // Forward to guest
            unsafe {
                asm!("csrs hvip, 1 << 5");
                asm!("csrc sie, 1 << 5"); // Clear timer interrupt
            }
        }
        9 => {
            // Supervisor external interrupt
            // Forward to guest
            unsafe {
                asm!("csrs hvip, 1 << 9");
                asm!("csrc sie, 1 << 9"); // Clear external interrupt
            }
        }
        _ => {
            // Unknown interrupt
            // Just ignore for now
        }
    }
}

fn handle_exception(exception_num: usize, epc: usize) {
    match exception_num {
        10 => {
            // Environment call from VS-mode
            // Handle SBI calls here
            // For simplicity, we'll just increment sepc to skip the ecall instruction
            unsafe {
                asm!("csrw sepc, {}", in(reg) epc + 4);
            }
        }
        _ => {
            // Forward other exceptions to the guest
            hs_forward_exception();
        }
    }
}

fn hs_forward_exception() {
    unsafe {
        let sepc: usize;
        let scause: usize;
        let stval: usize;

        asm!(
            "csrr {}, sepc",
            "csrr {}, scause",
            "csrr {}, stval",
            out(reg) sepc,
            out(reg) scause,
            out(reg) stval
        );

        // Forward to guest
        asm!(
            "csrw vsepc, {}",
            "csrw vscause, {}",
            "csrw vstval, {}",
            in(reg) sepc,
            in(reg) scause,
            in(reg) stval
        );

        // Get vstvec value
        let vstvec: usize;
        asm!("csrr {}, vstvec", out(reg) vstvec);

        // Set sepc to vstvec
        asm!("csrw sepc, {}", in(reg) vstvec);
    }
}

#[no_mangle]
pub extern "C" fn hstrap_exit() -> ! {
    unsafe {
        asm!(
            // restore registers
            "ld ra, 1*8(sp)
            ld gp, 3*8(sp)
            ld tp, 4*8(sp)
            ld t0, 5*8(sp)
            ld t1, 6*8(sp)
            ld t2, 7*8(sp)
            ld s0, 8*8(sp)
            ld s1, 9*8(sp)
            ld a0, 10*8(sp)
            ld a1, 11*8(sp)
            ld a2, 12*8(sp)
            ld a3, 13*8(sp)
            ld a4, 14*8(sp)
            ld a5, 15*8(sp)
            ld a6, 16*8(sp)
            ld a7, 17*8(sp)
            ld s2, 18*8(sp)
            ld s3, 19*8(sp)
            ld s4, 20*8(sp)
            ld s5, 21*8(sp)
            ld s6, 22*8(sp)
            ld s7, 23*8(sp)
            ld s8, 24*8(sp)
            ld s9, 25*8(sp)
            ld s10, 26*8(sp)
            ld s11, 27*8(sp)
            ld t3, 28*8(sp)
            ld t4, 29*8(sp)
            ld t5, 30*8(sp)
            ld t6, 31*8(sp)

            // restore sp
            addi sp, sp, 256
            csrrw sp, sscratch, sp

            sret",
            options(noreturn)
        );
    }
}
