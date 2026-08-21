/include/ "platform/generic/device-tree.dts"

/* The generic TSM remains domain 1 and is reduced to its own 32 MiB. */
&tmem {
	base = <0x0 0x90000000>;
	order = <25>;
};

&tdomain {
	shadowfax,tsm;
};

&{/reserved-memory} {
	tsm_staging: tsm-images@94000000 {
		reg = <0x0 0x94000000 0x0 0x02000000>;
		no-map;
	};
};

/*
 * Overlay nodes are appended after the generic untrusted domain. OpenSBI
 * therefore assigns domain 3 to this second trusted supervisor domain.
 */
&{/chosen/opensbi-domains} {
	tmem_secondary: tmem-secondary {
		compatible = "opensbi,domain,memregion";
		base = <0x0 0x92000000>;
		order = <25>;
	};

	tdomain_secondary: trusted-domain-secondary {
		compatible = "opensbi,domain,instance";
		possible-harts = <&cpu0>;
		regions = <&tmem_secondary 0x3f>, <&tdevices 0x3f>;
		next-arg1 = <0x0 0x0>;
		next-addr = <0x0 0x92000000>;
		next-mode = <0x1>;
		shadowfax,tsm;
		shadowfax,tsm-image = <0x0 EXTERNAL_TSM_ADDR 0x0 EXTERNAL_TSM_SIZE>;
		shadowfax,tsm-signature = <0x0 EXTERNAL_TSM_SIGNATURE_ADDR 0x0 EXTERNAL_TSM_SIGNATURE_SIZE>;
		shadowfax,trusts = <&udomain>;
	};
};
