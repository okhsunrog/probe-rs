OUTPUT_ARCH(riscv)
ENTRY(Init)
/* Leave room for probe-rs's architecture-specific return trampoline. */
MEMORY { RAM (rwx) : ORIGIN = 0x22010010, LENGTH = 16K - 16 }
SECTIONS {
    PrgCode : ALIGN(4) { KEEP(*(.text .text.*)) *(.rodata .rodata.* .srodata .srodata.*) . = ALIGN(4); } > RAM
    PrgData : ALIGN(4) { *(.data .data.* .sdata .sdata.*) *(.bss .bss.* .sbss .sbss.*) . = ALIGN(4); } > RAM
    DevDscr : ALIGN(4) { KEEP(*(.FlashDevice)) } > RAM
    /DISCARD/ : { *(.eh_frame .eh_frame_hdr) }
}
