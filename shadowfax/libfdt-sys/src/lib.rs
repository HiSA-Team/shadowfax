//! Minimal libfdt ABI used by `libfdt-rs`.
//!
//! Shadowfax already links OpenSBI's libfdt into `libplatsbi.a`; building the
//! copy bundled by `libfdt-sys` would require a bare-metal libc and would
//! duplicate the implementation. These declarations bind `libfdt-rs` to the
//! existing OpenSBI symbols instead.

#![no_std]

use core::ffi::{c_char, c_int, c_void};

pub const FDT_ERR_NOTFOUND: u32 = 1;
pub const FDT_ERR_EXISTS: u32 = 2;
pub const FDT_ERR_NOSPACE: u32 = 3;
pub const FDT_ERR_BADOFFSET: u32 = 4;
pub const FDT_ERR_BADPATH: u32 = 5;
pub const FDT_ERR_BADPHANDLE: u32 = 6;
pub const FDT_ERR_BADSTATE: u32 = 7;
pub const FDT_ERR_TRUNCATED: u32 = 8;
pub const FDT_ERR_BADMAGIC: u32 = 9;
pub const FDT_ERR_BADVERSION: u32 = 10;
pub const FDT_ERR_BADSTRUCTURE: u32 = 11;
pub const FDT_ERR_BADLAYOUT: u32 = 12;
pub const FDT_ERR_INTERNAL: u32 = 13;
pub const FDT_ERR_BADNCELLS: u32 = 14;
pub const FDT_ERR_BADVALUE: u32 = 15;
pub const FDT_ERR_BADOVERLAY: u32 = 16;
pub const FDT_ERR_NOPHANDLES: u32 = 17;
pub const FDT_ERR_BADFLAGS: u32 = 18;
pub const FDT_ERR_ALIGNMENT: u32 = 19;
pub const FDT_MAX_PHANDLE: u32 = 0xffff_fffe;

unsafe extern "C" {
    pub fn fdt_check_header(fdt: *const c_void) -> c_int;
    pub fn fdt_create(buf: *mut c_void, bufsize: c_int) -> c_int;
    pub fn fdt_path_offset(fdt: *const c_void, path: *const c_char) -> c_int;
    pub fn fdt_first_subnode(fdt: *const c_void, offset: c_int) -> c_int;
    pub fn fdt_next_subnode(fdt: *const c_void, offset: c_int) -> c_int;
    pub fn fdt_get_name(fdt: *const c_void, nodeoffset: c_int, lenp: *mut c_int)
        -> *const c_char;
    pub fn fdt_first_property_offset(fdt: *const c_void, nodeoffset: c_int) -> c_int;
    pub fn fdt_next_property_offset(fdt: *const c_void, offset: c_int) -> c_int;
    pub fn fdt_getprop_by_offset(
        fdt: *const c_void,
        offset: c_int,
        namep: *mut *const c_char,
        lenp: *mut c_int,
    ) -> *const c_void;
    pub fn fdt_getprop(
        fdt: *const c_void,
        nodeoffset: c_int,
        name: *const c_char,
        lenp: *mut c_int,
    ) -> *const c_void;
    pub fn fdt_get_phandle(fdt: *const c_void, nodeoffset: c_int) -> u32;
    pub fn fdt_get_path(
        fdt: *const c_void,
        nodeoffset: c_int,
        buf: *mut c_char,
        buflen: c_int,
    ) -> c_int;
    pub fn fdt_node_offset_by_phandle(fdt: *const c_void, phandle: u32) -> c_int;
    pub fn fdt_node_check_compatible(
        fdt: *const c_void,
        nodeoffset: c_int,
        compatible: *const c_char,
    ) -> c_int;
}
