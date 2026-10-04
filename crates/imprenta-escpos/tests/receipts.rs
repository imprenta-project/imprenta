use imprenta_escpos::render;

#[test]
fn native_purchase_receipt_uses_spanish_codepage_and_explicit_cut() {
    let result = render(
        r#"{"children":[
        {"type":"text","text":"España 9,50 €"},
        {"type":"feed","lines":3},
        {"type":"cut","mode":"partial"}
    ]}"#
        .as_bytes(),
    )
    .unwrap();
    assert!(result.escpos.starts_with(&[0x1b, 0x40]));
    assert!(result.escpos.contains(&0xa4));
    assert!(result.escpos.contains(&0xd5));
    assert!(result.escpos.windows(3).any(|w| w == [0x1d, 0x56, 1]));
    assert_eq!(result.tickets, 1);
    assert!(result.diagnostics.is_empty());
}

#[test]
fn control_characters_in_customer_data_are_rejected() {
    let result = render(br#"{"children":[{"type":"text","text":"name\u001b@"}]}"#);
    assert!(result.unwrap_err().to_string().contains("control"));
}

#[test]
fn invalid_json_is_an_error_and_not_a_panic() {
    assert!(render(b"{ invalid").is_err());
}

#[test]
fn products_wrap_under_their_own_column_without_moving_the_amount() {
    let result = render(
        br#"{"profile":{"columns":20},"children":[{"type":"row","columns":[
        {"text":"Extra virgin olive oil"},{"text":"12.50","width":6,"justification":"right"}
    ]}]}"#,
    )
    .unwrap();
    assert!(
        result
            .escpos
            .windows(b"Extra virgin   12.50\nolive oil           \n".len())
            .any(|w| w == b"Extra virgin   12.50\nolive oil           \n")
    );
}

#[test]
fn fixed_amount_column_can_refuse_overflow() {
    let result = render(
        br#"{"profile":{"columns":20},"children":[{"type":"row","columns":[
        {"text":"Product"},{"text":"12345.67","width":6,"overflow":"error"}
    ]}]}"#,
    );
    assert!(result.unwrap_err().to_string().contains("column text"));
}

#[test]
fn init_sets_printable_millimetres_margin_and_dpi() {
    let result = render(br#"{"children":[{"type":"init","width":51,"marginLeft":5,"dpi":203},{"type":"text","text":"Hello"}]}"#).unwrap();
    assert_eq!(result.profile.printable_width, 408);
    assert_eq!(result.profile.margin_left, 40);
    assert_eq!(result.profile.columns, 34);
    assert!(result.escpos.windows(4).any(|w| w == [0x1d, 0x4c, 40, 0]));
    assert!(result.escpos.windows(4).any(|w| w == [0x1d, 0x57, 152, 1]));
}

#[test]
fn init_cannot_reset_a_partially_composed_ticket() {
    let result = render(br#"{"children":[{"type":"text","text":"Hello"},{"type":"init"}]}"#);
    assert!(result.unwrap_err().to_string().contains("start"));
}

#[test]
fn formatted_text_and_inline_labels_follow_the_article_api() {
    let result = render(
        br#"{"children":[
        {"type":"text","text":"Total: ","lf":false},
        {"type":"text","text":"9.50","format":"%s EUR","prefix":"(","suffix":")"}
    ]}"#,
    )
    .unwrap();
    assert!(result.escpos.windows(6).any(|w| w == b"Total:"));
    assert!(result.escpos.windows(11).any(|w| w == b"(9.50 EUR)\n"));
    // No line feed between the two text runs; style commands may occur there.
    let first = result
        .escpos
        .windows(7)
        .position(|w| w == b"Total: ")
        .unwrap()
        + 7;
    let second = result
        .escpos
        .windows(5)
        .position(|w| w == b"(9.50")
        .unwrap();
    assert!(!result.escpos[first..second].contains(&10));
}

#[test]
fn inline_text_wraps_using_the_space_left_on_the_line() {
    let result = render(
        br#"{"profile":{"columns":8},"children":[
        {"type":"text","text":"123456","lf":false},
        {"type":"text","text":"abcd"}
    ]}"#,
    )
    .unwrap();
    assert!(result.escpos.windows(6).any(|w| w == b"ab\ncd\n"));
}

#[test]
fn combining_accents_are_normalized_before_native_encoding() {
    let result = render(r#"{"children":[{"type":"text","text":"ñ €"}]}"#.as_bytes()).unwrap();
    assert!(result.escpos.windows(3).any(|w| w == [0xa4, 32, 0xd5]));
    assert!(result.diagnostics.is_empty());
}

#[test]
fn unsupported_characters_are_diagnosed_once() {
    let result = render(r#"{"children":[{"type":"text","text":"🐈🐈"}]}"#.as_bytes()).unwrap();
    assert_eq!(result.diagnostics.len(), 1);
    assert!(result.diagnostics[0].contains("unsupported-character"));
    assert!(result.escpos.windows(3).any(|w| w == b"??\n"));
}

#[test]
fn underline_scale_and_strike_use_native_commands_and_do_not_leak() {
    let result = render(br#"{"children":[
        {"type":"text","text":"TOTAL","fontWidth":2,"fontHeight":3,"underline":"two-dot-thick","doubleStrike":true},
        {"type":"text","text":"Thanks"}
    ]}"#).unwrap();
    for command in [
        [0x1d, 0x21, 0x12],
        [0x1b, 0x2d, 2],
        [0x1b, 0x47, 1],
        [0x1d, 0x21, 0],
        [0x1b, 0x2d, 0],
        [0x1b, 0x47, 0],
    ] {
        assert!(
            result.escpos.windows(3).any(|w| w == command),
            "missing {command:?}"
        );
    }
}

#[test]
fn native_codes_are_emitted_without_rasterizing_the_ticket() {
    let result = render(
        br#"{"children":[
        {"type":"qrcode","value":"https://example.com","size":4},
        {"type":"barcode","value":"123456","barcodeType":"code128","hriPosition":"below-bar-code"},
        {"type":"pdf417","value":"receipt","errorCorrectionLevel":2}
    ]}"#,
    )
    .unwrap();
    assert!(result.escpos.windows(3).any(|w| w == [0x1d, 0x28, 0x6b]));
    assert!(result.escpos.windows(3).any(|w| w == [0x1d, 0x6b, 73]));
    assert!(!result.escpos.windows(3).any(|w| w == [0x1d, 0x76, 0x30]));
}

#[test]
fn invalid_codes_and_scales_are_refused() {
    for json in [
        r#"{"children":[{"type":"qrcode","value":"test","size":0}]}"#,
        r#"{"children":[{"type":"barcode","value":"abc","barcodeType":"ean13"}]}"#,
        r#"{"children":[{"type":"pdf417","value":"test","errorCorrectionLevel":9}]}"#,
        r#"{"children":[{"type":"text","text":"test","fontWidth":0}]}"#,
        r#"{"children":[{"type":"text","text":"test","fontHeight":9}]}"#,
        r#"{"profile":{"columns":-1},"children":[]}"#,
    ] {
        assert!(render(json.as_bytes()).is_err(), "accepted {json}");
    }
}

#[test]
fn raw_hex_is_explicit_and_invalid_hex_is_refused() {
    let result = render(br#"{"children":[{"type":"bytes","hex":"1B 32"}]}"#).unwrap();
    assert!(result.escpos.ends_with(&[0x1b, 0x32]));
    assert!(result.diagnostics[0].contains("raw-bytes"));
    assert!(render(br#"{"children":[{"type":"bytes","hex":"1B 3"}]}"#).is_err());
}

#[test]
fn images_are_decoded_in_rust_and_encoded_as_monochrome_blocks() {
    let image = imprenta_escpos::Image {
        name: "logo".into(),
        data: include_bytes!("../../imprenta-core/tests/images/logo.png").to_vec(),
    };
    let result = imprenta_escpos::render_with_images(
        br#"{"children":[{"type":"image","src":"logo","width":96,"filter":"dither"}]}"#,
        &[image],
    )
    .unwrap();
    assert!(result.escpos.windows(3).any(|w| w == [0x1d, 0x76, 0x30]));
    assert_eq!(result.tickets, 1);
    assert!(
        imprenta_escpos::render(br#"{"children":[{"type":"image","src":"missing","width":96}]}"#)
            .unwrap_err()
            .to_string()
            .contains("missing")
    );
}

#[test]
fn unknown_attributes_are_not_silently_ignored_in_json() {
    assert!(render(br#"{"children":[{"type":"text","text":"hello","fontWidht":4}]}"#).is_err());
}

#[test]
fn multiple_cuts_count_paper_pieces() {
    let result = render(br#"{"children":[{"type":"text","text":"one"},{"type":"cut"},{"type":"text","text":"two"},{"type":"cut"}]}"#).unwrap();
    assert_eq!(result.tickets, 2);
}
