use ldb::spec::{deserialize, serialize, Document, LdbValue};

#[test]
fn example_edad_activo_byte_exact() {
    let mut doc = Document::new();
    doc.insert("edad", LdbValue::Int32(21));
    doc.insert("activo", LdbValue::Boolean(true));

    let bytes = serialize(&doc);

    let expected = vec![
        0x4C, 0x44, 0x42, 0x00,
        0x00, 0x01,
        0x00, 0x00,
        0x24, 0x00, 0x00, 0x00,
        0x02, 0x00, 0x00, 0x00,
        0x05, 
        0x06,
        0x61, 0x63, 0x74, 0x69, 0x76, 0x6F,
        0x01,
        0x01,
        0x04,
        0x65, 0x64, 0x61, 0x64,
        0x15, 0x00, 0x00, 0x00,
        0xFF,
    ];

    assert_eq!(bytes, expected, "el hex dump no coincide con la especificación");
}

#[test]
fn roundtrip_all_basic_types() {
    let mut doc = Document::new();
    doc.insert("entero32", LdbValue::Int32(-42));
    doc.insert("entero64", LdbValue::Int64(9_000_000_000_000));
    doc.insert("decimal", LdbValue::Float64(3.14159));
    doc.insert("nombre", LdbValue::String("María".to_string()));
    doc.insert("activo", LdbValue::Boolean(false));
    doc.insert("nulo", LdbValue::Null);

    let mut nested = Document::new();
    nested.insert("x", LdbValue::Int32(1));
    nested.insert("y", LdbValue::Int32(2));
    doc.insert("punto", LdbValue::SubDocument(nested));

    let bytes = serialize(&doc);
    let parsed = deserialize(&bytes).expect("deserialización debe funcionar");
    assert_eq!(parsed, doc);
}

#[test]
fn invalid_magic_fails() {
    let mut bad = vec![0x00; 16];
    bad[0..4].copy_from_slice(b"BAD\0");
    assert!(deserialize(&bad).is_err());
}

#[test]
fn invalid_boolean_fails() {
    let mut doc = Document::new();
    doc.insert("x", LdbValue::Boolean(true));
    let mut bytes = serialize(&doc);
    let last = bytes.len() - 2;
    bytes[last] = 0x02;
    assert!(deserialize(&bytes).is_err());
}
