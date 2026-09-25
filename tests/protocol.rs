use omen_rs::protocol::secu::build_secu_raw;

#[test]
fn secu_header_is_correct() {
    let data = [0x00, 0x00, 0x00, 0x00];
    let buf = build_secu_raw(0x0002_0008, 0x28, &data);

    // 签名 "SECU"
    assert_eq!(&buf[0..4], b"SECU");
    // Command 小端 0x00020008
    assert_eq!(&buf[4..8], &[0x08, 0x00, 0x02, 0x00]);
    // CommandType 小端 0x28
    assert_eq!(&buf[8..12], &[0x28, 0x00, 0x00, 0x00]);
    // Size = 4
    assert_eq!(&buf[12..16], &[0x04, 0x00, 0x00, 0x00]);
    // data
    assert_eq!(&buf[16..20], &[0x00, 0x00, 0x00, 0x00]);
}
