//! Read-only BL706 flash-algorithm validation against a physical flash backup.
use anyhow::{Result, ensure};
use probe_rs::{
    Permissions,
    flashing::{FlashProgress, Flasher},
    probe::{WireProtocol, list::Lister},
};

fn main() -> Result<()> {
    let original = std::fs::read(std::env::args().nth(1).expect("reference image path"))?;
    ensure!(original.len() == 1024 * 1024, "expected full 1 MiB image");
    let probes = Lister::new().list_all();
    let info = probes
        .iter()
        .find(|p| p.vendor_id == 0x1209 && p.product_id == 0x4853)
        .expect("Grapple Probe Pico");
    let mut probe = info.open()?;
    probe.select_protocol(WireProtocol::Jtag)?;
    probe.set_speed(1000)?;
    let mut session = probe.attach("BL706", Permissions::default())?;
    let raw = session.target().flash_algorithms[0].clone();
    let mut flasher = Flasher::new(session.target(), 0, &raw)?;
    let mut progress = FlashProgress::empty();
    flasher.run_verify(&mut session, &mut progress, |active, _| {
        let id = active.call_vendor_function("JedecId", [None; 4])?;
        println!("JEDEC: {id:06x}");
        assert_eq!(id, 0x1420c2);
        for offset in [0, 0x1000, 0x2000, 0x7ff00, 0xfff00] {
            let mut bytes = [0; 256];
            active.read_flash(0x23000000 + offset as u64, &mut bytes)?;
            assert_eq!(bytes, original[offset..offset + 256]);
            println!("physical offset {offset:#x}: matches backup");
        }
        if let Some(path) = std::env::args().nth(2) {
            let mut bytes = vec![0; 0x5000];
            active.read_flash(0x23000000, &mut bytes)?;
            std::fs::write(path, &bytes).expect("save readback");
            assert_eq!(bytes, original[..bytes.len()]);
        }
        Ok(())
    })?;
    Ok(())
}
