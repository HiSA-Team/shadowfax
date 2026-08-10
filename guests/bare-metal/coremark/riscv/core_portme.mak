# Copyright 2018 Embedded Microprocessor Benchmark Consortium (EEMBC)
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#    http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# Original Author: Giuseppe Capasso

#File : core_portme.mak

# Flag : RV_PREFIX
# Specify where to look for the riscv toolchain
RV_PREFIX =
# Flag : OUTFLAG
#	Use this flag to define how to to get an executable (e.g -o)
OUTFLAG= -o
# Flag : CC
#	Use this flag to define compiler to use
CC 		= $(RV_PREFIX)gcc
# Flag : LD
#	Use this flag to define compiler to use
LD		= $(RV_PREFIX)ld
# Flag : AS
#	Use this flag to define compiler to use
AS		= $(RV_PREFIX)as
# Flag : CFLAGS
#	Use this flag to define compiler options. Note, you can add compiler options from the command line using XCFLAGS="other flags"
PORT_CFLAGS = -O1 -march=rv64gc -mabi=lp64 -Wall -Wextra -mcmodel=medany
FLAGS_STR = "$(PORT_CFLAGS) $(XCFLAGS) $(XLFLAGS) $(LFLAGS_END)"
CFLAGS = $(PORT_CFLAGS) -I$(PORT_DIR) -I. -DFLAGS_STR=\"$(FLAGS_STR)\"
#Flag : LFLAGS_END
#	Define any libraries needed for linking or other flags that should come at the end of the link line (e.g. linker scripts).
#	Note : On certain platforms, the default clock_gettime implementation is supported but requires linking of librt.
SEPARATE_COMPILE=1
# Flag : SEPARATE_COMPILE
# You must also define below how to create an object file, and how to link.

LFLAGS_END =

# Conditional logic for S-Mode
ifeq ($(SMODE),1)
    PORT_CFLAGS += -DSMODE
    SBI_SRC = $(PORT_DIR)/sbi.c
    SBI_OBJ = $(PORT_DIR)/sbi.o
    LINKERSCRIPT  = $(PORT_DIR)/smode.ld
else
    PORT_CFLAGS +=
    SBI_SRC =
    SBI_OBJ =
    LINKERSCRIPT  = $(PORT_DIR)/bare.ld
endif

OBJOUT 	= -o
LFLAGS 	= -nostdlib -T$(LINKERSCRIPT)
ASFLAGS =
OFLAG 	= -o
COUT 	= -c
# Flag : PORT_SRCS
# 	Port specific source files can be added here
#	You may also need cvt.c if the fcvt functions are not provided as intrinsics by your compiler!
PORT_SRCS = $(PORT_DIR)/core_portme.c $(PORT_DIR)/ee_printf.c $(PORT_DIR)/crt0.S $(SBI_SRC)
PORT_OBJS = $(PORT_DIR)/core_portme.o $(PORT_DIR)/ee_printf.o $(PORT_DIR)/crt0.o $(SBI_OBJ)
vpath %.c $(PORT_DIR)
vpath %.s $(PORT_DIR)

# Flag : LOAD
#	For a simple port, we assume self hosted compile and run, no load needed.

# Flag : RUN
#	For a simple port, we assume self hosted compile and run, simple invocation of the executable

LOAD = echo "Please set LOAD to the process of loading the executable to the flash"
RUN = echo "Please set LOAD to the process of running the executable (e.g. via jtag, or board reset)"

OEXT = .o
EXE = .bin

$(OPATH)$(PORT_DIR)/%$(OEXT) : %.c
	$(CC) $(CFLAGS) $(XCFLAGS) $(COUT) $< $(OBJOUT) $@

$(OPATH)%$(OEXT) : %.c
	$(CC) $(CFLAGS) $(XCFLAGS) $(COUT) $< $(OBJOUT) $@

$(OPATH)$(PORT_DIR)/%$(OEXT) : %.s
	$(AS) $(ASFLAGS) $< $(OBJOUT) $@

# Target : port_pre% and port_post%
# For the purpose of this simple port, no pre or post steps needed.

.PHONY : port_prebuild port_postbuild port_prerun port_postrun port_preload port_postload
port_pre% port_post% :

# FLAG : OPATH
# Path to the output folder. Default - current folder.
OPATH = ./
MKDIR = mkdir -p
