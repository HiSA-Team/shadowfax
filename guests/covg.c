/*
 * Minimal implementation of the CoVE guest ECALL for:
 *
 * struct sbiret sbi_covg_get_evidence(unsigned long pub_key_addr,
 *                                     unsigned long pub_key_size,
 *                                     unsigned long challenge_data_addr,
 *                                     unsigned long cert_format,
 *                                     unsigned long cert_addr_out,
 *                                     unsigned long cert_size);
 *
 * This performs an ECALL using the SBI calling convention:
 *   a0..a5  : args 0..5
 *   a6      : FID (function id)
 *   a7      : EID (extension id)
 * return:
 *   a0 = error
 *   a1 = value
 *
 * EID for COVG (ASCII "COVG") is 0x434F5647 per RFC.
 * The function id (FID) for get_evidence is FID #7 as specified.
 *
 * The actual service is implemented by the trap handler (hypervisor/TSM).
 * This code only performs the userspace/bare-metal ECALL invocation.
 *
 * Author: Giuseppe Capasso <capassog97@gmail.com>
 */


struct sbiret {
    long error;
    long value;
};

#define SBI_EXT_COVG            0x434F5647UL  /* 'C' 'O' 'V' 'G' */
#define SBI_EXT_COVG_GET_EVIDENCE_FID  8UL     /* FID #8 (per spec fragment provided) */

/* Publick Key */
static const unsigned char PUBLIC_KEY[] = {
/* 0000000 */  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00, 0x95, 0xa2, 0x23, 0xef,
/* 0000020 */  0x23, 0x51, 0x89, 0xc0, 0x27, 0x60, 0x86, 0x2b, 0xb5, 0xfb, 0x62, 0x73, 0x2e, 0x33, 0xba, 0x15,
/* 0000040 */  0x44, 0x27, 0xf7, 0x6e, 0x35, 0xe4, 0xcd, 0xd3, 0x5a, 0x68, 0x86, 0x27,
/* 0000054 */
};

/* Publick Key */
static const unsigned char NONCE[64] = {0};

/* Certificate Output */
static unsigned char CERTIFICATE_OUTPUT[256] = {0};

/* NOTE:
 * All args are passed as unsigned long to match SBI/syscall widths.
 * The inline asm uses register variables bound to the argument ABI registers.
 */
static inline struct sbiret sbi_covg_get_evidence(unsigned long pub_key_addr,
                                                  unsigned long pub_key_size,
                                                  unsigned long challenge_data_addr,
                                                  unsigned long cert_format,
                                                  unsigned long cert_addr_out,
                                                  unsigned long cert_size) {
    register unsigned long a0 asm("a0") = pub_key_addr;
    register unsigned long a1 asm("a1") = pub_key_size;
    register unsigned long a2 asm("a2") = challenge_data_addr;
    register unsigned long a3 asm("a3") = cert_format;
    register unsigned long a4 asm("a4") = cert_addr_out;
    register unsigned long a5 asm("a5") = cert_size;
    register unsigned long a6 asm("a6") = SBI_EXT_COVG_GET_EVIDENCE_FID;
    register unsigned long a7 asm("a7") = SBI_EXT_COVG;

    asm volatile (
        "ecall"
        : "+r"(a0), "+r"(a1)         /* a0,a1 are outputs (a0=err, a1=val) */
        : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
        : "memory"
    );

    struct sbiret ret;
    ret.error = (long)a0;
    ret.value = (long)a1;
    return ret;
}

/* Example main: call the ECALL (addresses must be page-aligned and confidential
 * per the spec; here we simply demonstrate the call).
 */
int main(void) {
    /* Example placeholders (must be replaced with proper confidential, page-aligned addresses
     * and sizes in a real TVM). */
    unsigned long pub_key_addr = (unsigned long) PUBLIC_KEY;          /* page-aligned physical/virtual addr */
    unsigned long pub_key_size = sizeof(PUBLIC_KEY);                  /* size in bytes (page-sized) */
    unsigned long challenge_addr = (unsigned long) NONCE;
    unsigned long cert_format = 0;  /* CBOR */
    unsigned long cert_addr_out = (unsigned long) CERTIFICATE_OUTPUT; /* where TSM should write the cert (page-aligned) */
    unsigned long cert_size = sizeof(CERTIFICATE_OUTPUT);             /* max cert buffer size */

    struct sbiret r = sbi_covg_get_evidence(pub_key_addr,
                                            pub_key_size,
                                            challenge_addr,
                                            cert_format,
                                            cert_addr_out,
                                            cert_size);

    /* r.error contains SBI error code (0 == success). r.value may carry a length or extra info */
    (void)r; /* silence unused warning */

    /* This example then spinloops. */
    while(1);

    /* unreachable */
    return 0;
}
