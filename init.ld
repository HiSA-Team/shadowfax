/* Author: Giuseppe Capasso <capassog97@gmail.com>
 *
*/
SECTIONS {
  . = 0x80000000;													/* QEMU default load address to run bios */
  PROVIDE(_fw_start = .);
  .text : {
		KEEP(*(.text._start));								/* Ensure _start is placed first */
		*(.text*);														/* Program code here */
	}
	/* Beginning of the read-only data sections */

	/*
	 * PMP regions must be to be power-of-2. RX/RW will have separate
	 * regions, so ensure that the split is power-of-2.
	 */
	. = ALIGN(1 << LOG2CEIL(SIZEOF(.text)));

	PROVIDE(_fw_rw_start = .);

	/* Beginning of the read-write data sections */

	.data :
	{
		PROVIDE(_data_start = .);

		*(.sdata)
		*(.sdata.*)
		*(.data)
		*(.data.*)
		*(.readmostly.data)
		*(*.data)
		. = ALIGN(8);

		PROVIDE(_data_end = .);
	}

	. = ALIGN(0x1000); /* Ensure next section is page aligned */

  PROVIDE(_fw_end = .);
}
