# Programs
CC        = $(CROSS_COMPILE)gcc
LD        = $(CROSS_COMPILE)ld
AS        = $(CROSS_COMPILE)as
OBJCOPY   = $(CROSS_COMPILE)objcopy

# ===============================
#  Compilation and Linking Flags
# ===============================

MARCH = rv64imac
MABI  = lp64

CFLAGS  = -Wall -Wextra -march=$(MARCH) -mabi=$(MABI)
LDFLAGS = -static -nostdlib
ASFLAGS = -march=$(MARCH) -mabi=$(MABI)
