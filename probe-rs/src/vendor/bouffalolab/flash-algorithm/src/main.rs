//! Standalone BL706 SPI 1-1-1 flash algorithm; code and data execute from RAM.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#![allow(non_snake_case)]
use core::ptr::{read_volatile, write_volatile};
const BASE: u32 = 0x2300_0000;
const SIZE: u32 = 1024 * 1024;
const PAGE: usize = 256;
const SECTOR: u32 = 4096;
const CTRL: usize = 0x4000_b000;
const BUFFER: usize = 0x4000_b700;
fn read(address: usize) -> u32 {
    unsafe { read_volatile(address as *const u32) }
}
fn write(address: usize, value: u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}
fn modify(address: usize, mask: u32, value: u32) {
    write(address, (read(address) & !mask) | value);
}
fn fence() {
    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("fence iorw, iorw");
    }
}
fn delay() {
    for _ in 0..3200 {
        core::hint::spin_loop();
    }
}
fn settle() {
    // The SF SRAM clock mux and trigger cross clock domains. An MMIO fence
    // orders bus transactions but does not wait for these signals to settle.
    for _ in 0..32 {
        core::hint::spin_loop();
    }
}
fn idle() -> Result<(), u32> {
    for _ in 0..100_000 {
        if read(CTRL + 8) & 1 == 0 {
            return Ok(());
        }
    }
    Err(2)
}
fn encoding(
    opcode: u8,
    address: Option<u32>,
    length: usize,
    output: bool,
) -> Result<(u32, u32), u32> {
    if length > PAGE || address.is_some_and(|a| a >= 1 << 24) {
        return Err(3);
    }
    let mut flags = 1 << 27;
    if address.is_some() {
        flags |= (1 << 26) | (2 << 17);
    }
    if length != 0 {
        flags |= (1 << 24) | (((length - 1) as u32) << 2);
    }
    if output {
        flags |= 1 << 23;
    }
    Ok(((u32::from(opcode) << 24) | address.unwrap_or(0), flags))
}
fn command(opcode: u8, address: Option<u32>, output: &[u8], input: &mut [u8]) -> Result<(), u32> {
    if !output.is_empty() && !input.is_empty() {
        return Err(3);
    }
    let (word, flags) = encoding(
        opcode,
        address,
        output.len().max(input.len()),
        !output.is_empty(),
    )?;
    idle()?;
    write(CTRL + 8, flags);
    modify(CTRL, 1 << 5, 0);
    fence();
    settle();
    for (index, chunk) in output.chunks(4).enumerate() {
        let mut bytes = [0; 4];
        bytes[..chunk.len()].copy_from_slice(chunk);
        write(BUFFER + index * 4, u32::from_le_bytes(bytes));
    }
    write(CTRL + 12, word);
    write(CTRL + 16, 0);
    modify(CTRL, 1 << 5, 1 << 5);
    fence();
    settle();
    write(CTRL + 8, flags | 2);
    fence();
    settle();
    idle()?;
    modify(CTRL, 1 << 5, 0);
    fence();
    settle();
    for (index, chunk) in input.chunks_mut(4).enumerate() {
        let bytes = read(BUFFER + index * 4).to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    Ok(())
}
fn status() -> Result<u8, u32> {
    let mut value = [0];
    command(5, None, &[], &mut value)?;
    Ok(value[0])
}
fn cycles() -> u32 {
    #[cfg(target_arch = "riscv32")]
    {
        let value: u32;
        unsafe { core::arch::asm!("csrr {}, mcycle", out(reg) value) };
        value
    }
    #[cfg(not(target_arch = "riscv32"))]
    0
}
fn ready(timeout_ms: u32) -> Result<(), u32> {
    // The debug reset sequence selects undivided RC32M before this algorithm.
    let start = cycles();
    for _ in 0..2_000_000 {
        if status()? & 1 == 0 {
            return Ok(());
        }
        if cycles().wrapping_sub(start) >= timeout_ms * 32_000 {
            break;
        }
    }
    Err(4)
}
fn write_enable() -> Result<(), u32> {
    ready(20)?;
    if status()? & 0x3c != 0 {
        return Err(5);
    }
    command(6, None, &[], &mut [])?;
    if status()? & 2 == 0 {
        return Err(6);
    }
    Ok(())
}
fn offset(address: u32, length: usize) -> Result<u32, u32> {
    let offset = address.checked_sub(BASE).ok_or(3u32)?;
    if offset
        .checked_add(length as u32)
        .is_none_or(|end| end > SIZE)
    {
        return Err(3);
    }
    Ok(offset)
}
fn buffer_valid(pointer: usize, length: usize) -> bool {
    pointer >= 0x2201_0000
        && pointer
            .checked_add(length)
            .is_some_and(|end| end <= 0x2202_e000)
}
#[unsafe(no_mangle)]
pub extern "C" fn VendorFunc_JedecId() -> u32 {
    let mut id = [0; 3];
    if command(0x9f, None, &[], &mut id).is_err() {
        return 0;
    }
    u32::from_le_bytes([id[0], id[1], id[2], 0])
}
#[unsafe(no_mangle)]
pub extern "C" fn Init(_address: u32, _clock: u32, _operation: u32) -> u32 {
    // SF_CTRL gate, nominal XCLK/4 after the BL70x reset sequence.
    modify(0x4000_0024, 1 << 11, 1 << 11);
    modify(0x4000_0008, 1 << 11, 0);
    modify(0x4000_0008, 0xff << 8, (3 << 8) | (1 << 11) | (1 << 14));
    modify(CTRL + 4, (1 << 28) | (1 << 30), (1 << 29) | (1 << 31));
    // Embedded flash is routed through SF2; no board GPIO configuration.
    // Read-only package/flash shadow eFuses: BL706 with embedded 1 MiB.
    let package = read(0x4000_7074);
    if package >> 30 != 2 || (package >> 26) & 7 != 2 {
        return 7;
    }
    modify(0x4000_0088, 0x3f, 0x3f);
    // GPIO24 is SF2 DATA1/MISO. Its original-pad input gate also controls
    // reads from embedded flash, even when the logical GPIO uses an SF/PSRAM
    // pad. Firmware that clears IE before selecting those pads can leave
    // JEDEC reads stuck at zero across a debug reset. Restore only IE;
    // preserve GPIO24's mux/electrical settings and neighbouring GPIO25.
    modify(0x4000_0130, 1, 1);
    modify(CTRL + 0x70, 3 | (1 << 30), 1);
    modify(CTRL, (1 << 3) | (1 << 5), 1 << 3);
    // Four FF bytes also terminate a pending continuous-read transaction.
    if let Err(error) = command(0xff, Some(0xff_ffff), &[], &mut []) {
        return error;
    }
    // Volatile mode recovery only; no nonvolatile status-register writes.
    for opcode in [0x66, 0x99, 0xab] {
        if let Err(error) = command(opcode, None, &[], &mut []) {
            return error;
        }
        delay();
    }
    let id = VendorFunc_JedecId();
    if id != 0x14_20_c2 {
        return 7;
    }
    ready(20).err().unwrap_or(0)
}
#[unsafe(no_mangle)]
pub extern "C" fn UnInit(_operation: u32) -> u32 {
    0
}
#[unsafe(no_mangle)]
pub extern "C" fn EraseSector(address: u32) -> u32 {
    let result = (|| {
        let address = offset(address, SECTOR as usize)?;
        if address % SECTOR != 0 {
            return Err(3);
        }
        write_enable()?;
        command(0x20, Some(address), &[], &mut [])?;
        ready(2500)
    })();
    result.err().unwrap_or(0)
}
#[unsafe(no_mangle)]
/// # Safety
/// The caller must provide a readable RAM buffer of `length` bytes.
pub unsafe extern "C" fn ProgramPage(address: u32, length: u32, pointer: *const u8) -> u32 {
    let length = length as usize;
    if length == 0 || !buffer_valid(pointer as usize, length) {
        return 3;
    }
    let result = (|| {
        let address = offset(address, length)?;
        if length > PAGE || address as usize % PAGE + length > PAGE {
            return Err(3);
        }
        let data = unsafe { core::slice::from_raw_parts(pointer, length) };
        write_enable()?;
        command(2, Some(address), data, &mut [])?;
        ready(80)
    })();
    result.err().unwrap_or(0)
}
#[unsafe(no_mangle)]
/// # Safety
/// The caller must provide an exclusive writable RAM buffer of `length` bytes.
pub unsafe extern "C" fn ReadFlash(address: u32, length: u32, pointer: *mut u8) -> u32 {
    let length = length as usize;
    if !buffer_valid(pointer as usize, length) {
        return 3;
    }
    let result = (|| {
        let address = offset(address, length)?;
        let data = unsafe { core::slice::from_raw_parts_mut(pointer, length) };
        for (index, chunk) in data.chunks_mut(PAGE).enumerate() {
            command(3, Some(address + (index * PAGE) as u32), &[], chunk)?;
        }
        Ok(())
    })();
    result.err().unwrap_or(0)
}
#[repr(C)]
pub struct Device {
    version: u16,
    name: [u8; 128],
    kind: u16,
    address: u32,
    size: u32,
    page: u32,
    reserved: u32,
    erased: u8,
    padding: [u8; 3],
    program_ms: u32,
    erase_ms: u32,
    sectors: [(u32, u32); 2],
}
const fn device_name() -> [u8; 128] {
    let mut name = [0; 128];
    let text = b"BL706 Macronix 1 MiB SPI flash";
    let mut i = 0;
    while i < text.len() {
        name[i] = text[i];
        i += 1;
    }
    name
}
#[used]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".FlashDevice")]
pub static FlashDevice: Device = Device {
    version: 0x101,
    name: device_name(),
    kind: 1,
    address: BASE,
    size: SIZE,
    page: PAGE as u32,
    reserved: 0,
    erased: 0xff,
    padding: [0; 3],
    program_ms: 100,
    erase_ms: 3000,
    sectors: [(SECTOR, 0), (u32::MAX, u32::MAX)],
};
#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_and_range_boundaries() {
        assert_eq!(
            encoding(0x9f, None, 3, false).unwrap(),
            (0x9f00_0000, (1 << 27) | (1 << 24) | 8)
        );
        assert!(encoding(3, Some(1 << 24), 1, false).is_err());
        assert!(encoding(2, Some(0), PAGE + 1, true).is_err());
        assert_eq!(offset(BASE + SIZE - 256, 256), Ok(SIZE - 256));
        assert!(offset(BASE - 1, 1).is_err());
        assert!(offset(BASE + SIZE - 1, 2).is_err());
        assert!(!buffer_valid(0x2300_0000, 16));
        assert!(!buffer_valid(usize::MAX - 1, 16));
    }
}
