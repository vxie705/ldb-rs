# Especificación del Sistema de Indexación — Árbol B+

## 1. Introducción

Este documento define el sistema de indexación en memoria del motor documental **LDB**. Utiliza un **Árbol B+ (B+ Tree)** para indexar campos específicos de los documentos. El árbol reside completamente en RAM y almacena como valores **offsets (`u64`)** hacia el archivo de datos en disco.

## 2. Arquitectura General

```
MEMORIA (RAM)
┌─────────────────────────────────────────────────────────────┐
│  Index: edad (B+ Tree)        Index: nombre (B+ Tree)       │
│  ┌──────────────┐             ┌──────────────┐               │
│  │ InternalNode │             │ InternalNode │               │
│  │  keys: [...] │             │  keys: [...] │               │
│  │  children:[] │             │  children:[] │               │
│  └──────┬───────┘             └──────┬───────┘               │
│         │                            │                       │
│    LeafNode                       LeafNode                    │
│    keys: [21, 25, 30]             keys: ["Ana", "Luis"]       │
│    offsets: [0x0000, 0x0024, ...] offsets: [0x0048, 0x006C] │
│         │                            │                       │
└─────────┼────────────────────────────┼───────────────────────┘
          │                            │
          ▼                            ▼
DISCO (data.ldb)
┌──────────┬──────────┬──────────┬──────────┐
│  Doc A   │  Doc B   │  Doc C   │  Doc D   │
│ @0x0000  │ @0x0024  │ @0x0048  │ @0x006C  │
└──────────┴──────────┴──────────┴──────────┘
```

**Principio clave**: los nodos hoja no almacenan documentos completos, sino **offsets absolutos en bytes** dentro del archivo `data.ldb`. La recuperación de un documento requiere:

1. Buscar la clave en el B+ Tree.
2. Obtener el `offset` (`u64`).
3. Hacer `seek(offset)` en el archivo.
4. Leer `DocSize` bytes (desde la cabecera LDB).
5. Deserializar con el parser LDB.

## 3. Parámetros del Árbol

| Parámetro | Valor | Descripción |
|-----------|-------|-------------|
| Orden `m` | 64 | Máximo número de hijos por nodo interno |
| Max claves interno | `m - 1 = 63` | |
| Min claves interno | `⌈m/2⌉ - 1 = 31` | Excepto raíz |
| Max claves hoja | `m - 1 = 63` | |
| Min claves hoja | `⌈(m-1)/2⌉ = 32` | Excepto raíz |
| Tipo de clave | `LdbValue` | Valor comparable del documento |
| Tipo de valor | `u64` | Offset absoluto en disco |

## 4. Estructuras de Datos

### 4.1 Nodo Interno

```rust
struct InternalNode {
    is_leaf: bool,              // false
    keys: Vec<LdbValue>,        // m-1 claves de enrutamiento, ordenadas
    children: Vec<NodeId>,      // m punteros a hijos (índices en arena)
}
```

**Invariantes**:
- `keys.len() + 1 == children.len()`
- `keys` está ordenado ascendentemente.
- Para todo `i`: `children[i]` contiene claves `≤ keys[i]` y `children[i+1]` contiene claves `> keys[i]`.

### 4.2 Nodo Hoja

```rust
struct LeafNode {
    is_leaf: bool,              // true
    keys: Vec<LdbValue>,        // hasta m-1 claves, ordenadas
    offsets: Vec<u64>,          // paralelo a keys
    next: Option<NodeId>,       // siguiente hoja (lista enlazada)
}
```

**Invariantes**:
- `keys.len() == offsets.len()`
- `keys` está ordenado ascendentemente.
- Las hojas forman una lista doblemente enlazada lógica mediante `next`.

### 4.3 Nodo Polimórfico

```rust
enum Node {
    Internal(InternalNode),
    Leaf(LeafNode),
}
```

### 4.4 Arena de Nodos

```rust
struct NodeArena {
    nodes: Vec<Node>,
}

type NodeId = usize;
```

Se utiliza una arena en lugar de `Box<Node>` para permitir mutaciones compartidas durante splits/merges y facilitar la futura serialización del índice a disco.

### 4.5 El Árbol

```rust
struct BPlusTree {
    arena: NodeArena,
    root: NodeId,
    order: usize,          
}
```

## 5. Pseudocódigo: Inserción

### 5.1 Inserción principal

```
ALGORITHM BPlusTree_Insert(tree, key, offset)
─────────────────────────────────────────────
INPUT:  tree, key, offset
OUTPUT: tree actualizado

1.  leaf ← Find_Leaf_Node(tree, tree.root, key)
2.  IF key EXISTS IN leaf.keys:
3.      // Política de duplicados: reemplazar offset
4.      idx ← index_of(leaf.keys, key)
5.      leaf.offsets[idx] ← offset
6.      RETURN

7.  pos ← Binary_Search_Insert_Position(leaf.keys, key)
8.  leaf.keys.insert(pos, key)
9.  leaf.offsets.insert(pos, offset)

10. IF leaf.keys.len() < tree.order - 1:
11.     RETURN

12. Split_Leaf(tree, leaf_id)
─────────────────────────────────────────────
```

### 5.2 División de hoja

```
ALGORITHM Split_Leaf(tree, leaf_id)
─────────────────────────────────────────────
1.  leaf ← tree.arena[leaf_id]
2.  mid ← leaf.keys.len() / 2
3.  new_leaf ← NEW LeafNode
4.  new_leaf.keys ← leaf.keys[mid..]
5.  new_leaf.offsets ← leaf.offsets[mid..]
6.  leaf.keys ← leaf.keys[..mid]
7.  leaf.offsets ← leaf.offsets[..mid]
8.  new_leaf.next ← leaf.next
9.  leaf.next ← new_leaf_id
10. push_up_key ← new_leaf.keys[0]
11. Insert_In_Parent(tree, leaf_id, push_up_key, new_leaf_id)
─────────────────────────────────────────────
```

### 5.3 Inserción en padre

```
ALGORITHM Insert_In_Parent(tree, left_id, key, right_id)
─────────────────────────────────────────────
1.  IF left_id IS tree.root:
2.      new_root ← NEW InternalNode
3.      new_root.keys ← [key]
4.      new_root.children ← [left_id, right_id]
5.      tree.root ← new_root_id
6.      RETURN

7.  parent_id ← Find_Parent(tree, tree.root, left_id)
8.  parent ← tree.arena[parent_id]
9.  pos ← Binary_Search_Insert_Position(parent.keys, key)
10. parent.keys.insert(pos, key)
11. parent.children.insert(pos + 1, right_id)

12. IF parent.children.len() > tree.order:
13.     Split_Internal(tree, parent_id)
─────────────────────────────────────────────
```

### 5.4 División de nodo interno

```
ALGORITHM Split_Internal(tree, node_id)
─────────────────────────────────────────────
1.  node ← tree.arena[node_id]
2.  mid ← node.keys.len() / 2
3.  push_up_key ← node.keys[mid]
4.  new_node ← NEW InternalNode
5.  new_node.keys ← node.keys[mid+1..]
6.  new_node.children ← node.children[mid+1..]
7.  node.keys ← node.keys[..mid]
8.  node.children ← node.children[..mid+1]
9.  Insert_In_Parent(tree, node_id, push_up_key, new_node_id)
─────────────────────────────────────────────
```

### 5.5 Búsqueda de hoja

```
ALGORITHM Find_Leaf_Node(tree, node_id, key)
─────────────────────────────────────────────
1.  node ← tree.arena[node_id]
2.  WHILE node IS Internal:
3.      i ← 0
4.      WHILE i < node.keys.len() AND key ≥ node.keys[i]:
5.          i ← i + 1
6.      node_id ← node.children[i]
7.      node ← tree.arena[node_id]
8.  RETURN node_id
─────────────────────────────────────────────
```

## 6. Pseudocódigo: Búsqueda

### 6.1 Búsqueda exacta

```
ALGORITHM BPlusTree_Search(tree, key)
─────────────────────────────────────────────
1.  leaf_id ← Find_Leaf_Node(tree, tree.root, key)
2.  leaf ← tree.arena[leaf_id]
3.  idx ← Binary_Search(leaf.keys, key)
4.  IF idx FOUND:
5.      RETURN Some(leaf.offsets[idx])
6.  RETURN None
─────────────────────────────────────────────
```

### 6.2 Búsqueda por rango: `key > X`

```
ALGORITHM BPlusTree_Range_GreaterThan(tree, X)
─────────────────────────────────────────────
OUTPUT: Vec<u64> de offsets con clave > X

1.  results ← []
2.  leaf_id ← Find_Leaf_Node(tree, tree.root, X)
3.  leaf ← tree.arena[leaf_id]
4.  idx ← Binary_Search(leaf.keys, X)

5.  IF idx FOUND:
6.      start ← idx + 1
7.  ELSE:
8.      start ← Insert_Position(leaf.keys, X)  // primer clave > X

9.  FOR i FROM start TO leaf.keys.len() - 1:
10.     results.push(leaf.offsets[i])

11. current_id ← leaf.next
12. WHILE current_id IS NOT None:
13.     current ← tree.arena[current_id]
14.     FOR i FROM 0 TO current.keys.len() - 1:
15.         results.push(current.offsets[i])
16.     current_id ← current.next

17. RETURN results
─────────────────────────────────────────────
```

### 6.3 Búsqueda por rango: `key < X`

```
ALGORITHM BPlusTree_Range_LessThan(tree, X)
─────────────────────────────────────────────
OUTPUT: Vec<u64> de offsets con clave < X

1.  results ← []
2.  leaf_id ← Find_Leaf_Node(tree, tree.root, X)
3.  leaf ← tree.arena[leaf_id]
4.  idx ← Binary_Search(leaf.keys, X)

5.  IF idx FOUND:
6.      end ← idx          // excluir X
7.  ELSE:
8.      end ← Insert_Position(leaf.keys, X)

9.  current_id ← Find_Leftmost_Leaf(tree, tree.root)

10. WHILE current_id IS NOT None AND current_id ≠ leaf_id:
11.     current ← tree.arena[current_id]
12.     FOR i FROM 0 TO current.keys.len() - 1:
13.         results.push(current.offsets[i])
14.     current_id ← current.next

15. IF current_id == leaf_id:
16.     FOR i FROM 0 TO end - 1:
17.         results.push(leaf.offsets[i])

18. RETURN results
─────────────────────────────────────────────
```

### 6.4 Búsqueda por rango: `X < key < Y`

```
ALGORITHM BPlusTree_Range_Between(tree, X, Y)
─────────────────────────────────────────────
OUTPUT: Vec<u64> de offsets con X < clave < Y

1.  results ← []
2.  leaf_id ← Find_Leaf_Node(tree, tree.root, X)
3.  leaf ← tree.arena[leaf_id]
4.  start ← Insert_Position(leaf.keys, X)  // primer clave > X

5.  current_id ← leaf_id
6.  WHILE current_id IS NOT None:
7.     current ← tree.arena[current_id]
8.     FOR i FROM start TO current.keys.len() - 1:
9.         IF current.keys[i] < Y:
10.            results.push(current.offsets[i])
11.        ELSE:
12.            RETURN results
13.    start ← 0
14.    current_id ← current.next

15. RETURN results
─────────────────────────────────────────────
```

### 6.5 Búsqueda de la hoja más a la izquierda

```
ALGORITHM Find_Leftmost_Leaf(tree, node_id)
─────────────────────────────────────────────
1.  node ← tree.arena[node_id]
2.  WHILE node IS Internal:
3.      node_id ← node.children[0]
4.      node ← tree.arena[node_id]
5.  RETURN node_id
─────────────────────────────────────────────
```

## 7. Manejo de Claves Duplicadas

En una base de datos documental es común que múltiples documentos compartan el mismo valor en un campo indexado (ej. `{"ciudad": "Lima"}`).

**Política implementada (fase 1)**: reemplazo de offset. Si se inserta una clave duplicada, se actualiza el offset existente. Para soportar múltiples documentos con la misma clave, se recomienda una de dos estrategias:

1. **Posting list**: el offset en el leaf apunta a una estructura secundaria en disco que almacena todos los offsets de documentos con esa clave.
2. **Duplicados en hoja**: permitir entradas repetidas `(key, offset)` en el nodo hoja.

La implementación de referencia utiliza **reemplazo de offset** por simplicidad. La extensión a posting lists se documenta como mejora futura.

## 8. Recuperación de Documentos desde Disco

```
ALGORITHM Fetch_Document(file, offset)
─────────────────────────────────────────────
1.  file.seek(SeekFrom::Start(offset))
2.  header_bytes ← file.read(16)
3.  doc_size ← parse_u32_le(header_bytes[8..12])
4.  full_doc ← file.read(doc_size)
5.  RETURN Ldb_Deserialize(full_doc)
─────────────────────────────────────────────
```

## 9. Invariantes del Sistema

1. Todas las claves en un nodo hoja están ordenadas.
2. Todas las claves en un nodo interno están ordenadas.
3. Cada nodo interno tiene entre `⌈m/2⌉` y `m` hijos (excepto raíz).
4. Cada nodo hoja tiene entre `⌈(m-1)/2⌉` y `m-1` claves (excepto raíz).
5. Todas las hojas están al mismo nivel.
6. Las hojas forman una lista enlazada ordenada.
7. Los offsets apuntan a posiciones válidas dentro del archivo de datos.

## 10. Futuras Extensiones

- **Delete con merge/redistribución**: implementar borrado que mantenga invariantes de ocupación mínima.
- **Posting lists**: soportar múltiples offsets por clave duplicada.
- **Índices compuestos**: claves compuestas por múltiples campos.
- **Índices únicos**: rechazar inserciones duplicadas.
- **Persistencia del índice**: serializar el B+ Tree a disco para recuperación rápida.
