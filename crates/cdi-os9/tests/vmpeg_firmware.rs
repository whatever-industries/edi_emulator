// SPDX-License-Identifier: GPL-3.0-or-later

use cdi_os9::{parse_csd_records, scan_modules, CsdParseError, Module, ModuleType};

const SYSTEM_FIRMWARE: &[u8] = include_bytes!("../../../firmware/cdi220b.rom");
const VMPEG_FIRMWARE: &[u8] = include_bytes!("../../../firmware/vmpega.rom");
const IMPEG_FIRMWARE: &[u8] = include_bytes!("../../../firmware/impega.rom");

fn first_module(image: &[u8], name: &str) -> Module {
    scan_modules(image)
        .into_iter()
        .find(|module| module.name == name)
        .unwrap_or_else(|| panic!("missing {name} module"))
}

#[test]
fn bundled_system_firmware_exposes_native_csd_records() {
    let csd = first_module(SYSTEM_FIRMWARE, "csd_220");
    assert!(csd.crc_ok);
    let records = parse_csd_records(SYSTEM_FIRMWARE, &csd).expect("valid system CSD records");

    assert_eq!(records[0].device_class, 0);
    assert_eq!(records[0].name, "sys");
    assert_eq!(records[0].fields, ["RV=\"1.1\""]);
    assert!(records
        .iter()
        .any(|record| record.device_class == 3 && record.name == "/vid"));
    assert!(records
        .iter()
        .any(|record| record.device_class == 11 && record.name == "/pck2"));
}

#[test]
fn bundled_vmpeg_firmware_exposes_native_devices_and_memory_descriptors() {
    let modules = scan_modules(VMPEG_FIRMWARE);
    let first_half: Vec<_> = modules
        .iter()
        .filter(|module| module.offset < 128 * 1024)
        .collect();

    for name in ["mv", "ma"] {
        let descriptor = first_half
            .iter()
            .find(|module| module.name == name)
            .unwrap_or_else(|| panic!("missing /{name} device descriptor"));
        assert_eq!(descriptor.mod_type, ModuleType::Descriptor);
        assert!(descriptor.crc_ok);
    }

    let csd = first_half
        .iter()
        .find(|module| module.name == "csd_fmvvm")
        .expect("missing VMPEG CSD module");
    assert!(csd.crc_ok);
    let records = parse_csd_records(VMPEG_FIRMWARE, csd).expect("valid VMPEG CSD records");

    assert_eq!(records.len(), 4);
    assert_eq!(records[0].device_class, 90);
    assert_eq!(records[0].name, "/mv");
    assert_eq!(records[0].fields, ["RV=\"CTP2\""]);
    assert_eq!(records[1].device_class, 91);
    assert_eq!(records[1].name, "/ma");
    assert!(records[1].fields.is_empty());

    // Philips TN 101 defines SZ in Mbit, with a default of four. RAM00
    // therefore advertises 1 MiB, while RAM01 defaults to 512 KiB and assigns
    // MPEG memory color $90. Priority is intentionally not asserted here:
    // TN 101 says it exists only in OS-9's live system structures.
    assert_eq!(records[2].device_class, 30);
    assert_eq!(records[2].name, "RAM00");
    assert_eq!(records[2].fields, ["SZ#8", "AT#150"]);
    assert_eq!(records[3].device_class, 30);
    assert_eq!(records[3].name, "RAM01");
    assert_eq!(records[3].fields, ["CO#90", "AT#700"]);
}

#[test]
fn bundled_impeg_firmware_uses_the_same_device_classes_with_ctp3_drivers() {
    let csd = first_module(IMPEG_FIRMWARE, "csd_fmvimpeg");
    assert!(csd.crc_ok);
    let records = parse_csd_records(IMPEG_FIRMWARE, &csd).expect("valid IMPEG CSD records");

    assert_eq!(records.len(), 4);
    assert_eq!(records[0].device_class, 90);
    assert_eq!(records[0].name, "/mv");
    assert_eq!(records[0].fields, ["RV=\"CTP3\""]);
    assert_eq!(records[1].device_class, 91);
    assert_eq!(records[1].name, "/ma");
    assert_eq!(records[1].fields, ["RV=\"CTP3\""]);
    assert_eq!(records[2].name, "RAM00");
    assert_eq!(records[3].name, "RAM01");
}

#[test]
fn csd_data_must_end_before_the_module_crc() {
    let csd = first_module(VMPEG_FIRMWARE, "csd_fmvvm");
    let mut malformed = VMPEG_FIRMWARE.to_vec();
    let module_start = csd.offset as usize;
    let module_end = module_start + csd.size as usize;
    for byte in &mut malformed[module_start + 0x34..module_end] {
        if *byte == 0 {
            *byte = b'X';
        }
    }

    assert_eq!(
        parse_csd_records(&malformed, &csd),
        Err(CsdParseError::MissingTerminator)
    );
}
