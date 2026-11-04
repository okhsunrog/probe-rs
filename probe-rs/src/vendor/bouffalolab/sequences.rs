//! BL70x debug memory access configuration.

use std::sync::Arc;

use crate::architecture::riscv::{
    communication_interface::{
        MemoryAccessConfig, MemoryAccessMethod, RiscvBusAccess, RiscvCommunicationInterface,
    },
    sequences::RiscvDebugSequence,
};

#[derive(Debug)]
pub(super) struct Bl70x;

impl Bl70x {
    pub(super) fn create() -> Arc<dyn RiscvDebugSequence> {
        Arc::new(Self)
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
    fn on_connect(&self, interface: &mut RiscvCommunicationInterface) -> Result<(), crate::Error> {
        configure_glb_access(interface.memory_access_config());
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
