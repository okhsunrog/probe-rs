# BL706 flash algorithm

Standalone RV32IMC code executing entirely in RAM. The supported profile is
embedded Macronix JEDEC `C2 20 14`, 1 MiB, 256-byte pages, 4 KiB sectors.
Other IDs or external flash pad configurations are rejected. No chip erase,
eFuse, protection unlock, or nonvolatile status-register writes are implemented.

`Init` selects an 8 MHz SPI clock after the BL70x RC32M debug reset, routes SF2
to the embedded pads, and recovers from continuous-read/reset/power-down modes.
`ReadFlash` uses physical flash offsets independent of the previous XIP mapping.
`UnInit` leaves XIP disabled; a system reset boots the ROM and restores XIP.
Callers must not resume a saved flash PC after taking controller ownership.

The SF SRAM clock mux and command trigger cross clock domains. Fences plus
settling delays are required before testing BUSY and reading the first SRAM word.
On Pinecil V2, omitting these delays returned stale data for JEDEC and word zero.

Sources: Bouffalo BL702 SDK `bl702_sf_ctrl.c`, `bl702_sflash.c`,
`bl702_sf_cfg.c` and their register headers. Host tests check command/range
boundaries. Hardware read validation compares physical offsets to a full backup.

Rebuild from this directory (the tested compiler is `nightly-2026-10-14`):

```sh
cargo +nightly-2026-10-14 build --release --target riscv32imc-unknown-none-elf
cargo +nightly-2026-10-14 test
cargo +nightly-2026-10-14 clippy --tests -- -D warnings
uv run --no-project regenerate.py
```

Build `target-gen` in the repository root first. The regeneration script retains
the variant definitions instead of round-tripping them through target-gen's
YAML compactor. Rebuilding must not introduce an unexplained blob change.
