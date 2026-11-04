//! Bouffalo Lab vendor support.

use probe_rs_target::Chip;

use crate::{
    Error, MemoryInterface,
    architecture::riscv::communication_interface::RiscvCommunicationInterface,
    config::{DebugSequence, Registry},
    vendor::Vendor,
};

mod sequences;

/// Bouffalo Lab.
#[derive(docsplay::Display)]
pub struct BouffaloLab;

impl Vendor for BouffaloLab {
    fn try_create_debug_sequence(&self, chip: &Chip) -> Option<DebugSequence> {
        matches!(chip.name.as_str(), "BL702" | "BL704" | "BL706")
            .then(|| DebugSequence::Riscv(sequences::Bl70x::create()))
    }

    fn try_detect_riscv_chip(
        &self,
        _registry: &Registry,
        probe: &mut RiscvCommunicationInterface,
        idcode: u32,
    ) -> Result<Option<String>, Error> {
        // BL702, BL704 and BL706 share their TAP ID; package information
        // distinguishes them. Do not access this register on other devices.
        if idcode != 0x2000_0e05 {
            return Ok(None);
        }

        // EF_DATA_0_EF_KEY_SLOT_5_W2, pkg_info[31:30]. Read the shadow
        // register only; do not reload or program eFuses during detection.
        // Vendor SDK: bl702_ef_ctrl.h / EF_Ctrl_Read_Device_Info().
        let device_info = probe.read_word_32(0x4000_7074)?;
        Ok(chip_from_package(device_info).map(str::to_owned))
    }
}

fn chip_from_package(device_info: u32) -> Option<&'static str> {
    match device_info >> 30 {
        0 => Some("BL702"), // QFN32
        1 => Some("BL704"), // QFN40
        2 => Some("BL706"), // QFN48
        _ => None,          // Reserved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_packages_without_using_flash_configuration_bits() {
        for (package, expected) in [(0, "BL702"), (1, "BL704"), (2, "BL706")] {
            let name = chip_from_package((package << 30) | 0x3fff_ffff).unwrap();
            assert_eq!(name, expected);
            let target = Registry::from_builtin_families()
                .get_target_by_name(name)
                .unwrap();
            assert_eq!(target.cores.len(), 1);
            if expected == "BL706" {
                assert_eq!(target.flash_algorithms.len(), 1);
                let raw = &target.flash_algorithms[0];
                assert_eq!(raw.flash_properties.address_range, 0x2300_0000..0x2310_0000);
                assert!(raw.pc_read.is_some());
                assert!(raw.pc_erase_all.is_none());
                crate::flashing::FlashAlgorithm::assemble_from_raw_with_core(raw, "main", &target)
                    .unwrap();
            } else {
                assert!(target.flash_algorithms.is_empty());
            }
        }
        assert_eq!(chip_from_package(u32::MAX), None);
    }
}
