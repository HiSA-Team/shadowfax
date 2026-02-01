#!/bin/sh

qemu-system-riscv64 -nographic \
  -M virt \
  -cpu 'rv64,h=true' \
  -smp 1 \
  -bios target/riscv64imac-unknown-none-elf/debug/shadowfax \
  -m 64M \
  -device loader,file=linux.bin,addr=0x80A00000,force-raw=on \
  -device loader,file=initramfs.cpio.gz,addr=0x80820000 \
  -dtb device-tree.dtb \
  -netdev tap,id=net0,ifname=tap0,script=no,downscript=no \
  -device virtio-net-device,netdev=net0
