use ldb::btree::BPlusTree;
use ldb::spec::LdbValue;

#[test]
fn search_existing_keys() {
    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::Int32(10), 0x1000);
    tree.insert(LdbValue::Int32(20), 0x2000);
    tree.insert(LdbValue::Int32(30), 0x3000);

    assert_eq!(tree.search(&LdbValue::Int32(10)), Some(0x1000));
    assert_eq!(tree.search(&LdbValue::Int32(20)), Some(0x2000));
    assert_eq!(tree.search(&LdbValue::Int32(30)), Some(0x3000));
}

#[test]
fn search_missing_key_returns_none() {
    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::Int32(10), 0x1000);
    assert_eq!(tree.search(&LdbValue::Int32(99)), None);
}

#[test]
fn duplicate_key_replaces_offset() {
    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::Int32(10), 0x1000);
    tree.insert(LdbValue::Int32(10), 0x9999);
    assert_eq!(tree.search(&LdbValue::Int32(10)), Some(0x9999));
}

#[test]
fn range_greater_than() {
    let mut tree = BPlusTree::new(4);
    for i in 0..10 {
        tree.insert(LdbValue::Int32(i), i as u64 * 0x10);
    }

    let result = tree.range_greater_than(&LdbValue::Int32(6));
    assert_eq!(result, vec![0x70, 0x80, 0x90]);
}

#[test]
fn range_greater_than_not_inclusive() {
    let mut tree = BPlusTree::new(4);
    for i in 0..5 {
        tree.insert(LdbValue::Int32(i * 10), i as u64 * 0x100);
    }

    let result = tree.range_greater_than(&LdbValue::Int32(20));
    assert_eq!(result, vec![0x300, 0x400]);
}

#[test]
fn range_less_than() {
    let mut tree = BPlusTree::new(4);
    for i in 0..10 {
        tree.insert(LdbValue::Int32(i), i as u64 * 0x10);
    }

    let result = tree.range_less_than(&LdbValue::Int32(3));
    assert_eq!(result, vec![0x00, 0x10, 0x20]);
}

#[test]
fn range_less_than_not_inclusive() {
    let mut tree = BPlusTree::new(4);
    for i in 0..5 {
        tree.insert(LdbValue::Int32(i * 10), i as u64 * 0x100);
    }

    let result = tree.range_less_than(&LdbValue::Int32(20));
    assert_eq!(result, vec![0x000, 0x100]);
}

#[test]
fn range_between() {
    let mut tree = BPlusTree::new(4);
    for i in 0..10 {
        tree.insert(LdbValue::Int32(i), i as u64 * 0x10);
    }

    let result = tree.range_between(&LdbValue::Int32(3), &LdbValue::Int32(7));
    assert_eq!(result, vec![0x40, 0x50, 0x60]);
}

#[test]
fn range_between_not_inclusive() {
    let mut tree = BPlusTree::new(4);
    for i in 0..5 {
        tree.insert(LdbValue::Int32(i * 10), i as u64 * 0x100);
    }

    let result = tree.range_between(&LdbValue::Int32(10), &LdbValue::Int32(30));
    assert_eq!(result, vec![0x200]);
}

#[test]
fn large_scale_insert_and_search() {
    let mut tree = BPlusTree::new(64);
    let n = 1000;

    for i in 0..n {
        tree.insert(LdbValue::Int32(i), i as u64 * 0x1000);
    }

    for i in 0..n {
        assert_eq!(
            tree.search(&LdbValue::Int32(i)),
            Some(i as u64 * 0x1000),
            "falló la búsqueda de {}",
            i
        );
    }
}

#[test]
fn large_scale_range_query() {
    let mut tree = BPlusTree::new(64);
    let n = 1000;

    for i in 0..n {
        tree.insert(LdbValue::Int32(i), i as u64);
    }

    let result = tree.range_between(&LdbValue::Int32(100), &LdbValue::Int32(105));
    assert_eq!(result, vec![101, 102, 103, 104]);
}

#[test]
fn string_keys() {
    let mut tree = BPlusTree::new(4);
    tree.insert(LdbValue::String("ana".to_string()), 0x100);
    tree.insert(LdbValue::String("luis".to_string()), 0x200);
    tree.insert(LdbValue::String("pedro".to_string()), 0x300);

    assert_eq!(
        tree.search(&LdbValue::String("luis".to_string())),
        Some(0x200)
    );

    let result = tree.range_between(
        &LdbValue::String("ana".to_string()),
        &LdbValue::String("pedro".to_string()),
    );
    assert_eq!(result, vec![0x200]);
}
