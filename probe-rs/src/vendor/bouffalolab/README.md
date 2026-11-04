# BL70x JTAG support

BL702/BL704/BL706 share TAP ID `0x20000e05`. Package shadow eFuses identify
the variant. Detection does not reload or program eFuses.

## Reset and RAM boot

The BL706 tested on Pinecil V2 does not acknowledge generic RISC-V hart/NDM
reset. The sequence uses the SDK's `GLB_SW_System_Reset` flow: switch to RC32M,
latch undivided HCLK/BCLK, then assert system and CPU reset in GLB.

Before reset it borrows one TCM word at `0x2201fff0` for a parking loop. The ROM
HBN wake-up request (`HBN_RSV0=0x4e424845`, `HBN_RSV1=entry`) directs startup
there before the installed flash application can initialize peripherals.
After reconnect/halt both the PC and ROM's completed wake-up flag `0x4e424857`
are checked. The borrowed word and retention registers are restored. DPC is
set to the ROM entry for normal reset/resume; RAM boot replaces it with the
ELF entry point. Entry code must initialize its own SP/GP, .bss and interrupts.

The ELF loader only enables RISC-V RAM boot for sequences that opt in. The
entry must be instruction-aligned, in RAM and backed by loaded data. No flash
algorithm is needed for RAM images.

```sh
probe-rs run --chip BL706 --protocol jtag --speed 1000 \
  --probe 1209:4853 path/to/ram-firmware.elf
```

GLB uses CPU program-buffer access. RAM/flash retain fast System Bus access.
Source references are Bouffalo Lab's BL702 `bl702_glb.c`, `bl702_hbn.h`, and
the BLISP BL70x RAM-loader wake-up sequence. No firmware flash or eFuse
programming is part of reset/RAM boot.

Hardware validation: standalone Grapple Probe Pico, Pinecil V2 BL706, 1 MHz
TCK; 20 consecutive reset/load/RTT cycles each produced a fresh startup and
timer ticks from zero, without USB reconnection or pressing a target button.
BL702/BL704 share this sequence but have not been separately tested.
