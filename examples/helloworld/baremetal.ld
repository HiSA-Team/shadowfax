/* Run qemu-system-riscv64 -nographic -machine virt to get machine info.
 * Firmware Base             : 0x80000000
 * Domain0 Next Address      : 0x0000000000000000
 * Domain0 Next Arg1         : 0x0000000087e00000
 * Domain0 Next Mode         : S-mode
 * Domain0 SysReset          : yes
*/

SECTIONS {
  . = 0x80000000;													/* QEMU default load address to run bios */
  .text : {
		KEEP(*(.text._start));								/* Ensure _start is placed first */
		*(.text*);														/* Program code here */
	}
	. = ALIGN (CONSTANT (COMMONPAGESIZE));	/* Make sure linker does not jam data into text section, making text writable */
  .data : { 
		*(.data*)															/* Stack goes here */
	}
}
