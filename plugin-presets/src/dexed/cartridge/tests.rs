use super::*;

fn cartridge_bytes(names: &[(usize, &str)]) -> Vec<u8> {
    test_cartridge_bytes(names)
}

#[test]
fn parses_program_names_of_a_valid_cartridge() {
    let bytes = cartridge_bytes(&[(0, "Say Again."), (31, "LAST      ")]);

    let cartridge = parse_dx7_cartridge(bytes.clone()).unwrap();

    assert_eq!(cartridge.program_names().len(), DX7_PROGRAMS_PER_CARTRIDGE);
    assert_eq!(cartridge.program_names()[0], "Say Again.");
    assert_eq!(cartridge.program_names()[31], "LAST");
    assert_eq!(cartridge.sysex_bytes(), bytes.as_slice());
}

#[test]
fn name_bytes_are_sanitized_for_use_inside_a_patch_path() {
    let mut bytes = cartridge_bytes(&[]);
    let start = HEADER_LEN + NAME_OFFSET_IN_VOICE;
    bytes[start..start + NAME_LEN].copy_from_slice(b"A\x00B/C\\D\x7f\x01\x02");
    bytes[CHECKSUM_OFFSET] = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);

    let cartridge = parse_dx7_cartridge(bytes).unwrap();

    assert_eq!(cartridge.program_names()[0], "A B C D");
}

#[test]
fn blank_name_falls_back_to_a_placeholder() {
    let bytes = cartridge_bytes(&[(0, "          ")]);

    let cartridge = parse_dx7_cartridge(bytes).unwrap();

    assert_eq!(cartridge.program_names()[0], UNNAMED_PROGRAM);
}

#[test]
fn rejects_wrong_length() {
    let error = parse_dx7_cartridge(vec![0u8; 163]).unwrap_err().to_string();

    assert!(error.contains("4104"), "{error}");
    assert!(error.contains("163"), "{error}");
}

#[test]
fn rejects_single_voice_dump_format() {
    let mut bytes = cartridge_bytes(&[]);
    bytes[3] = 0x00;
    bytes[CHECKSUM_OFFSET] = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);

    let error = parse_dx7_cartridge(bytes).unwrap_err().to_string();

    assert!(error.contains("32-voice"), "{error}");
}

#[test]
fn rejects_non_yamaha_manufacturer() {
    let mut bytes = cartridge_bytes(&[]);
    bytes[1] = 0x41;
    bytes[CHECKSUM_OFFSET] = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);

    let error = parse_dx7_cartridge(bytes).unwrap_err().to_string();

    assert!(error.contains("Yamaha"), "{error}");
}

#[test]
fn rejects_broken_checksum() {
    let mut bytes = cartridge_bytes(&[]);
    bytes[CHECKSUM_OFFSET] ^= 0x01;

    let error = parse_dx7_cartridge(bytes).unwrap_err().to_string();

    assert!(error.contains("checksum"), "{error}");
}

#[test]
fn accepts_any_midi_channel_in_the_sub_status_byte() {
    let mut bytes = cartridge_bytes(&[(0, "CH15")]);
    bytes[2] = 0x0F;
    bytes[CHECKSUM_OFFSET] = checksum(&bytes[HEADER_LEN..CHECKSUM_OFFSET]);

    let cartridge = parse_dx7_cartridge(bytes).unwrap();

    assert_eq!(cartridge.program_names()[0], "CH15");
}

/// checksum の式は「データ部 7bit 総和の 2 の補数」。手元の実物 33 件がこの式で合う。
#[test]
fn checksum_is_the_twos_complement_of_the_seven_bit_sum() {
    assert_eq!(checksum(&[0x00]), 0x00);
    assert_eq!(checksum(&[0x01]), 0x7F);
    assert_eq!(checksum(&[0x40, 0x40]), 0x00);
    assert_eq!(checksum(&[0x7F]), 0x01);
}
