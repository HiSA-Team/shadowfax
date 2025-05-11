# Centralized file to manage build variables for payloads using `make`.
# Usage:
# 	When compiling use CROSS_COMPILE to pass the start of your ie. toolchain
# 	eg. make CROSS_COMPILE=riscv64-linux-gnu-
#
# Author: Giuseppe Capasso <capassog97@gmail.com>

ifdef CROSS_COMPILE
CC				=	$(CROSS_COMPILE)gcc
AR				=	$(CROSS_COMPILE)ar
LD				=	$(CROSS_COMPILE)ld
AS				=	$(CROSS_COMPILE)as
OBJCOPY		=	$(CROSS_COMPILE)objcopy
OBJDUMP		=	$(CROSS_COMPILE)objdump
else
CC				?=	gcc
AR				?=	ar
LD				?=	ld
AS				?=	as
OBJCOPY 	?=	objcopy
OBJDUMP		?=	objdump
endif

MARCH ?= rv64imac
MABI ?= lp64

CFLAGS  = -Wall -Wextra -march=$(MARCH) -mabi=$(MABI)
LDFLAGS =
ASFLAGS = -march=$(MARCH) -mabi=$(MABI)
