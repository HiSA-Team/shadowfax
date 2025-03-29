/* This is from QEMU.
 * Run qemu-system-riscv64 -nographic -machine virt to get machine info.
 * Firmware Base             : 0x80000000
 * Domain0 Next Address      : 0x0000000000000000
 * Domain0 Next Arg1         : 0x0000000087e00000
 * Domain0 Next Mode         : S-mode
 * Domain0 SysReset          : yes
*/

SECTIONS {
  . = 0x80000000;                           /* QEMU default load address to run bios */
	PROVIDE(_fw_start = .);

  . = ALIGN (0x1000);
  .text : {
    KEEP(*(.text._start));                  /* Ensure _start is placed first */
    *(.text*);                              /* Program code here */
  }

	. = ALIGN(1 << LOG2CEIL(SIZEOF(.text)));

  PROVIDE(_fw_rw_start = .);
  .data : {
    *(.data*)															  /* Stack goes here */
  }

  . = ALIGN(0x1000); /* Ensure next section is page aligned */

	.bss :
	{
		PROVIDE(_bss_start = .);
		*(.sbss)
		*(.sbss.*)
		*(.bss)
		*(.bss.*)
		. = ALIGN(8);
		PROVIDE(_bss_end = .);
	}

	/* End of the read-write data sections */

	. = ALIGN(0x1000); /* Need this to create proper sections */

	PROVIDE(_fw_end = .);

  . = 0x80060000;
	. = ALIGN(0x1000);
	.payload :
	{
		PROVIDE(_payload_start = .);
		*(.payload)
		. = ALIGN(8);
		PROVIDE(_payload_end = .);
	}
}
