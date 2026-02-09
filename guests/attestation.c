#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

// -----------------------------------------------------------------------------
// SBI Definitions
// -----------------------------------------------------------------------------
struct sbiret {
    long error;
    long value;
};

#define SBI_EXT_DBCN                    0x4442434E
#define SBI_EXT_DBCN_CONSOLE_WRITE      0x00
#define SBI_EXT_COVG                    0x434F5647
#define SBI_EXT_COVG_GET_EVIDENCE_FID   8

// -----------------------------------------------------------------------------
// Low-level SBI Wrapper
// -----------------------------------------------------------------------------
static inline struct sbiret sbi_call(unsigned long eid, unsigned long fid,
                                     unsigned long arg0, unsigned long arg1,
                                     unsigned long arg2, unsigned long arg3,
                                     unsigned long arg4, unsigned long arg5) {
    register unsigned long a0 asm("a0") = arg0;
    register unsigned long a1 asm("a1") = arg1;
    register unsigned long a2 asm("a2") = arg2;
    register unsigned long a3 asm("a3") = arg3;
    register unsigned long a4 asm("a4") = arg4;
    register unsigned long a5 asm("a5") = arg5;
    register unsigned long a6 asm("a6") = fid;
    register unsigned long a7 asm("a7") = eid;

    asm volatile ("ecall"
                  : "+r"(a0), "+r"(a1)
                  : "r"(a2), "r"(a3), "r"(a4), "r"(a5), "r"(a6), "r"(a7)
                  : "memory");

    struct sbiret ret;
    ret.error = (long)a0;
    ret.value = (long)a1;
    return ret;
}

// -----------------------------------------------------------------------------
// Console Output Helpers (DBCN)
// -----------------------------------------------------------------------------
void sbi_putc(char c) {
    // We use the console_write function which takes a length.
    // Usually FID 0 or 2 depending on version, implementation assumes FID 0 (write)
    // or FID 2 (write_byte). Let's use the one provided in your snippet:
    // Ideally DBCN write takes (num_bytes, addr_low, addr_high).
    // For single char, simple wrappers often exist.
    // Adapting to your previous snippet's style:
    unsigned long base_addr = (unsigned long)&c;
    sbi_call(SBI_EXT_DBCN, 2, (unsigned long)c, 0, 0, 0, 0, 0);
}

void print_str(const char *s) {
    while (*s) sbi_putc(*s++);
}

void print_hex(const uint8_t *buf, size_t len) {
    const char hex[] = "0123456789ABCDEF";
    for (size_t i = 0; i < len; i++) {
        sbi_putc(hex[(buf[i] >> 4) & 0xF]);
        sbi_putc(hex[buf[i] & 0xF]);
        // Add spacing for readability
        if (i < len - 1 && (i+1) % 16 == 0) print_str("\n        ");
        else if (i < len - 1) sbi_putc(' ');
    }
    print_str("\n");
}

void print_uint(unsigned long val) {
    char buf[21];
    int i = 0;
    if (val == 0) { print_str("0"); return; }
    while (val > 0) {
        buf[i++] = (val % 10) + '0';
        val /= 10;
    }
    while (i > 0) sbi_putc(buf[--i]);
}

// -----------------------------------------------------------------------------
// Minimal CBOR Parser (Zero Dependency)
// -----------------------------------------------------------------------------
// Major Types (MT) - Top 3 bits
#define CBOR_MT_UINT    0   // 0x00..0x17
#define CBOR_MT_NINT    1   // 0x20..0x37
#define CBOR_MT_BSTR    2   // 0x40..0x57 (Byte String)
#define CBOR_MT_TSTR    3   // 0x60..0x77 (Text String)
#define CBOR_MT_ARRAY   4   // 0x80..0x97
#define CBOR_MT_MAP     5   // 0xA0..0xB7
#define CBOR_MT_TAG     6   // 0xC0..0xD7
#define CBOR_MT_SIMPLE  7   // 0xE0..0xF7 (Simple/Float)

// Simple Values (When MT is 7)
#define CBOR_SV_FALSE   20
#define CBOR_SV_TRUE    21
#define CBOR_SV_NULL    22
#define CBOR_SV_UNDEF   23

// Common Tags
#define CBOR_TAG_COSE_SIGN1 18

void debug_cursor(const char* label, uint8_t *curr, size_t lookahead) {
    print_str("    DEBUG ["); print_str(label); print_str("]: ");
    print_hex(curr, lookahead);
}

// Reads the header, returns the major type and the argument (value or length)
// Updates the cursor. Returns 0 on success, -1 on bounds check error.
int cbor_read_head(uint8_t **cursor, uint8_t *end, uint8_t *major_type, uint64_t *val) {
    if (*cursor >= end) return -1;

    uint8_t byte = **cursor;
    *cursor += 1;

    *major_type = (byte >> 5) & 0x07;
    uint8_t info = byte & 0x1F;

    if (info < 24) {
        *val = info;
    } else if (info == 24) {
        if (*cursor + 1 > end) return -1;
        *val = **cursor;
        *cursor += 1;
    } else if (info == 25) {
        if (*cursor + 2 > end) return -1;
        *val = ((uint64_t)(*cursor)[0] << 8) | (*cursor)[1];
        *cursor += 2;
    } else if (info == 26) {
        if (*cursor + 4 > end) return -1;
        // Big endian read
        *val = 0;
        for (int i = 0; i < 4; i++) *val = (*val << 8) | (*cursor)[i];
        *cursor += 4;
    } else if (info == 27) {
        if (*cursor + 8 > end) return -1;
        *val = 0;
        for (int i = 0; i < 8; i++) *val = (*val << 8) | (*cursor)[i];
        *cursor += 8;
    } else {
        return -1; // Unhandled (reserved/float)
    }
    return 0;
}

// Skips a single CBOR item (recursive)
int cbor_skip(uint8_t **cursor, uint8_t *end) {
    uint8_t mt;
    uint64_t val;
    if (cbor_read_head(cursor, end, &mt, &val) != 0) return -1;

    switch (mt) {
        case CBOR_MT_UINT:
        case CBOR_MT_NINT:
            // Value is already read/skipped by read_head
            break;
        case CBOR_MT_BSTR:
        case CBOR_MT_TSTR:
            // val is length
            if (*cursor + val > end) return -1;
            *cursor += val;
            break;
        case CBOR_MT_ARRAY:
            for (uint64_t i = 0; i < val; i++) {
                if (cbor_skip(cursor, end) != 0) return -1;
            }
            break;
        case CBOR_MT_MAP:
            for (uint64_t i = 0; i < val * 2; i++) { // Key + Value
                if (cbor_skip(cursor, end) != 0) return -1;
            }
            break;
        case CBOR_MT_TAG:
            // Tag is like a wrapper, skip the content inside
            if (cbor_skip(cursor, end) != 0) return -1;
            break;
        default:
            return -1;
    }
    return 0;
}

// -----------------------------------------------------------------------------
// Data (Aligned for Safety)
// -----------------------------------------------------------------------------
// We use 4K alignment to ensure these buffers don't straddle page boundaries,
// reducing the complexity required in the Hypervisor's translation logic.
#define PAGE_SIZE 4096

__attribute__((aligned(PAGE_SIZE)))
static uint8_t CHALLENGE[64];

__attribute__((aligned(PAGE_SIZE)))
static uint8_t CERT_BUFFER[4096]; // 4KB should be enough for 3 certificates

void parse_and_print_evidence(uint8_t *data, size_t len) {
    uint8_t *curr = data;
    uint8_t *end = data + len;
    uint8_t mt;
    uint64_t val;

    print_str("[GUEST] Parsing Evidence CBOR...\n");

    // 1. Evidence Envelope: Expect Array(3)
    if (cbor_read_head(&curr, end, &mt, &val) != 0) {
        print_str("Err: Stream Empty\n"); return;
    }
    if (mt != CBOR_MT_ARRAY || val != 3) {
        print_str("Error: Expected Evidence Array(3), got MT=");
        print_uint(mt); print_str(" Len="); print_uint(val); print_str("\n");
        return;
    }

    const char *names[] = {"Platform", "TSM     ", "TVM     "};

    for (int i = 0; i < 3; i++) {
        print_str("  --------------------------------------------------\n");
        print_str("  Layer: ");
        print_str(names[i]);
        print_str("\n");

        // Peek/Read Head for the COSE_Sign1 Item
        uint8_t *item_start = curr;
        if (cbor_read_head(&curr, end, &mt, &val) != 0) {
            print_str("    Err: Unexpected End\n");
            return;
        }

        // A. Handle Tags (e.g. Tag 18 for COSE_Sign1)
        // Some implementations wrap the Array in a Tag, others don't. We handle both.
        while (mt == CBOR_MT_TAG) {
            print_str("    [Info] Tag: "); print_uint(val);
            if (val == CBOR_TAG_COSE_SIGN1) print_str(" (COSE_Sign1)");
            print_str("\n");

            // Read the next item (the content being tagged)
            if (cbor_read_head(&curr, end, &mt, &val) != 0) return;
        }

        // B. Expect Array(4) for COSE_Sign1 structure
        if (mt != CBOR_MT_ARRAY || val != 4) {
            print_str("    ERROR: Expected COSE Array(4). Got MT=");
            print_uint(mt); print_str(" Val="); print_uint(val); print_str("\n");
            debug_cursor("BAD BYTES", item_start, 16);
            return;
        }

        // --- FIELD 1: PROTECTED HEADERS (BSTR or Empty BSTR) ---
        cbor_read_head(&curr, end, &mt, &val);
        if (mt == CBOR_MT_BSTR) {
            // print_str("    Protected Len: "); print_uint(val); print_str("\n");
            curr += val;
        } else {
             print_str("    Err: Protected Header must be BSTR. Got MT="); print_uint(mt); print_str("\n");
             return;
        }

        // --- FIELD 2: UNPROTECTED HEADERS (Map) ---
        cbor_read_head(&curr, end, &mt, &val);
        if (mt == CBOR_MT_MAP) {
            // print_str("    Unprotected Map Items: "); print_uint(val); print_str("\n");
            for(int k=0; k<val*2; k++) cbor_skip(&curr, end);
        } else {
             print_str("    Err: Unprotected Header must be MAP. Got MT="); print_uint(mt); print_str("\n");
             return;
        }

        // --- FIELD 3: PAYLOAD (BSTR or NULL) ---
        cbor_read_head(&curr, end, &mt, &val);

        if (mt == CBOR_MT_SIMPLE && val == CBOR_SV_NULL) {
            print_str("    Payload: [NULL] (Detached content)\n");
            // Do NOT increment curr. 'val' is the value (22), not a length.
        }
        else if (mt == CBOR_MT_BSTR) {
            print_str("    Payload: "); print_uint(val); print_str(" bytes\n");

            // Safety Check
            if (curr + val > end) { print_str("    Err: Buffer Overflow\n"); return; }

            // Print Preview for TVM (ASCII check)
            if (i == 2 && val > 0) {
                 print_str("    Preview: ");
                 print_hex(curr, val > 16 ? 16 : val);
            }
            curr += val; // Advance cursor by length
        }
        else {
            print_str("    Err: Payload must be BSTR or NULL. Got MT="); print_uint(mt); print_str("\n");
            return;
        }

        // --- FIELD 4: SIGNATURE (BSTR) ---
        cbor_read_head(&curr, end, &mt, &val);
        if (mt == CBOR_MT_BSTR) {
            print_str("    Signature: "); print_uint(val); print_str(" bytes\n");
            curr += val;
        } else {
             print_str("    Err: Signature must be BSTR. Got MT="); print_uint(mt); print_str("\n");
             return;
        }
    }
    print_str("  --------------------------------------------------\n");
}

int main(void) {
    print_str("\n==========================================\n");
    print_str("[GUEST] TVM Started. Initializing Test...\n");

    // 1. Prepare Challenge Data
    // We fill it with a pattern so we can verify the attestation later if needed
    for (int i = 0; i < 64; i++) {
        CHALLENGE[i] = 0xAA;
    }
    print_str("[GUEST] Challenge Data prepared (0xAA...)\n");

    // 2. Clear Output Buffer
    for (int i = 0; i < sizeof(CERT_BUFFER); i++) CERT_BUFFER[i] = 0;

    // 3. Call Hypervisor (SBI)
    unsigned long pub_key_addr = 0; // Not used in your implementation yet
    unsigned long pub_key_size = 0;

    print_str("[GUEST] Invoking SBI_COVG_GET_EVIDENCE...\n");

    struct sbiret ret = sbi_call(
        SBI_EXT_COVG,
        SBI_EXT_COVG_GET_EVIDENCE_FID,
        pub_key_addr,
        pub_key_size,
        (unsigned long)CHALLENGE,
        0, // Format
        (unsigned long)CERT_BUFFER,
        sizeof(CERT_BUFFER)
    );

    // 4. Handle Result
    if (ret.error != 0) {
        print_str("[GUEST] SBI Call Failed! Error: ");
        print_uint(ret.error); // Usually prints as unsigned, so -1 looks huge
        print_str("\n");
    } else {
        size_t evidence_len = (size_t)ret.value;
        print_str("[GUEST] Success! Evidence received.\n");
        print_str("[GUEST] Total Size: "); print_uint(evidence_len); print_str(" bytes.\n");

        // 5. Parse and Print Details
        parse_and_print_evidence(CERT_BUFFER, evidence_len);

        // Optional: Dump full hex
        print_str("[GUEST] Raw Dump:\n");
        print_hex(CERT_BUFFER, evidence_len);
    }

    print_str("[GUEST] Test Complete. Halting.\n");
    while(1) {
        asm volatile ("wfi");
    }
}
