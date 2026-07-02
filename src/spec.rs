use std::collections::BTreeMap;
use std::fmt;

pub const MAGIC: &[u8; 4] = b"LDB\0";

pub const END_OF_DOCUMENT: u8 = 0xFF;

pub mod tags {
    pub const INT32: u8 = 0x01;
    pub const INT64: u8 = 0x02;
    pub const FLOAT64: u8 = 0x03;
    pub const STRING: u8 = 0x04;
    pub const BOOLEAN: u8 = 0x05;
    pub const SUB_DOCUMENT: u8 = 0x06;
    pub const NULL: u8 = 0x07;
    pub const END_OF_DOCUMENT: u8 = 0xFF;
}

#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum LdbValue {
    Int32(i32),
    Int64(i64),
    Float64(f64),
    String(String),
    Boolean(bool),
    SubDocument(Document),
    Null,
}

impl LdbValue {
    pub fn tag(&self) -> u8 {
        match self {
            LdbValue::Int32(_) => tags::INT32,
            LdbValue::Int64(_) => tags::INT64,
            LdbValue::Float64(_) => tags::FLOAT64,
            LdbValue::String(_) => tags::STRING,
            LdbValue::Boolean(_) => tags::BOOLEAN,
            LdbValue::SubDocument(_) => tags::SUB_DOCUMENT,
            LdbValue::Null => tags::NULL,
        }
    }

    pub fn discriminant(&self) -> u8 {
        unsafe { *(self as *const Self as *const u8) }
    }
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Default)]
pub struct Document {
    pub fields: BTreeMap<String, LdbValue>,
}

impl Document {
    pub fn new() -> Self {
        Self {
            fields: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, value: LdbValue) {
        self.fields.insert(key.into(), value);
    }
}

#[derive(Debug, PartialEq)]
pub enum LdbError {
    InvalidMagic,
    InvalidVersion,
    InvalidDocSize,
    InvalidFieldCount,
    InvalidTypeTag(u8),
    InvalidBoolean(u8),
    InvalidKeyLength,
    InvalidUtf8,
    UnexpectedEndOfDocument,
    MissingEndOfDocument,
    ExtraBytesAfterDocument,
    SubDocumentLengthMismatch,
}

impl fmt::Display for LdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LdbError::InvalidMagic => write!(f, "magic bytes inválidos"),
            LdbError::InvalidVersion => write!(f, "versión no soportada"),
            LdbError::InvalidDocSize => write!(f, "DocSize no coincide"),
            LdbError::InvalidFieldCount => write!(f, "FieldCount no coincide"),
            LdbError::InvalidTypeTag(t) => write!(f, "type tag inválido: 0x{:02X}", t),
            LdbError::InvalidBoolean(b) => write!(f, "valor booleano inválido: 0x{:02X}", b),
            LdbError::InvalidKeyLength => write!(f, "longitud de clave inválida"),
            LdbError::InvalidUtf8 => write!(f, "clave o string no es UTF-8 válido"),
            LdbError::UnexpectedEndOfDocument => write!(f, "fin de documento inesperado"),
            LdbError::MissingEndOfDocument => write!(f, "falta marcador de fin de documento"),
            LdbError::ExtraBytesAfterDocument => write!(f, "bytes extra después del documento"),
            LdbError::SubDocumentLengthMismatch => write!(f, "longitud de sub-documento no coincide"),
        }
    }
}

impl std::error::Error for LdbError {}

pub fn serialize(doc: &Document) -> Vec<u8> {
    let mut body = Vec::new();
    for (key, value) in &doc.fields {
        serialize_field(&mut body, key, value);
    }
    body.push(tags::END_OF_DOCUMENT);

    let doc_size = (16 + body.len()) as u32;
    let field_count = doc.fields.len() as u32;

    let mut out = Vec::with_capacity(doc_size as usize);
    out.extend_from_slice(MAGIC);
    out.push(0);
    out.push(1); 
    out.push(0); 
    out.push(0); 
    out.extend_from_slice(&doc_size.to_le_bytes());
    out.extend_from_slice(&field_count.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn serialize_field(out: &mut Vec<u8>, key: &str, value: &LdbValue) {
    out.push(value.tag());
    let key_bytes = key.as_bytes();
    assert!(key_bytes.len() <= u8::MAX as usize, "clave excede 255 bytes");
    out.push(key_bytes.len() as u8);
    out.extend_from_slice(key_bytes);

    match value {
        LdbValue::Int32(v) => out.extend_from_slice(&v.to_le_bytes()),
        LdbValue::Int64(v) => out.extend_from_slice(&v.to_le_bytes()),
        LdbValue::Float64(v) => out.extend_from_slice(&v.to_le_bytes()),
        LdbValue::String(v) => {
            let bytes = v.as_bytes();
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        LdbValue::Boolean(v) => out.push(if *v { 1 } else { 0 }),
        LdbValue::SubDocument(doc) => {
            let nested_body = serialize_sub_document(doc);
            out.extend_from_slice(&(nested_body.len() as u32).to_le_bytes());
            out.extend_from_slice(&nested_body);
        }
        LdbValue::Null => {}
    }
}

fn serialize_sub_document(doc: &Document) -> Vec<u8> {
    let mut body = Vec::new();
    for (key, value) in &doc.fields {
        serialize_field(&mut body, key, value);
    }
    body.push(tags::END_OF_DOCUMENT);
    body
}

pub fn deserialize(bytes: &[u8]) -> Result<Document, LdbError> {
    if bytes.len() < 16 {
        return Err(LdbError::UnexpectedEndOfDocument);
    }

    if &bytes[0..4] != MAGIC {
        return Err(LdbError::InvalidMagic);
    }

    let doc_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if doc_size != bytes.len() {
        return Err(LdbError::InvalidDocSize);
    }

    let field_count = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;

    let mut doc = Document::new();
    let mut pos = 16;
    let mut count = 0;

    while pos < bytes.len() {
        if pos >= bytes.len() {
            return Err(LdbError::UnexpectedEndOfDocument);
        }
        let tag = bytes[pos];
        if tag == tags::END_OF_DOCUMENT {
            pos += 1;
            break;
        }
        let (key, value, consumed) = deserialize_field(bytes, pos)?;
        doc.insert(key, value);
        count += 1;
        pos += consumed;
    }

    if count != field_count {
        return Err(LdbError::InvalidFieldCount);
    }

    if pos != bytes.len() {
        return Err(LdbError::ExtraBytesAfterDocument);
    }

    Ok(doc)
}

fn deserialize_field(bytes: &[u8], start: usize) -> Result<(String, LdbValue, usize), LdbError> {
    if start + 2 > bytes.len() {
        return Err(LdbError::UnexpectedEndOfDocument);
    }

    let tag = bytes[start];
    let key_len = bytes[start + 1] as usize;
    if key_len == 0 {
        return Err(LdbError::InvalidKeyLength);
    }

    let key_start = start + 2;
    let key_end = key_start + key_len;
    if key_end > bytes.len() {
        return Err(LdbError::UnexpectedEndOfDocument);
    }

    let key = std::str::from_utf8(&bytes[key_start..key_end])
        .map_err(|_| LdbError::InvalidUtf8)?
        .to_string();

    let value_start = key_end;
    let (value, value_len) = match tag {
        tags::INT32 => {
            if value_start + 4 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let v = i32::from_le_bytes([
                bytes[value_start],
                bytes[value_start + 1],
                bytes[value_start + 2],
                bytes[value_start + 3],
            ]);
            (LdbValue::Int32(v), 4)
        }
        tags::INT64 => {
            if value_start + 8 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[value_start..value_start + 8]);
            (LdbValue::Int64(i64::from_le_bytes(arr)), 8)
        }
        tags::FLOAT64 => {
            if value_start + 8 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[value_start..value_start + 8]);
            (LdbValue::Float64(f64::from_le_bytes(arr)), 8)
        }
        tags::STRING => {
            if value_start + 4 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let len = u32::from_le_bytes([
                bytes[value_start],
                bytes[value_start + 1],
                bytes[value_start + 2],
                bytes[value_start + 3],
            ]) as usize;
            let str_start = value_start + 4;
            let str_end = str_start + len;
            if str_end > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let s = std::str::from_utf8(&bytes[str_start..str_end])
                .map_err(|_| LdbError::InvalidUtf8)?
                .to_string();
            (LdbValue::String(s), 4 + len)
        }
        tags::BOOLEAN => {
            if value_start + 1 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let b = match bytes[value_start] {
                0 => false,
                1 => true,
                other => return Err(LdbError::InvalidBoolean(other)),
            };
            (LdbValue::Boolean(b), 1)
        }
        tags::SUB_DOCUMENT => {
            if value_start + 4 > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            let len = u32::from_le_bytes([
                bytes[value_start],
                bytes[value_start + 1],
                bytes[value_start + 2],
                bytes[value_start + 3],
            ]) as usize;
            let body_start = value_start + 4;
            let body_end = body_start + len;
            if body_end > bytes.len() {
                return Err(LdbError::UnexpectedEndOfDocument);
            }
            if len == 0 || bytes[body_end - 1] != tags::END_OF_DOCUMENT {
                return Err(LdbError::MissingEndOfDocument);
            }
            let sub_doc = deserialize_sub_document(&bytes[body_start..body_end])?;
            (LdbValue::SubDocument(sub_doc), 4 + len)
        }
        tags::NULL => (LdbValue::Null, 0),
        other => return Err(LdbError::InvalidTypeTag(other)),
    };

    let total_consumed = 2 + key_len + value_len;
    Ok((key, value, total_consumed))
}

fn deserialize_sub_document(bytes: &[u8]) -> Result<Document, LdbError> {
    let mut doc = Document::new();
    let mut pos = 0;

    while pos < bytes.len() {
        if bytes[pos] == tags::END_OF_DOCUMENT {
            if pos + 1 != bytes.len() {
                return Err(LdbError::SubDocumentLengthMismatch);
            }
            break;
        }
        let (key, value, consumed) = deserialize_field(bytes, pos)?;
        doc.insert(key, value);
        pos += consumed;
    }

    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_simple() {
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
            0x05, 0x06, 0x61, 0x63, 0x74, 0x69, 0x76, 0x6F, 0x01,
            0x01, 0x04, 0x65, 0x64, 0x61, 0x64, 0x15, 0x00, 0x00, 0x00,
            0xFF,
        ];
        assert_eq!(bytes, expected);

        let parsed = deserialize(&bytes).unwrap();
        assert_eq!(parsed, doc);
    }
}
