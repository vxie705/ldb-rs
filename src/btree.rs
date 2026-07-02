use crate::spec::LdbValue;

fn cmp_ldb_value(a: &LdbValue, b: &LdbValue) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let ord = (a.discriminant() as u8).cmp(&(b.discriminant() as u8));
    if ord != Ordering::Equal {
        return ord;
    }

    match (a, b) {
        (LdbValue::Int32(x), LdbValue::Int32(y)) => x.cmp(y),
        (LdbValue::Int64(x), LdbValue::Int64(y)) => x.cmp(y),
        (LdbValue::Float64(x), LdbValue::Float64(y)) => {
            if x.is_nan() && y.is_nan() {
                x.to_bits().cmp(&y.to_bits())
            } else if x.is_nan() {
                Ordering::Greater
            } else if y.is_nan() {
                Ordering::Less
            } else {
                x.partial_cmp(y).unwrap_or(Ordering::Equal)
            }
        }
        (LdbValue::String(x), LdbValue::String(y)) => x.cmp(y),
        (LdbValue::Boolean(x), LdbValue::Boolean(y)) => x.cmp(y),
        (LdbValue::SubDocument(_), LdbValue::SubDocument(_)) => {
            Ordering::Equal
        }
        (LdbValue::Null, LdbValue::Null) => Ordering::Equal,
        _ => unreachable!(),
    }
}

fn binary_search_ldb(keys: &[LdbValue], key: &LdbValue) -> Result<usize, usize> {
    let mut low = 0;
    let mut high = keys.len();
    while low < high {
        let mid = (low + high) / 2;
        match cmp_ldb_value(&keys[mid], key) {
            std::cmp::Ordering::Less => low = mid + 1,
            std::cmp::Ordering::Greater => high = mid,
            std::cmp::Ordering::Equal => return Ok(mid),
        }
    }
    Err(low)
}

pub type NodeId = usize;

#[derive(Debug, Clone)]
pub struct InternalNode {
    pub keys: Vec<LdbValue>,
    pub children: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct LeafNode {
    pub keys: Vec<LdbValue>,
    pub offsets: Vec<u64>,
    pub next: Option<NodeId>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Internal(InternalNode),
    Leaf(LeafNode),
}

#[derive(Debug, Default, Clone)]
pub struct NodeArena {
    pub(crate) nodes: Vec<Node>,
}

impl NodeArena {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn alloc(&mut self, node: Node) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }

    pub fn get(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id]
    }
}

#[derive(Debug, Clone)]
pub struct BPlusTree {
    pub(crate) arena: NodeArena,
    pub(crate) root: NodeId,
    pub(crate) order: usize,
}

impl BPlusTree {
    pub fn new(order: usize) -> Self {
        assert!(order >= 3, "el orden debe ser al menos 3");
        let mut arena = NodeArena::new();
        let root = arena.alloc(Node::Leaf(LeafNode {
            keys: Vec::new(),
            offsets: Vec::new(),
            next: None,
        }));
        Self { arena, root, order }
    }

    pub fn order(&self) -> usize {
        self.order
    }

    pub fn insert(&mut self, key: LdbValue, offset: u64) {
        let leaf_id = self.find_leaf(self.root, &key);
        if let Some(idx) = self.key_index_in_leaf(leaf_id, &key) {
            if let Node::Leaf(leaf) = self.arena.get_mut(leaf_id) {
                leaf.offsets[idx] = offset;
            }
            return;
        }

        let pos = self.insert_position_in_leaf(leaf_id, &key);
        if let Node::Leaf(leaf) = self.arena.get_mut(leaf_id) {
            leaf.keys.insert(pos, key);
            leaf.offsets.insert(pos, offset);
        }

        if self.is_leaf_full(leaf_id) {
            self.split_leaf(leaf_id);
        }
    }

    pub fn search(&self, key: &LdbValue) -> Option<u64> {
        let leaf_id = self.find_leaf(self.root, key);
        if let Node::Leaf(leaf) = self.arena.get(leaf_id) {
            if let Ok(idx) = binary_search_ldb(&leaf.keys, key) {
                return Some(leaf.offsets[idx]);
            }
        }
        None
    }
    pub fn range_greater_than(&self, x: &LdbValue) -> Vec<u64> {
        let mut results = Vec::new();
        let mut leaf_id = self.find_leaf(self.root, x);

        loop {
            let next = {
                let leaf = match self.arena.get(leaf_id) {
                    Node::Leaf(l) => l,
                    _ => unreachable!(),
                };

                let start = match binary_search_ldb(&leaf.keys, x) {
                    Ok(idx) => idx + 1,
                    Err(idx) => idx,
                };

                for i in start..leaf.keys.len() {
                    results.push(leaf.offsets[i]);
                }

                leaf.next
            };

            match next {
                Some(n) => leaf_id = n,
                None => break,
            }
        }

        results
    }
    pub fn range_less_than(&self, x: &LdbValue) -> Vec<u64> {
        let mut results = Vec::new();
        let mut leaf_id = self.find_leftmost_leaf(self.root);
        let target_id = self.find_leaf(self.root, x);

        while leaf_id != target_id {
            let next = {
                let leaf = match self.arena.get(leaf_id) {
                    Node::Leaf(l) => l,
                    _ => unreachable!(),
                };
                for i in 0..leaf.keys.len() {
                    results.push(leaf.offsets[i]);
                }
                leaf.next
            };
            match next {
                Some(n) => leaf_id = n,
                None => break,
            }
        }

        if leaf_id == target_id {
            let leaf = match self.arena.get(leaf_id) {
                Node::Leaf(l) => l,
                _ => unreachable!(),
            };
            let end = match binary_search_ldb(&leaf.keys, x) {
                Ok(idx) => idx,
                Err(idx) => idx,
            };
            for i in 0..end {
                results.push(leaf.offsets[i]);
            }
        }

        results
    }

    pub fn range_between(&self, x: &LdbValue, y: &LdbValue) -> Vec<u64> {
        let mut results = Vec::new();
        let mut leaf_id = self.find_leaf(self.root, x);
        let mut start_from_beginning = false;

        loop {
            let (next, collected) = {
                let leaf = match self.arena.get(leaf_id) {
                    Node::Leaf(l) => l,
                    _ => unreachable!(),
                };

                let start = if start_from_beginning {
                    0
                } else {
                    match binary_search_ldb(&leaf.keys, x) {
                        Ok(idx) => idx + 1,
                        Err(idx) => idx,
                    }
                };

                let mut stop = false;
                for i in start..leaf.keys.len() {
                    if cmp_ldb_value(&leaf.keys[i], y) == std::cmp::Ordering::Less {
                        results.push(leaf.offsets[i]);
                    } else {
                        stop = true;
                        break;
                    }
                }

                (leaf.next, stop)
            };

            if collected {
                break;
            }

            start_from_beginning = true;
            match next {
                Some(n) => leaf_id = n,
                None => break,
            }
        }

        results
    }

    fn find_leaf(&self, node_id: NodeId, key: &LdbValue) -> NodeId {
        let mut current = node_id;
        loop {
            match self.arena.get(current) {
                Node::Leaf(_) => return current,
                Node::Internal(internal) => {
                let mut i = 0;
                while i < internal.keys.len()
                    && cmp_ldb_value(key, &internal.keys[i]) != std::cmp::Ordering::Less
                {
                    i += 1;
                }
                    current = internal.children[i];
                }
            }
        }
    }

    fn find_leftmost_leaf(&self, node_id: NodeId) -> NodeId {
        let mut current = node_id;
        loop {
            match self.arena.get(current) {
                Node::Leaf(_) => return current,
                Node::Internal(internal) => current = internal.children[0],
            }
        }
    }

    fn key_index_in_leaf(&self, leaf_id: NodeId, key: &LdbValue) -> Option<usize> {
        if let Node::Leaf(leaf) = self.arena.get(leaf_id) {
            binary_search_ldb(&leaf.keys, key).ok()
        } else {
            unreachable!()
        }
    }

    fn insert_position_in_leaf(&self, leaf_id: NodeId, key: &LdbValue) -> usize {
        if let Node::Leaf(leaf) = self.arena.get(leaf_id) {
            binary_search_ldb(&leaf.keys, key).unwrap_or_else(|idx| idx)
        } else {
            unreachable!()
        }
    }

    fn is_leaf_full(&self, leaf_id: NodeId) -> bool {
        if let Node::Leaf(leaf) = self.arena.get(leaf_id) {
            leaf.keys.len() >= self.order - 1
        } else {
            unreachable!()
        }
    }

    fn split_leaf(&mut self, leaf_id: NodeId) {
        let (new_keys, new_offsets, push_up_key, next) = {
            let leaf = match self.arena.get_mut(leaf_id) {
                Node::Leaf(l) => l,
                _ => unreachable!(),
            };

            let mid = leaf.keys.len() / 2;
            let new_keys = leaf.keys.split_off(mid);
            let new_offsets = leaf.offsets.split_off(mid);
            let push_up_key = new_keys[0].clone();
            let next = leaf.next;

            (new_keys, new_offsets, push_up_key, next)
        };

        let new_leaf_id = self.arena.alloc(Node::Leaf(LeafNode {
            keys: new_keys,
            offsets: new_offsets,
            next,
        }));

        {
            let leaf = match self.arena.get_mut(leaf_id) {
                Node::Leaf(l) => l,
                _ => unreachable!(),
            };
            leaf.next = Some(new_leaf_id);
        }

        let parent_id = self.find_parent(self.root, leaf_id);
        self.insert_in_parent(parent_id, leaf_id, push_up_key, new_leaf_id);
    }

    fn insert_in_parent(&mut self, parent_id: Option<NodeId>, left_id: NodeId, key: LdbValue, right_id: NodeId) {
        match parent_id {
            None => {
                let new_root = InternalNode {
                    keys: vec![key],
                    children: vec![left_id, right_id],
                };
                self.root = self.arena.alloc(Node::Internal(new_root));
            }
            Some(pid) => {
                let pos = {
                    let parent = match self.arena.get(pid) {
                        Node::Internal(p) => p,
                        _ => unreachable!(),
                    };
                    binary_search_ldb(&parent.keys, &key).unwrap_or_else(|idx| idx)
                };

                let parent = match self.arena.get_mut(pid) {
                    Node::Internal(p) => p,
                    _ => unreachable!(),
                };
                parent.keys.insert(pos, key);
                parent.children.insert(pos + 1, right_id);

                if parent.children.len() > self.order {
                    self.split_internal(pid);
                }
            }
        }
    }

    fn split_internal(&mut self, node_id: NodeId) {
        let (new_node_id, push_up_key, parent_id) = {
            let node = match self.arena.get_mut(node_id) {
                Node::Internal(n) => n,
                _ => unreachable!(),
            };

            let mid = node.keys.len() / 2;
            let push_up_key = node.keys[mid].clone();

            let new_keys = node.keys.split_off(mid + 1);
            let new_children = node.children.split_off(mid + 1);
            node.keys.pop();

            let new_node = InternalNode {
                keys: new_keys,
                children: new_children,
            };
            let new_node_id = self.arena.alloc(Node::Internal(new_node));

            (new_node_id, push_up_key, self.find_parent(self.root, node_id))
        };

        self.insert_in_parent(parent_id, node_id, push_up_key, new_node_id);
    }

    fn find_parent(&self, current: NodeId, target: NodeId) -> Option<NodeId> {
        match self.arena.get(current) {
            Node::Leaf(_) => None,
            Node::Internal(internal) => {
                for &child in &internal.children {
                    if child == target {
                        return Some(current);
                    }
                    if let Some(parent) = self.find_parent(child, target) {
                        return Some(parent);
                    }
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_search() {
        let mut tree = BPlusTree::new(4);
        tree.insert(LdbValue::Int32(21), 0x100);
        tree.insert(LdbValue::Int32(30), 0x200);
        tree.insert(LdbValue::Int32(25), 0x150);

        assert_eq!(tree.search(&LdbValue::Int32(21)), Some(0x100));
        assert_eq!(tree.search(&LdbValue::Int32(25)), Some(0x150));
        assert_eq!(tree.search(&LdbValue::Int32(30)), Some(0x200));
        assert_eq!(tree.search(&LdbValue::Int32(99)), None);
    }

    #[test]
    fn range_queries() {
        let mut tree = BPlusTree::new(4);
        for i in 0..10 {
            tree.insert(LdbValue::Int32(i), i as u64 * 0x10);
        }

        let gt = tree.range_greater_than(&LdbValue::Int32(6));
        assert_eq!(gt, vec![0x70, 0x80, 0x90]);

        let lt = tree.range_less_than(&LdbValue::Int32(3));
        assert_eq!(lt, vec![0x00, 0x10, 0x20]);

        let between = tree.range_between(&LdbValue::Int32(3), &LdbValue::Int32(7));
        assert_eq!(between, vec![0x40, 0x50, 0x60]);
    }

    #[test]
    fn split_creates_internal_nodes() {
        let mut tree = BPlusTree::new(4);
        for i in 0..20 {
            tree.insert(LdbValue::Int32(i), i as u64);
        }

        for i in 0..20 {
            assert_eq!(tree.search(&LdbValue::Int32(i)), Some(i as u64));
        }
    }
}
