ROOT                := ../..
LAUNCHER_NAME       := tvm-launch-latency
LAUNCHER_METADATA   := 1M
GUEST_MEMORY_SIZE   := 268435456
GUEST_IN_RAM        := 0
RUN_QEMU_FLAGS      := -M virt -m 1G -smp 1 -nographic -monitor none -no-reboot

include $(ROOT)/guests/bare-metal/launcher.mk
