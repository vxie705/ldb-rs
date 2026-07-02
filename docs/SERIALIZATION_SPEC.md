# Especificación del Formato Binario LDB

## 1. Introducción

LDB (LSLS Document Binary) es el formato de serialización binaria del motor de bases de datos documental embebido **LDB**. Está inspirado en BSON pero optimizado para motores embebidos con cabecera auto-descriptiva, sin padding y con terminador explícito.

- **Endianness**: Little-Endian (nativo en x86_64/ARM64).
- **Alineación**: Sin padding (`packed`).
- **Strings**: UTF-8 sin terminador nulo.
- **Claves**: UTF-8 con prefijo de longitud de 1 byte (`u8`), máximo 255 bytes.

## 2. Cabecera del Documento (16 bytes)

| Offset | Tamaño | Campo        | Descripción                                      |
|--------|--------|--------------|--------------------------------------------------|
| 0      | 4      | Magic        | `4C 44 42 00` — cadena `"LDB\0"`               |
| 4      | 2      | Version      | `Mayor (1B) | Menor (1B)` — ej. `00 01` = v0.1   |
| 6      | 2      | Flags        | Reservado para bit-flags futuros (compresión, checksum) |
| 8      | 4      | DocSize      | `u32` LE — tamaño total en bytes (header + body + trailer) |
| 12     | 4      | FieldCount   | `u32` LE — número de campos de primer nivel     |

## 3. Type Tags (1 byte)

| Tag  | Tipo         | Valor almacenado                                          |
|------|--------------|-----------------------------------------------------------|
| 0x01 | Int32        | 4 bytes, signed, little-endian                            |
| 0x02 | Int64        | 8 bytes, signed, little-endian                            |
| 0x03 | Float64      | 8 bytes, IEEE 754 binary64, little-endian                 |
| 0x04 | String       | 4 bytes `u32` LE (longitud) + N bytes UTF-8               |
| 0x05 | Boolean      | 1 byte: `0x00` false, `0x01` true                         |
| 0x06 | Sub-document | 4 bytes `u32` LE (longitud) + body anidado + `0xFF`       |
| 0x07 | Null         | 0 bytes                                                   |
| 0xFF | End-of-Document | Marcador de fin del documento                          |

Rango `0x08–0xFE` reservado para tipos futuros (Array, DateTime, Binary, ObjectId, etc.).

## 4. Estructura de un Field Entry

Cada campo del body tiene la siguiente estructura:

```
[type_tag: 1B] [key_len: 1B] [key_bytes: key_len B] [value: según tipo]
```

## 5. Representación Detallada por Tipo

### 5.1 Int32 — tag `0x01`

```
01 | LL | K...K | VV VV VV VV
```

- `LL`: longitud de la clave (1 byte).
- `K...K`: bytes UTF-8 de la clave.
- `VV VV VV VV`: valor entero de 32 bits con signo, little-endian.

### 5.2 Int64 — tag `0x02`

```
02 | LL | K...K | VV VV VV VV VV VV VV VV
```

### 5.3 Float64 — tag `0x03`

```
03 | LL | K...K | VV VV VV VV VV VV VV VV
```

Representación IEEE 754 binary64, little-endian.

### 5.4 String — tag `0x04`

```
04 | LL | K...K | NN NN NN NN | UTF-8 bytes...
```

- `NN NN NN NN`: longitud en bytes del string valor, `u32` LE.
- No incluye terminador nulo.

### 5.5 Boolean — tag `0x05`

```
05 | LL | K...K | VV
```

- `VV`: `0x00` = false, `0x01` = true.

### 5.6 Sub-document — tag `0x06`

```
06 | LL | K...K | NN NN NN NN | <body anidado> | FF
```

- `NN NN NN NN`: longitud total del sub-documento (body + trailer `0xFF`), `u32` LE.
- El sub-documento **no repite la cabecera de 16 bytes**; es solo `body + 0xFF` para evitar overhead.

### 5.7 Null — tag `0x07`

```
07 | LL | K...K
```

Sin bytes de valor.

## 6. Ejemplo: `{"edad": 21, "activo": true}`

### Cálculo de tamaños

| Sección        | Bytes |
|----------------|-------|
| Header         | 16    |
| `"edad" → 21`  | 10    |
| `"activo" → true` | 9  |
| Trailer `0xFF` | 1     |
| **Total**      | **36 (0x24)** |

### Hex dump completo

```
Offset  00 01 02 03  04 05 06 07  08 09 0A 0B  0C 0D 0E 0F
──────  ──────────  ──────────  ──────────  ──────────
0x00    4C 44 42 00  00 01 00 00  24 00 00 00  02 00 00 00
0x10    01 04 65 64  61 64 15 00  00 00 05 06  61 63 74 69
0x20    76 6F 01 FF
```

### Desglose

```
4C 44 42 00              Magic: "LDB\0"
00 01                    Version: 0.1
00 00                    Flags: 0
24 00 00 00              DocSize: 36 bytes
02 00 00 00              FieldCount: 2

01                       Type tag: Int32
04                       Key length: 4
65 64 61 64              Key: "edad"
15 00 00 00              Value: 21 (0x15 LE)

05                       Type tag: Boolean
06                       Key length: 6
61 63 74 69 76 6F        Key: "activo"
01                       Value: true

FF                       End-of-Document
```

## 7. Reglas de Validación

1. Los primeros 4 bytes deben ser exactamente `4C 44 42 00`.
2. `DocSize` debe coincidir con la cantidad real de bytes leídos.
3. `FieldCount` debe coincidir con el número de field entries antes del trailer `0xFF`.
4. Cada `key_len` debe ser > 0 y ≤ 255.
5. Las claves deben ser UTF-8 válidas.
6. El trailer debe ser `0xFF`.
7. Para Boolean, solo se aceptan `0x00` y `0x01`.
8. Para Sub-document, `NN` debe coincidir con la longitud real del body anidado + trailer.

## 8. Extensibilidad

- Nuevos type tags se agregan en el rango `0x08–0xFE` sin romper compatibilidad.
- Los bits de `Flags` permiten activar compresión o checksums en versiones futuras.
