# Especificación de Persistencia del Índice B+ Tree — `index.ldb`

## 1. Introducción

Este documento define el formato binario para persistir el índice B+ Tree del motor documental **LDB** a un archivo llamado `index.ldb`. El índice vive en memoria como una arena de nodos (`Vec<Node>`) referenciados por `NodeId = usize`; este formato permite guardarlo y cargarlo de forma secuencial y eficiente.

## 2. Objetivos de diseño

- **Portabilidad**: el archivo debe ser legible en x86, x86_64 y ARM64 sin conversiones de tamaño.
- **Eficiencia de carga**: lectura secuencial, sin tablas de mapeo adicionales.
- **Compactación**: reutilizar la serialización de `LdbValue` ya definida para documentos.
- **Extensibilidad**: reservar espacio para flags, checksums y alineación futura.

## 3. Layout del archivo

```mermaid
packet-beta
    title index.ldb Layout
    0-3: "Magic 'IDX\\0'"
    4-5: "Version"
    6-7: "Flags"
    8-11: "HeaderSize"
    12-15: "NodeCount"
    16-19: "RootNodeId"
    20-23: "Order"
    24-31: "Checksum (CRC64 or 0)"
    32..: "Node blocks (sequential)"
```

## 4. Cabecera del archivo (32 bytes)

| Offset | Tamaño | Campo | Descripción |
|--------|--------|-------|-------------|
| 0 | 4 | Magic | `49 44 58 00` — cadena `"IDX\0"` |
| 4 | 2 | Version | `Mayor (1B) | Menor (1B)` — ej. `00 01` = v0.1 |
| 6 | 2 | Flags | Bit 0: checksum habilitado; resto reservado |
| 8 | 4 | HeaderSize | Tamaño de la cabecera (32 bytes) |
| 12 | 4 | NodeCount | Número total de nodos en el archivo |
| 16 | 4 | RootNodeId | Índice del nodo raíz (0-based) |
| 20 | 4 | Order | Orden `m` del B+ Tree |
| 24 | 8 | Checksum | CRC64 del contenido del archivo (o 0 si deshabilitado) |

## 5. Formato de cada bloque de nodo

Cada nodo comienza con un **Node Header** de 9 bytes:

| Offset en nodo | Tamaño | Campo | Descripción |
|----------------|--------|-------|-------------|
| 0 | 1 | NodeType | `0x01` = Internal, `0x02` = Leaf |
| 1 | 4 | NodeSize | Bytes totales del bloque (incluye este header) |
| 5 | 4 | KeyCount | Número de claves en el nodo |

### 5.1 Nodo Interno (`NodeType = 0x01`)

```mermaid
packet-beta
    title Internal Node Block
    0: "NodeType 0x01"
    1-4: "NodeSize u32 LE"
    5-8: "KeyCount u32 LE"
    9..: "Keys[] (LdbValue each)"
    after-keys: "Children[] u32 LE (KeyCount+1)"
```

Estructura:

```
[NodeType: 1B = 0x01]
[NodeSize: u32 LE]
[KeyCount: u32 LE]
[KeyCount × LdbValue serializado]       // claves de enrutamiento
[(KeyCount + 1) × NodeId u32 LE]        // punteros a hijos
```

Invariante: `children.len() == keys.len() + 1`.

### 5.2 Nodo Hoja (`NodeType = 0x02`)

```mermaid
packet-beta
    title Leaf Node Block
    0: "NodeType 0x02"
    1-4: "NodeSize u32 LE"
    5-8: "KeyCount u32 LE"
    9..: "Keys[] (LdbValue each)"
    after-keys: "Offsets[] u64 LE (KeyCount)"
    end-4: "NextLeaf u32 LE (0xFFFFFFFF = None)"
```

Estructura:

```
[NodeType: 1B = 0x02]
[NodeSize: u32 LE]
[KeyCount: u32 LE]
[KeyCount × LdbValue serializado]       // claves indexadas
[KeyCount × Offset u64 LE]              // offsets al archivo data.ldb
[NextLeaf: u32 LE]                      // 0xFFFFFFFF = None
```

Invariante: `offsets.len() == keys.len()`.

## 6. Serialización de `LdbValue` dentro del índice

Se reutiliza el formato LDB del documento, pero **sin clave** (solo el valor):

```
[Tag: 1B] [Value bytes según tipo]
```

| Tag | Tipo | Bytes de valor |
|-----|------|----------------|
| `0x01` | Int32 | 4 bytes, signed, little-endian |
| `0x02` | Int64 | 8 bytes, signed, little-endian |
| `0x03` | Float64 | 8 bytes, IEEE 754, little-endian |
| `0x04` | String | 4 bytes `u32` LE (longitud) + N bytes UTF-8 |
| `0x05` | Boolean | 1 byte: `0x00` false, `0x01` true |
| `0x06` | Sub-document | 4 bytes `u32` LE (longitud) + body + `0xFF` |
| `0x07` | Null | 0 bytes |

## 7. Algoritmos

### 7.1 Persistencia

```
ALGORITHM SaveIndex(tree, path)
─────────────────────────────────
1.  Abrir archivo para escritura binaria.
2.  Escribir cabecera con:
        Magic = "IDX\0"
        Version = 0.1
        Flags = 1 (checksum habilitado por defecto)
        HeaderSize = 32
        NodeCount = tree.arena.nodes.len()
        RootNodeId = tree.root
        Order = tree.order
        Checksum = 0 (placeholder)
3.  Para cada nodo i en tree.arena.nodes (en orden):
4.      Serializar nodo i como bloque y escribirlo.
5.  Calcular CRC64 de todo el archivo y escribirlo en bytes 24-31.
6.  Cerrar archivo.
─────────────────────────────────
```

### 7.2 Carga

```
ALGORITHM LoadIndex(path) -> BPlusTree
─────────────────────────────────
1.  Leer 32 bytes de cabecera.
2.  Validar magic y versión.
3.  Leer NodeCount, RootNodeId, Order.
4.  Si checksum habilitado: verificar CRC64.
5.  Crear NodeArena con capacidad NodeCount.
6.  Para i de 0 a NodeCount-1:
7.      Leer NodeType, NodeSize, KeyCount.
8.      Si NodeType == 0x01: deserializar InternalNode.
9.      Si NodeType == 0x02: deserializar LeafNode.
10.     Añadir nodo a arena.
11. Construir BPlusTree { arena, root: RootNodeId, order: Order }.
12. Retornar árbol.
─────────────────────────────────
```

## 8. Decisiones de diseño

1. **`NodeId` como `u32` en disco**: garantiza portabilidad entre plataformas. 4.294.967.295 nodos es suficiente para un índice embebido.
2. **Nodos secuenciales**: al escribir la arena en orden, los `NodeId` en disco coinciden con los índices en memoria al cargar. No se requiere tabla de mapeo.
3. **`NodeSize` por nodo**: permite validación de límites y facilita futura alineación a páginas.
4. **Sentinel `0xFFFFFFFF` para `None`**: representa `next: None` en nodos hoja.
5. **Checksum CRC64 opcional**: controlado por el bit 0 de `Flags`. Habilitado por defecto.

## 9. Reglas de validación

1. Magic debe ser exactamente `49 44 58 00`.
2. Version debe ser `00 01` (v0.1).
3. NodeCount debe coincidir con la cantidad de bloques leídos.
4. RootNodeId debe ser menor que NodeCount.
5. NodeType debe ser `0x01` o `0x02`.
6. NodeSize debe coincidir con los bytes consumidos por el bloque.
7. KeyCount debe ser consistente con el tipo de nodo.
8. Si checksum habilitado, CRC64 debe coincidir.

## 10. Futuras extensiones

- Alineación de bloques a 4KB para I/O de página.
- Compresión de bloques de nodo.
- Checksum por bloque además del checksum global.
- WAL para actualizaciones incrementales del índice.