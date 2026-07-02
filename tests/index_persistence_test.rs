use ldb::btree::BPlusTree;
use ldb::index_persistence::{load_index, save_index};
use ldb::spec::LdbValue;
use std::fs;

#[test]
fn save_and_load_small_index() {
    let path = "test_index_small.ldb";
    let _ = fs::remove_file(path);

    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::Int32(21), 0x100);
    tree.insert(LdbValue::Int32(30), 0x200);
    tree.insert(LdbValue::Int32(25), 0x150);

    save_index(&tree, path).expect("guardar índice");
    let loaded = load_index(path).expect("cargar índice");

    assert_eq!(loaded.order(), tree.order());
    assert_eq!(loaded.search(&LdbValue::Int32(21)), Some(0x100));
    assert_eq!(loaded.search(&LdbValue::Int32(25)), Some(0x150));
    assert_eq!(loaded.search(&LdbValue::Int32(30)), Some(0x200));
    assert_eq!(loaded.search(&LdbValue::Int32(99)), None);

    let _ = fs::remove_file(path);
}

#[test]
fn save_and_load_large_index() {
    let path = "test_index_large.ldb";
    let _ = fs::remove_file(path);

    let mut tree = BPlusTree::new(64);
    for i in 0..1000 {
        tree.insert(LdbValue::Int32(i), i as u64 * 0x1000);
    }

    save_index(&tree, path).expect("guardar índice");
    let loaded = load_index(path).expect("cargar índice");

    for i in 0..1000 {
        assert_eq!(
            loaded.search(&LdbValue::Int32(i)),
            Some(i as u64 * 0x1000),
            "falló la búsqueda de {}",
            i
        );
    }

    let _ = fs::remove_file(path);
}

#[test]
fn save_and_load_range_queries() {
    let path = "test_index_range.ldb";
    let _ = fs::remove_file(path);

    let mut tree = BPlusTree::new(4);
    for i in 1..=10 {
        tree.insert(LdbValue::Int32(i * 10), i as u64 * 0x100);
    }

    save_index(&tree, path).expect("guardar índice");
    let loaded = load_index(path).expect("cargar índice");

    let gt = loaded.range_greater_than(&LdbValue::Int32(50));
    assert_eq!(gt, vec![0x600, 0x700, 0x800, 0x900, 0xA00]);

    let lt = loaded.range_less_than(&LdbValue::Int32(50));
    assert_eq!(lt, vec![0x100, 0x200, 0x300, 0x400]);

    let between = loaded.range_between(&LdbValue::Int32(20), &LdbValue::Int32(60));
    assert_eq!(between, vec![0x300, 0x400, 0x500]);

    let _ = fs::remove_file(path);
}

#[test]
fn save_and_load_string_keys() {
    let path = "test_index_strings.ldb";
    let _ = fs::remove_file(path);

    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::String("ana".to_string()), 0x100);
    tree.insert(LdbValue::String("luis".to_string()), 0x200);
    tree.insert(LdbValue::String("pedro".to_string()), 0x300);

    save_index(&tree, path).expect("guardar índice");
    let loaded = load_index(path).expect("cargar índice");

    assert_eq!(
        loaded.search(&LdbValue::String("luis".to_string())),
        Some(0x200)
    );

    let between = loaded.range_between(
        &LdbValue::String("ana".to_string()),
        &LdbValue::String("pedro".to_string()),
    );
    assert_eq!(between, vec![0x200]);

    let _ = fs::remove_file(path);
}