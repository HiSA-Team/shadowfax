#include <stddef.h>
#include <stdint.h>

#include "baremetal.h"

/*
 * GPA window mapped with ADD_ZERO_PAGES: past the guest image (linked at
 * 0x10000, 128 KiB) and inside the launcher's 2 MiB guest RAM.
 */
#define WINDOW_START 0x100000UL
#define WINDOW_END   0x200000UL
#define SECRET       0xa5

int main(void)
{
    volatile unsigned char *window = (volatile unsigned char *)WINDOW_START;

    for (uintptr_t offset = 0; offset < WINDOW_END - WINDOW_START; ++offset) {
        if (window[offset] != 0) {
            puts("[TVM] LEAK: zero page holds ");
            puthex(window[offset]);
            puts(" at GPA ");
            puthex(WINDOW_START + offset);
            putchar('\n');
            /* The host only continues after a clean shutdown: hang to fail the run. */
            halt();
        }
    }
    puts("[TVM] zero pages are zero; planting secret\n");

    for (uintptr_t offset = 0; offset < WINDOW_END - WINDOW_START; ++offset)
        window[offset] = SECRET;
    shutdown();
}
