# Author: Giuseppe Capasso
# Email: capassog97@gmail.com
# Centralized file to manage build variables. This will be included in example, tests and scripts.
# Usage:
# 	When compiling use CROSS_COMPILE to pass the start of your ie. toolchain
# 	eg. make CROSS_COMPILE=riscv64-linux-musl

ifdef CROSS_COMPILE
CC		=	$(CROSS_COMPILE)gcc
AR		=	$(CROSS_COMPILE)ar
LD		=	$(CROSS_COMPILE)ld
AS 		= $(CROSS_COMPILE)as
else
CC		?=	gcc
AR		?=	ar
LD		?=	ld
AS		?=	as
endif

ARCH = rv64gh

CFLAGS = -Wall -Wextra
LDFLAGS =
