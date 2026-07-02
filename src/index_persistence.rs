use crate::btree::{BPlusTree, InternalNode, LeafNode, Node, NodeArena};
use crate::spec::LdbValue;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const INDEX_MAGIC: &[u8; 4] = b"IDX\0";

const INDEX_VERSION: [u8; 2] = [0, 1];

const HEADER_SIZE: u32 = 32;

const CHECKSUM_OFFSET: usize = 24;

const NULL_NODE_ID: u32 = 0xFFFFFFFF;

const FLAG_CHECKSUM: u16 = 0x0001;

#[derive(Debug, PartialEq)]
pub enum IndexError {
    InvalidMagic,
    InvalidVersion,
    InvalidHeaderSize,
    InvalidNodeCount,
    InvalidRootNodeId,
    InvalidNodeType(u8),
    InvalidNodeSize,
    InvalidKeyCount,
    InvalidChecksum,
    InvalidLdbValueTag(u8),
    InvalidBoolean(u8),
    InvalidUtf8,
    UnexpectedEndOfFile,
    ExtraBytesAfterIndex,
    IoError(String),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::InvalidMagic => write!(f, "magic bytes inválidos"),
            IndexError::InvalidVersion => write!(f, "versión no soportada"),
            IndexError::InvalidHeaderSize => write!(f, "tamaño de cabecera inválido"),
            IndexError::InvalidNodeCount => write!(f, "NodeCount inválido"),
            IndexError::InvalidRootNodeId => write!(f, "RootNodeId inválido"),
            IndexError::InvalidNodeType(t) => write!(f, "tipo de nodo inválido: 0x{:02X}", t),
            IndexError::InvalidNodeSize => write!(f, "NodeSize no coincide"),
            IndexError::InvalidKeyCount => write!(f, "KeyCount inválido"),
            IndexError::InvalidChecksum => write!(f, "checksum CRC64 inválido"),
            IndexError::InvalidLdbValueTag(t) => write!(f, "tag de LdbValue inválido: 0x{:02X}", t),
            IndexError::InvalidBoolean(b) => write!(f, "valor booleano inválido: 0x{:02X}", b),
            IndexError::InvalidUtf8 => write!(f, "string no es UTF-8 válido"),
            IndexError::UnexpectedEndOfFile => write!(f, "fin de archivo inesperado"),
            IndexError::ExtraBytesAfterIndex => write!(f, "bytes extra después del índice"),
            IndexError::IoError(e) => write!(f, "error de E/S: {}", e),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<std::io::Error> for IndexError {
    fn from(e: std::io::Error) -> Self {
        IndexError::IoError(e.to_string())
    }
}

pub fn save_index<P: AsRef<Path>>(tree: &BPlusTree, path: P) -> Result<(), IndexError> {
    let bytes = serialize_index(tree);

    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    file.flush()?;
    Ok(())
}

pub fn load_index<P: AsRef<Path>>(path: P) -> Result<BPlusTree, IndexError> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    deserialize_index(&bytes)
}

pub fn serialize_index(tree: &BPlusTree) -> Vec<u8> {
    let mut out = Vec::new();

    out.extend_from_slice(INDEX_MAGIC);
    out.extend_from_slice(&INDEX_VERSION);
    out.extend_from_slice(&FLAG_CHECKSUM.to_le_bytes());
    out.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    out.extend_from_slice(&(tree.arena.nodes.len() as u32).to_le_bytes());
    out.extend_from_slice(&(tree.root as u32).to_le_bytes());
    out.extend_from_slice(&(tree.order as u32).to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());

    for node in &tree.arena.nodes {
        serialize_node(&mut out, node);
    }

    let checksum = crc64(&out);
    out[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 8].copy_from_slice(&checksum.to_le_bytes());

    out
}

pub fn deserialize_index(bytes: &[u8]) -> Result<BPlusTree, IndexError> {
    if bytes.len() < HEADER_SIZE as usize {
        return Err(IndexError::UnexpectedEndOfFile);
    }

    if &bytes[0..4] != INDEX_MAGIC {
        return Err(IndexError::InvalidMagic);
    }

    let version = [bytes[4], bytes[5]];
    if version != INDEX_VERSION {
        return Err(IndexError::InvalidVersion);
    }

    let header_size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    if header_size != HEADER_SIZE {
        return Err(IndexError::InvalidHeaderSize);
    }

    let node_count = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let root_node_id = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) as usize;
    let order = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) as usize;

    if node_count == 0 {
        return Err(IndexError::InvalidNodeCount);
    }
    if root_node_id >= node_count {
        return Err(IndexError::InvalidRootNodeId);
    }

    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    if flags & FLAG_CHECKSUM != 0 {
        let stored = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27],
            bytes[28], bytes[29], bytes[30], bytes[31],
        ]);
        let mut copy = bytes.to_vec();
        copy[CHECKSUM_OFFSET..CHECKSUM_OFFSET + 8].fill(0);
        let computed = crc64(&copy);
        if stored != computed {
            return Err(IndexError::InvalidChecksum);
        }
    }

    let mut arena = NodeArena::new();
    let mut pos = HEADER_SIZE as usize;

    for _ in 0..node_count {
        let (node, consumed) = deserialize_node(bytes, pos)?;
        arena.alloc(node);
        pos += consumed;
    }

    if pos != bytes.len() {
        return Err(IndexError::ExtraBytesAfterIndex);
    }

    Ok(BPlusTree { arena, root: root_node_id, order })
}

fn serialize_node(out: &mut Vec<u8>, node: &Node) {
    match node {
        Node::Internal(internal) => serialize_internal_node(out, internal),
        Node::Leaf(leaf) => serialize_leaf_node(out, leaf),
    }
}

fn serialize_internal_node(out: &mut Vec<u8>, node: &InternalNode) {
    let key_count = node.keys.len() as u32;
    let mut body = Vec::new();

    for key in &node.keys {
        serialize_value(&mut body, key);
    }
    for &child in &node.children {
        body.extend_from_slice(&(child as u32).to_le_bytes());
    }

    let node_size = (9 + body.len()) as u32;

    out.push(0x01);
    out.extend_from_slice(&node_size.to_le_bytes());
    out.extend_from_slice(&key_count.to_le_bytes());
    out.extend_from_slice(&body);
}

fn serialize_leaf_node(out: &mut Vec<u8>, node: &LeafNode) {
    let key_count = node.keys.len() as u32;
    let mut body = Vec::new();

    for key in &node.keys {
        serialize_value(&mut body, key);
    }
    for &offset in &node.offsets {
        body.extend_from_slice(&offset.to_le_bytes());
    }
    let next = node.next.map(|id| id as u32).unwrap_or(NULL_NODE_ID);
    body.extend_from_slice(&next.to_le_bytes());

    let node_size = (9 + body.len()) as u32;

    out.push(0x02);
    out.extend_from_slice(&node_size.to_le_bytes());
    out.extend_from_slice(&key_count.to_le_bytes());
    out.extend_from_slice(&body);
}

fn deserialize_node(bytes: &[u8], start: usize) -> Result<(Node, usize), IndexError> {
    if start + 9 > bytes.len() {
        return Err(IndexError::UnexpectedEndOfFile);
    }

    let node_type = bytes[start];
    let node_size = u32::from_le_bytes([
        bytes[start + 1],
        bytes[start + 2],
        bytes[start + 3],
        bytes[start + 4],
    ]) as usize;
    let key_count = u32::from_le_bytes([
        bytes[start + 5],
        bytes[start + 6],
        bytes[start + 7],
        bytes[start + 8],
    ]) as usize;

    if node_size < 9 {
        return Err(IndexError::InvalidNodeSize);
    }
    if start + node_size > bytes.len() {
        return Err(IndexError::UnexpectedEndOfFile);
    }

    let body = &bytes[start + 9..start + node_size];
    let node = match node_type {
        0x01 => Node::Internal(deserialize_internal_node_body(body, key_count)?),
        0x02 => Node::Leaf(deserialize_leaf_node_body(body, key_count)?),
        other => return Err(IndexError::InvalidNodeType(other)),
    };

    Ok((node, node_size))
}

fn deserialize_internal_node_body(body: &[u8], key_count: usize) -> Result<InternalNode, IndexError> {
    let mut pos = 0;
    let mut keys = Vec::with_capacity(key_count);

    for _ in 0..key_count {
        let (value, consumed) = deserialize_value(body, pos)?;
        keys.push(value);
        pos += consumed;
    }

    let expected_children_size = (key_count + 1) * 4;
    if pos + expected_children_size != body.len() {
        return Err(IndexError::InvalidNodeSize);
    }

    let mut children = Vec::with_capacity(key_count + 1);
    for _ in 0..=key_count {
        let id = u32::from_le_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
        children.push(id as usize);
        pos += 4;
    }

    Ok(InternalNode { keys, children })
}

fn deserialize_leaf_node_body(body: &[u8], key_count: usize) -> Result<LeafNode, IndexError> {
    let mut pos = 0;
    let mut keys = Vec::with_capacity(key_count);

    for _ in 0..key_count {
        let (value, consumed) = deserialize_value(body, pos)?;
        keys.push(value);
        pos += consumed;
    }

    let expected_offsets_size = key_count * 8;
    if pos + expected_offsets_size + 4 != body.len() {
        return Err(IndexError::InvalidNodeSize);
    }

    let mut offsets = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        let offset = u64::from_le_bytes([
            body[pos], body[pos + 1], body[pos + 2], body[pos + 3],
            body[pos + 4], body[pos + 5], body[pos + 6], body[pos + 7],
        ]);
        offsets.push(offset);
        pos += 8;
    }

    let next = u32::from_le_bytes([body[pos], body[pos + 1], body[pos + 2], body[pos + 3]]);
    let next = if next == NULL_NODE_ID { None } else { Some(next as usize) };

    Ok(LeafNode { keys, offsets, next })
}

fn serialize_value(out: &mut Vec<u8>, value: &LdbValue) {
    out.push(value.tag());
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
            let nested = crate::spec::serialize(doc);
            out.extend_from_slice(&(nested.len() as u32).to_le_bytes());
            out.extend_from_slice(&nested);
        }
        LdbValue::Null => {}
    }
}

fn deserialize_value(bytes: &[u8], start: usize) -> Result<(LdbValue, usize), IndexError> {
    use crate::spec::tags;

    if start >= bytes.len() {
        return Err(IndexError::UnexpectedEndOfFile);
    }

    let tag = bytes[start];
    let value_start = start + 1;

    let (value, value_len) = match tag {
        tags::INT32 => {
            if value_start + 4 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let v = i32::from_le_bytes([bytes[value_start], bytes[value_start + 1], bytes[value_start + 2], bytes[value_start + 3]]);
            (LdbValue::Int32(v), 4)
        }
        tags::INT64 => {
            if value_start + 8 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[value_start..value_start + 8]);
            (LdbValue::Int64(i64::from_le_bytes(arr)), 8)
        }
        tags::FLOAT64 => {
            if value_start + 8 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes[value_start..value_start + 8]);
            (LdbValue::Float64(f64::from_le_bytes(arr)), 8)
        }
        tags::STRING => {
            if value_start + 4 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let len = u32::from_le_bytes([
                bytes[value_start], bytes[value_start + 1], bytes[value_start + 2], bytes[value_start + 3],
            ]) as usize;
            let str_start = value_start + 4;
            let str_end = str_start + len;
            if str_end > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let s = std::str::from_utf8(&bytes[str_start..str_end])
                .map_err(|_| IndexError::InvalidUtf8)?
                .to_string();
            (LdbValue::String(s), 4 + len)
        }
        tags::BOOLEAN => {
            if value_start + 1 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let b = match bytes[value_start] {
                0 => false,
                1 => true,
                other => return Err(IndexError::InvalidBoolean(other)),
            };
            (LdbValue::Boolean(b), 1)
        }
        tags::SUB_DOCUMENT => {
            if value_start + 4 > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let len = u32::from_le_bytes([
                bytes[value_start], bytes[value_start + 1], bytes[value_start + 2], bytes[value_start + 3],
            ]) as usize;
            let body_start = value_start + 4;
            let body_end = body_start + len;
            if body_end > bytes.len() {
                return Err(IndexError::UnexpectedEndOfFile);
            }
            let doc = crate::spec::deserialize(&bytes[body_start..body_end])
                .map_err(|_| IndexError::InvalidLdbValueTag(tag))?;
            (LdbValue::SubDocument(doc), 4 + len)
        }
        tags::NULL => (LdbValue::Null, 0),
        other => return Err(IndexError::InvalidLdbValueTag(other)),
    };

    Ok((value, 1 + value_len))
}

/// CRC64 simple (ISO/IEC 3309 polynomial).
fn crc64(data: &[u8]) -> u64 {
    const POLY: u64 = 0xC96C5795D7870F42;
    let mut crc: u64 = 0;
    for &byte in data {
        crc ^= (byte as u64) << 56;
        for _ in 0..8 {
            if crc & (1u64 << 63) != 0 {
                crc = (crc << 1) ^ POLY;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::LdbValue;

    #[test]
    fn roundtrip_small_tree() {
        let mut tree = BPlusTree::new(4);
        tree.insert(LdbValue::Int32(21), 0x100);
        tree.insert(LdbValue::Int32(30), 0x200);
        tree.insert(LdbValue::Int32(25), 0x150);

        let bytes = serialize_index(&tree);
        let loaded = deserialize_index(&bytes).unwrap();

        assert_eq!(loaded.order(), tree.order());
        assert_eq!(loaded.search(&LdbValue::Int32(21)), Some(0x100));
        assert_eq!(loaded.search(&LdbValue::Int32(25)), Some(0x150));
        assert_eq!(loaded.search(&LdbValue::Int32(30)), Some(0x200));
        assert_eq!(loaded.search(&LdbValue::Int32(99)), None);
    }

    #[test]
    fn roundtrip_large_tree() {
        let mut tree = BPlusTree::new(64);
        for i in 0..1000 {
            tree.insert(LdbValue::Int32(i), i as u64 * 0x1000);
        }

        let bytes = serialize_index(&tree);
        let loaded = deserialize_index(&bytes).unwrap();

        for i in 0..1000 {
            assert_eq!(loaded.search(&LdbValue::Int32(i)), Some(i as u64 * 0x1000));
        }
    }

    #[test]
    fn invalid_magic_fails() {
        let mut bytes = serialize_index(&BPlusTree::new(4));
        bytes[0..4].copy_from_slice(b"BAD\0");
        assert!(deserialize_index(&bytes).is_err());
    }

    #[test]
    fn invalid_checksum_fails() {
        let mut bytes = serialize_index(&BPlusTree::new(4));
        bytes[CHECKSUM_OFFSET] = !bytes[CHECKSUM_OFFSET];
        assert!(deserialize_index(&bytes).is_err());
    }
}