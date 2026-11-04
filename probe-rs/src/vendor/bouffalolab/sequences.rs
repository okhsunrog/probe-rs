//! BL70x debug memory access configuration.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{Error, MemoryInterface, Session};

use crate::architecture::riscv::{
    communication_interface::{
        MemoryAccessConfig, MemoryAccessMethod, RiscvBusAccess, RiscvCommunicationInterface,
    },
    sequences::RiscvDebugSequence,
};

#[derive(Debug)]
pub(super) struct Bl70x {
    flash_reset_pending: AtomicBool,
}

impl Bl70x {
    pub(super) fn create() -> Arc<dyn RiscvDebugSequence> {
        Arc::new(Self {
            flash_reset_pending: AtomicBool::new(false),
        })
    }
}

fn configure_glb_access(config: &mut MemoryAccessConfig) {
    // On BL706, SBA reads of GLB GPIO registers can leave sbbusy stuck after
    // firmware clock initialization. CPU program-buffer reads work correctly.
    // Keep SBA for RAM and flash, including RTT, and restrict this workaround
    // to the GLB register block. Do not probe peripheral registers at attach.
    for width in [
        RiscvBusAccess::A8,
        RiscvBusAccess::A16,
        RiscvBusAccess::A32,
        RiscvBusAccess::A64,
        RiscvBusAccess::A128,
    ] {
        config.set_region_override(
            width,
            0x4000_0000..0x4000_1000,
            MemoryAccessMethod::ProgramBuffer,
        );
    }
}

impl RiscvDebugSequence for Bl70x {
    fn on_resume(&self, interface: &mut RiscvCommunicationInterface) -> Result<(), Error> {
        if self.flash_reset_pending.swap(false, Ordering::Relaxed) {
            // The stub first lets resumeack reach the probe, then resets the DM.
            // Do not issue RTT/DMI traffic until the reset and ROM boot finish.
            std::thread::sleep(Duration::from_millis(100));
            interface.enter_debug_mode()?;
            configure_glb_access(interface.memory_access_config());
        }
        Ok(())
    }

    fn prepare_running_on_flash(
        &self,
        interface: &mut RiscvCommunicationInterface,
    ) -> Result<(), Error> {
        // The ROM parking/wake path has already initialized its workspace.
        // Jumping back to the ROM entry did not boot flash. Instead resume a
        // short RAM stub that performs a real system reset without a wake flag.
        // Delay the reset so the probe observes resumeack before debug resets.
        const STUB: u64 = 0x2201_fff0;
        interface.write_32(
            STUB,
            &[
                0xfff6_0613, // addi a2, a2, -1
                0xfe06_1ee3, // bne a2, zero, -4
                0x00b5_2023, // sw a1, 0(a0)
                0x0000_006f, // jal zero, 0
            ],
        )?;
        interface.write_word_32(0x4000_f100, 0)?;
        interface.write_word_32(0x4000_f104, 0)?;
        let resets = interface.read_word_32(0x4000_0018)? & !7;
        interface.write_word_32(0x4000_0018, resets)?;
        interface.write_csr(0x100a, 0x4000_0018)?;
        interface.write_csr(0x100b, u64::from(resets | 6))?;
        interface.write_csr(0x100c, 160_000)?;
        interface.write_csr(0x7b1, STUB)?;
        self.flash_reset_pending.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn on_connect(&self, interface: &mut RiscvCommunicationInterface) -> Result<(), crate::Error> {
        configure_glb_access(interface.memory_access_config());
        Ok(())
    }

    fn reset_system_and_halt(
        &self,
        interface: &mut RiscvCommunicationInterface,
        timeout: Duration,
    ) -> Result<(), Error> {
        self.flash_reset_pending.store(false, Ordering::Relaxed);
        const PARK: u64 = 0x2201_fff0;
        const RSV0: u64 = 0x4000_f100;
        const RSV1: u64 = 0x4000_f104;
        interface.halt(timeout)?;
        let saved = [
            interface.read_word_32(PARK)?,
            interface.read_word_32(RSV0)?,
            interface.read_word_32(RSV1)?,
        ];
        // The ROM wake-up path parks in a temporary RAM loop before stock
        // firmware can initialize peripherals. Preserve the borrowed RAM word.
        let result = (|| {
            interface.write_word_32(PARK, 0x0000_006f)?; // jal zero, 0
            interface.write_word_32(RSV1, PARK as u32)?;
            interface.write_word_32(RSV0, 0x4e42_4845)?;

            // GLB_SW_System_Reset: select RC32M before latching undivided HCLK/BCLK.
            let root = interface.read_word_32(0x4000_f030)?;
            interface.write_word_32(0x4000_f030, root & !3)?;
            let clocks = interface.read_word_32(0x4000_0000)?;
            interface.write_word_32(0x4000_0000, clocks & 0xff00_00ff)?;
            interface.write_word_32(0x4000_0ffc, 1)?;
            interface.write_word_32(0x4000_0ffc, 0)?;
            let resets = interface.read_word_32(0x4000_0018)? & !7;
            interface.write_word_32(0x4000_0018, resets)?;
            // Reset can interrupt the program-buffer store's completion handshake.
            let reset_result = interface.write_word_32(0x4000_0018, resets | 6);
            std::thread::sleep(Duration::from_millis(2));
            interface.enter_debug_mode()?;
            configure_glb_access(interface.memory_access_config());
            interface.halt(timeout)?;
            let pc = interface.read_csr_progbuf(0x7b1)?;
            let wakeup = interface.read_word_32(RSV0)?;
            if pc != PARK || wakeup != 0x4e42_4857 {
                return Err(Error::Other(format!(
                    "BL70x system reset did not reach the ROM parking loop: PC={pc:#x}, wakeup={wakeup:#x}, reset={reset_result:?}"
                )));
            }
            interface.write_word_32(PARK, saved[0])?;
            interface.write_word_32(RSV0, saved[1])?;
            interface.write_word_32(RSV1, saved[2])?;
            let dcsr = interface.read_csr_progbuf(0x7b0)?;
            interface.write_csr(0x7b0, dcsr | (1 << 15))?;
            let mstatus = interface.read_csr_progbuf(0x300)?;
            interface.write_csr(0x300, mstatus & !8)?;
            // Flash boot replaces this with a reset stub; RAM boot uses its ELF entry.
            interface.write_csr(0x7b1, 0x2100_0000)?;
            Ok(())
        })();
        if result.is_err() && interface.halt(timeout).is_ok() {
            // Do not leave a stale ROM wake-up request after a failed reset.
            let _ = interface.write_word_32(PARK, saved[0]);
            let _ = interface.write_word_32(RSV0, saved[1]);
            let _ = interface.write_word_32(RSV1, saved[2]);
        }
        result
    }

    fn supports_ram_boot(&self) -> bool {
        true
    }

    fn prepare_running_on_ram(
        &self,
        session: &mut Session,
        entry: u64,
        core_id: usize,
    ) -> Result<(), Error> {
        if entry & 1 != 0 || !(0x2201_0000..0x2202_e000).contains(&entry) {
            return Err(Error::Other(format!("Invalid BL70x RAM entry {entry:#x}")));
        }
        let mut core = session.core(core_id)?;
        core.write_core_reg(core.program_counter(), entry)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glb_override_preserves_fast_ram_and_flash_access() {
        let mut config = MemoryAccessConfig::default();
        for width in [RiscvBusAccess::A8, RiscvBusAccess::A16, RiscvBusAccess::A32] {
            config.set_default_method(width, MemoryAccessMethod::SystemBus);
        }
        configure_glb_access(&mut config);
        for width in [RiscvBusAccess::A8, RiscvBusAccess::A16, RiscvBusAccess::A32] {
            for address in [0x4000_0000, 0x4000_0128, 0x4000_0190, 0x4000_0fff] {
                assert_eq!(
                    config.method(address, width),
                    MemoryAccessMethod::ProgramBuffer
                );
            }
            for address in [0x2201_0000, 0x2202_0000, 0x2300_0000, 0x4000_1000] {
                assert_eq!(config.method(address, width), MemoryAccessMethod::SystemBus);
            }
        }
    }
}
