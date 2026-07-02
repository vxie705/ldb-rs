use ldb::btree::BPlusTree;
use ldb::index_persistence::{load_index, save_index};
use ldb::spec::{deserialize, serialize, Document, LdbValue};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

fn main() {
    let data_path = "data.ldb";
    let index_path = "index.ldb";

    let _ = std::fs::remove_file(data_path);
    let _ = std::fs::remove_file(index_path);

    let docs = vec![
        make_doc("Ana", 21, true),
        make_doc("Luis", 25, false),
        make_doc("Pedro", 30, true),
        make_doc("María", 22, true),
        make_doc("Carmen", 28, false),
    ];

    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(data_path)
        .expect("abrir data.ldb");

    let mut offsets = Vec::new();
    for doc in &docs {
        let bytes = serialize(doc);
        let offset = file.stream_position().expect("stream_position");
        file.write_all(&bytes).expect("escribir documento");
        offsets.push(offset);
    }
    file.flush().expect("flush");

    println!("Escritos {} documentos en {}", docs.len(), data_path);
    for (i, off) in offsets.iter().enumerate() {
        println!("  Doc[{}] @ offset 0x{:04X}", i, off);
    }

    let mut index = BPlusTree::new(4);
    for (doc, offset) in docs.iter().zip(offsets.iter()) {
        if let LdbValue::Int32(edad) = doc.fields.get("edad").unwrap() {
            index.insert(LdbValue::Int32(*edad), *offset);
        }
    }

    println!("\nBúsqueda exacta edad=25:");
    if let Some(offset) = index.search(&LdbValue::Int32(25)) {
        let doc = fetch_document(&mut file, offset);
        println!("  Encontrado @ 0x{:04X}: {:?}", offset, doc.fields.get("nombre"));
    }

    println!("\nBúsqueda por rango edad > 23:");
    for offset in index.range_greater_than(&LdbValue::Int32(23)) {
        let doc = fetch_document(&mut file, offset);
        println!(
            "  @ 0x{:04X}: {:?} tiene {:?} años",
            offset,
            doc.fields.get("nombre").unwrap(),
            doc.fields.get("edad").unwrap()
        );
    }

    println!("\nBúsqueda por rango 21 < edad < 29:");
    for offset in index.range_between(&LdbValue::Int32(21), &LdbValue::Int32(29)) {
        let doc = fetch_document(&mut file, offset);
        println!(
            "  @ 0x{:04X}: {:?} tiene {:?} años",
            offset,
            doc.fields.get("nombre").unwrap(),
            doc.fields.get("edad").unwrap()
        );
    }

    println!("\nGuardando índice en {}...", index_path);
    save_index(&index, index_path).expect("guardar índice");
    println!("  Índice guardado.");

    println!("\nCargando índice desde {}...", index_path);
    let loaded_index = load_index(index_path).expect("cargar índice");
    println!("  Índice cargado: orden {}", loaded_index.order());

    let recovered = loaded_index.search(&LdbValue::Int32(25));
    assert_eq!(recovered, Some(offsets[1]));
    println!("  Búsqueda con índice cargado edad=25: {:?}", recovered);

    println!("\nVerificación de integridad (todos los docs leídos correctamente):");
    for (i, offset) in offsets.iter().enumerate() {
        let doc = fetch_document(&mut file, *offset);
        assert_eq!(doc, docs[i]);
        println!("  Doc[{}] OK", i);
    }

    println!("\n✅ End-to-end completado exitosamente.");
}

fn make_doc(nombre: &str, edad: i32, activo: bool) -> Document {
    let mut doc = Document::new();
    doc.insert("nombre", LdbValue::String(nombre.to_string()));
    doc.insert("edad", LdbValue::Int32(edad));
    doc.insert("activo", LdbValue::Boolean(activo));
    doc
}

fn fetch_document(file: &mut File, offset: u64) -> Document {
    file.seek(SeekFrom::Start(offset)).expect("seek");
    let mut header = [0u8; 16];
    file.read_exact(&mut header).expect("leer header");
    let doc_size = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    let mut buf = vec![0u8; doc_size];
    file.seek(SeekFrom::Start(offset)).expect("seek");
    file.read_exact(&mut buf).expect("leer documento completo");
    deserialize(&buf).expect("deserializar")
}