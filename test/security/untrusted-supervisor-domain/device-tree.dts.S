/include/ "platform/generic/device-tree.dts"

/* Test-local two-hart extension; the generic platform remains unchanged. */
&{/cpus} {
	cpu1: cpu@1 {
		device_type = "cpu";
		reg = <0x01>;
		status = "okay";
		compatible = "riscv";
		riscv,cbop-block-size = <0x40>;
		riscv,cboz-block-size = <0x40>;
		riscv,cbom-block-size = <0x40>;
		riscv,isa-extensions = "i", "m", "a", "f", "d", "c", "h", "zic64b", "zicbom", "zicbop", "zicboz", "ziccamoa", "ziccif", "zicclsm", "ziccrse", "zicntr", "zicsr", "zifencei", "zihintntl", "zihintpause", "zihpm", "zmmul", "za64rs", "zaamo", "zalrsc", "zawrs", "zfa", "zca", "zcd", "zba", "zbb", "zbc", "zbs", "ssccptr", "sscounterenw", "sstc", "sstvala", "sstvecd", "svadu", "svvptc";
		riscv,isa-base = "rv64i";
		riscv,isa = "rv64imafdch_zic64b_zicbom_zicbop_zicboz_ziccamoa_ziccif_zicclsm_ziccrse_zicntr_zicsr_zifencei_zihintntl_zihintpause_zihpm_zmmul_za64rs_zaamo_zalrsc_zawrs_zfa_zca_zcd_zba_zbb_zbc_zbs_ssccptr_sscounterenw_sstc_sstvala_sstvecd_svadu_svvptc";
		mmu-type = "riscv,sv57";
		opensbi-domain = <&attacker_domain>;

		cpu1_intc: interrupt-controller {
			#interrupt-cells = <0x01>;
			interrupt-controller;
			compatible = "riscv,cpu-intc";
		};
	};
};

&{/cpus/cpu-map/cluster0} {
	core1 { cpu = <&cpu1>; };
};

&{/chosen} {
	opensbi-config {
		compatible = "opensbi,config";
		cold-boot-harts = <&cpu0>;
	};
};

&{/chosen/opensbi-domains} {
	attacker_mem: attacker-memory {
		compatible = "opensbi,domain,memregion";
		base = <0x0 0x94000000>;
		order = <25>;
	};

	coord_mem: coordination-memory {
		compatible = "opensbi,domain,memregion";
		base = <0x0 COORDINATION_ADDRESS>;
		order = <12>;
	};

	attacker_domain: attacker-domain {
		compatible = "opensbi,domain,instance";
		possible-harts = <&cpu1>;
		boot-hart = <&cpu1>;
		regions = <&attacker_mem 0x3f>, <&coord_mem 0x3f>, <&tdevices 0x3f>;
		next-arg1 = <0x0 0x0>;
		next-addr = <0x0 ATTACKER_ADDRESS>;
		next-mode = <0x1>;
	};
};

&udomain {
	regions = <&umem 0x3f>, <&umem_high 0x3f>, <&uplic 0x3f>,
		  <&udevices 0x3f>, <&coord_mem 0x3f>;
};

&{/soc/plic@c000000} {
	interrupts-extended = <&{/cpus/cpu@0/interrupt-controller} 0x0b
			       &{/cpus/cpu@0/interrupt-controller} 0x09
			       &cpu1_intc 0x0b &cpu1_intc 0x09>;
};

&{/soc/clint@2000000} {
	interrupts-extended = <&{/cpus/cpu@0/interrupt-controller} 0x03
			       &{/cpus/cpu@0/interrupt-controller} 0x07
			       &cpu1_intc 0x03 &cpu1_intc 0x07>;
};
