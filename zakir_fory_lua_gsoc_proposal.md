# Apache Fory Lua Xlang Serialization — GSoC 2026 Proposal

**Project**: Apache Fory Lua Serialization
**Organization**: Apache Software Foundation
**Issue**: [#3380](https://github.com/apache/fory/issues/3380)
**Full name**: Zakir Shaik
**Email**: zakir03032002@gmail.com
**Location**: Hyderabad, India
**GitHub**: https://github.com/Zakir032002
**Mentors**: Chaokun Yang (chaokunyang@apache.org), Weipeng Wang (dev@fory.apache.org)
**Size**: ~350 hours (Large)
**Difficulty**: Hard

---

## About Me

I am a B.Tech Computer Science graduate with hands-on experience in systems programming across Rust, C++, Python, and Lua. My work spans serialization internals, binary protocol implementation, and high-performance data systems.

**Open-source contributions:**

- **Apache KVRocks (C++)** — merged PR [#3366](https://github.com/apache/kvrocks/pull/3366): RDB serialization for SortedInt type; open PRs for JSON DUMP ([#3368](https://github.com/apache/kvrocks/pull/3368)) and HyperLogLog DUMP ([#3371](https://github.com/apache/kvrocks/pull/3371))
- **Apache Fory (Rust)** — open PRs for streaming deserialization ([#3369](https://github.com/apache/fory/pull/3369)), configurable size guardrails ([#3421](https://github.com/apache/fory/pull/3421)), and foryc CI binary builds ([#3385](https://github.com/apache/fory/pull/3385))
- **RisingWave Labs (Rust)** — open PR [#25021](https://github.com/risingwavelabs/risingwave/pull/25021): writer-style anymap/vector functions achieving 6–12× speedup by eliminating per-row heap allocations

**Personal projects:**

- **Fault-Tolerant Raft KV Store (Rust)** — production-grade distributed key-value storage engine utilizing Raft consensus and RocksDB-backed persistence; features log compaction, state machine snapshotting, and consistent hashing for horizontal scaling.
- **ML Inference Server (Rust)** — high-performance ONNX-based inference engine with gRPC APIs; implements dynamic batching, session pooling, and backpressure control for Docker-orchestrated deployments.

---

## Project Synopsis

Apache Fory is a blazingly fast multi-language serialization framework powered by JIT compilation, code generation, and zero-copy techniques — delivering up to 170× faster performance than alternatives. It currently supports Java, Python, C++, Go, Rust, JavaScript, C#, Swift, Dart, Kotlin, and Scala. However, **Lua has no runtime implementation**.

This project delivers a **complete Lua xlang serialization runtime** — protocol-correct, wire-compatible with all existing Fory runtimes. The implementation follows the [Xlang Serialization Spec](https://github.com/apache/fory/blob/main/docs/specification/xlang_serialization_spec.md) and the [Implementation Guide](https://github.com/apache/fory/blob/main/docs/specification/xlang_implementation_guide.md) precisely.

The Lua runtime will serialize and deserialize Lua tables, enums, and unions into Fory's binary xlang format — enabling **direct interoperability between Lua applications and Java/Python/Go/Rust/C++ services** without any IDL generation or schema compilation step.

**Impact**: Lua is the dominant embedded scripting language in gaming (Roblox, World of Warcraft, Defold), networking (OpenResty/Nginx), IoT (NodeMCU), and configuration (Redis, HAProxy). Adding Fory support unlocks high-performance cross-language serialization for these ecosystems. A Java game server can exchange structured objects with a Lua game client at zero-copy speeds — no protobuf compilation, no JSON parsing overhead.

**Public API** (the deliverable surface):

```lua
local Fory = require("fory")

-- Create instance
local f = Fory.new({ xlang = true, compatible = true, ref_tracking = false })

-- Register types
f:register_struct(PlayerStruct, 100)              -- by numeric ID
f:register_struct(ItemStruct, "game", "item")     -- by namespace + name
f:register_enum(Color, 101)

-- Serialize / Deserialize
local bytes = f:serialize(obj)
local result = f:deserialize(bytes)
```

---

## Problem Deep Dive

### Current State — Based on Actual Codebase Analysis

I have read the xlang serialization spec (1462 lines), the implementation guide (258 lines), the xlang type mapping spec, `XlangTestBase.java` (2904 lines), `GoXlangTest.java` (500 lines), the Go `xlang_test_main.go` peer binary (2588 lines), the Python runtime `_fory.py`, and the repository structure for all 11 existing language implementations.

Here is the verified state of the Fory ecosystem relevant to this project:

| Component | Status | Evidence |
|---|---|---|
| **Xlang binary spec** | ✅ Complete | `docs/specification/xlang_serialization_spec.md` — 1462 lines covering header, ref meta, type meta, TypeDef, meta string, all value formats |
| **Implementation guide** | ✅ Complete | `docs/specification/xlang_implementation_guide.md` — 8-phase checklist for new languages |
| **XlangTestBase** | ✅ Complete | `java/fory-core/src/test/java/org/apache/fory/xlang/XlangTestBase.java` — 2904 lines with 30+ test cases |
| **Peer test pattern** | ✅ Established | Each language (Go, Rust, Python, C++, Swift, C#, Dart, JS) has: (1) `*XlangTest.java` extending `XlangTestBase`, (2) a peer binary/script that reads/writes `DATA_FILE`, (3) CI env var gating |
| **Lua runtime** | ❌ Does not exist | No `lua/` directory. No `LuaXlangTest.java`. No CI workflow for Lua. |

### Verified Protocol Elements the Lua Runtime Must Implement

From reading the spec cover-to-cover:

- **Header**: 1-byte bitmap — bit 0 (null), bit 1 (xlang), bit 2 (oob). All data little-endian.
- **Reference flags**: `NULL_FLAG(-3/0xFD)`, `REF_FLAG(-2/0xFE)` + varuint32 ref_id, `NOT_NULL_VALUE_FLAG(-1/0xFF)`, `REF_VALUE_FLAG(0/0x00)`. Sequential ID assignment starting from 0.
- **Type IDs**: Internal 8-bit IDs (0–56 defined). User types write internal type ID then `user_type_id` as varuint32 separately — no bit packing.
- **Varint encoding**: unsigned varint32/64 using MSB continuation; signed varint32/64 using ZigZag then unsigned varint. Tagged int64 using 4-or-9-byte hybrid encoding.
- **String format**: `varuint36_small((byte_length << 2) | encoding)` then raw bytes. Encodings: LATIN1(0), UTF16(1), UTF8(2).
- **Meta strings**: 5 encoding types (UTF8, LOWER_SPECIAL, LOWER_UPPER_DIGIT_SPECIAL, FIRST_TO_LOWER_SPECIAL, ALL_TO_LOWER_SPECIAL) with dedup via `(length << 1) | flag` marker.
- **Collection/list**: `varuint32(length)` + 1-byte elements header (bits: track_ref, has_null, is_decl_elem_type, is_same_type) + optional type info + elements.
- **Map**: `varuint32(total_size)` + chunk-based format (1-byte KV header + 1-byte chunk_size + N key-value pairs per chunk, max 255 pairs/chunk).
- **Struct field order**: 6-group classification (primitive non-nullable → primitive nullable → built-in → collection → map → other), with intra-group sorting by compression category, size descending, type ID, field identifier lexicographic.
- **TypeDef**: 8-byte global header (low 8 bits meta size, bit 8 HAS_FIELDS_META, bit 9 COMPRESS_META, high 50 bits hash) + body (meta header byte with num_fields, REGISTER_BY_NAME flag, type spec, field info list).
- **Shared TypeDef streaming**: `marker = (index << 1) | flag` — flag 0 = new definition follows, flag 1 = reference to previous.
- **Union**: `case_id(varuint32)` + `case_value` encoded as full xlang value (ref meta + type meta + value).

### Why Lua is Non-Trivial

1. **Lua has a single `table` type** for arrays, maps, and objects. The serializer must use declared type descriptors to disambiguate whether a table should serialize as LIST, MAP, or STRUCT. This is fundamentally different from Go (which has distinct `slice`, `map`, `struct`) or Python (which has `list`, `dict`, classes).

2. **Lua numbers are floating-point by default** (double in Lua 5.3+, with optional integer subtype via `math.type()`). The serializer must correctly handle int8/int16/int32/int64/float32/float64 type distinctions using Lua 5.3+ integer support or explicit wrapper types.

3. **Lua 5.3+ has native 64-bit integers** but they share the `number` type. The runtime must use `math.type(x)` to distinguish `"integer"` vs `"float"` for correct type ID selection, and must handle the full int64 range including `math.mininteger` / `math.maxinteger`.

4. **Metatable restoration** is critical for deserialized structs to be usable. Unlike Go/Rust where struct types are statically resolved, Lua objects get their behavior from metatables. The deserializer must: resolve the type descriptor from type meta → call `create_instance()` → set metatable immediately → reserve reference slot → populate fields. This enables circular reference support.

5. **No native binary buffer** — Lua has `string.pack`/`string.unpack` (5.3+) for binary encoding, but no mutable buffer with cursor tracking. A custom `Buffer` class must be built with auto-growth, little-endian reads/writes, and efficient byte slicing.

6. **Cross-language field ordering** must exactly match the spec's 6-group deterministic ordering algorithm. The Lua implementation must compute identical field order to Java/Go/Rust/Python for every struct, or schema-consistent mode will produce incompatible wire bytes.

---

## System Architecture

### Repository Layout

```
lua/                                    ← NEW top-level directory
├── fory/
│   ├── init.lua                        ← Public API: Fory.new(config)
│   ├── buffer.lua                      ← Mutable byte buffer with LE read/write
│   ├── varint.lua                      ← Varint32/64, ZigZag, tagged int64
│   ├── header.lua                      ← Fory header bitmap read/write
│   ├── ref_resolver.lua                ← Reference tracking (write/read)
│   ├── type_registry.lua               ← Type registration (numeric ID + named)
│   ├── type_resolver.lua               ← Type ID dispatch → serializer lookup
│   ├── meta_string.lua                 ← Meta string encode/decode + dedup
│   ├── type_def.lua                    ← TypeDef encode/decode + shared streaming
│   ├── murmur3.lua                     ← MurmurHash3 x64_128 (for schema hash)
│   ├── serializers/
│   │   ├── primitive.lua               ← bool, int8–int64, float32/64
│   │   ├── string.lua                  ← String with LATIN1/UTF16/UTF8
│   │   ├── temporal.lua                ← Duration, Timestamp, Date
│   │   ├── collection.lua              ← List/Set with elements header
│   │   ├── map.lua                     ← Map with chunk-based KV format
│   │   ├── enum.lua                    ← Enum ordinal serialization
│   │   ├── struct.lua                  ← Struct with field ordering + schema hash
│   │   ├── union.lua                   ← Union with case_id + Any-style payload
│   │   ├── binary.lua                  ← Binary/array types
│   │   └── skip.lua                    ← Skip unknown fields/types
│   └── types.lua                       ← Type ID constants (0–56)
├── tests/
│   ├── test_buffer.lua
│   ├── test_varint.lua
│   ├── test_meta_string.lua
│   ├── test_type_def.lua
│   ├── test_serialization.lua
│   └── xlang/
│       └── xlang_test_main.lua         ← Peer binary for XlangTestBase
├── rockspec/
│   └── fory-scm-1.rockspec
└── README.md

java/fory-core/src/test/java/org/apache/fory/xlang/
    └── LuaXlangTest.java               ← NEW: extends XlangTestBase

.github/workflows/ci.yml                ← MODIFY: add Lua xlang CI job
```

### Component Interaction Diagram

```
┌──────────────────────────────────────────────────────────────────┐
│                     Lua Application Layer                        │
│  local f = Fory.new({ xlang=true, compatible=true })             │
│  f:register_struct(MyStruct, 100)                                │
│  local bytes = f:serialize(obj)   /   f:deserialize(bytes)       │
└────────────────────┬─────────────────────────────────────────────┘
                     ▼
┌──────────────────────────────────────────────────────────────────┐
│                    Fory Core (init.lua)                          │
│  serialize(obj):                    deserialize(bytes):          │
│  1. Write header bitmap             1. Read header bitmap        │
│  2. Write ref_or_null               2. Read ref_or_null          │
│  3. Write type meta                 3. Read type meta            │
│  4. Dispatch to serializer          4. Lookup serializer         │
│  5. Write value                     5. Read value                │
└────────┬──────────────────────────────────────┬──────────────────┘
         │                                      │
  ┌──────▼──────┐   ┌───────────────┐   ┌──────▼──────────┐
  │   Buffer    │   │  RefResolver  │   │  TypeRegistry   │
  │ write_int8  │   │ write_ref_    │   │ • by_id: {}     │
  │ write_le32  │   │   or_null     │   │ • by_name: {}   │
  │ read_le64   │   │ read_ref_     │   │ • register()    │
  │ varint32    │   │   or_null     │   │ • resolve()     │
  └─────────────┘   └───────────────┘   └─────────────────┘
         │
  ┌──────▼────────────────────────────────────────────────┐
  │               Serializers                             │
  │  Prim. │ String │ Coll. │ Map │ Struct │ Enum │ Union │
  └───────────────────────────────────────────────────────┘
         │
  ┌──────▼────────────────────────────────────────────────┐
  │               Meta Layer                              │
  │  MetaString (encode/decode/dedup)                     │
  │  TypeDef    (encode/decode/shared streaming)          │
  │  MurmurHash3 x64_128                                  │
  └───────────────────────────────────────────────────────┘
```

### Lua-Specific Type Mapping

| Fory Type | Type ID | Lua Type | Notes |
|---|---|---|---|
| bool | 1 | `boolean` | Direct mapping |
| int8 | 2 | `integer` | Range-checked via wrapper |
| int16 | 3 | `integer` | Range-checked via wrapper |
| int32 | 4 | `integer` | `math.type(x) == "integer"` |
| varint32 | 5 | `integer` | ZigZag + varint encoding |
| int64 | 6 | `integer` | Lua 5.3+ native 64-bit |
| varint64 | 7 | `integer` | ZigZag + varint encoding |
| tagged_int64 | 8 | `integer` | Hybrid 4-or-9 byte |
| float32 | 19 | `number` | Via `string.pack("<f")` |
| float64 | 20 | `number` | `math.type(x) == "float"` |
| string | 21 | `string` | UTF-8 native in Lua |
| list | 22 | `table` (array-like) | Declared type descriptor |
| set | 23 | `table` (set-like) | Keys=elements, values=true |
| map | 24 | `table` (dict-like) | Declared type descriptor |
| enum | 25 | `integer` | Ordinal value |
| struct | 27 | `table` + metatable | Metatable restoration |
| binary | 41 | `string` | Lua strings are byte arrays |

### Technology Choices & Trade-offs

| Decision | Choice | Rationale |
|---|---|---|
| Lua version | Lua 5.3/5.4 | Native 64-bit integers (`math.type`), `string.pack`/`string.unpack` for binary encoding — essential for protocol correctness |
| Buffer impl | Pure Lua with `string.pack`/`string.unpack` | Portable across all Lua 5.3+ environments; no C dependency |
| LuaJIT fast path | Optional, not required | Pure Lua is canonical; LuaJIT FFI can accelerate buffer ops for ~2–5× speedup but must not change wire format |
| Table disambiguation | Declared type descriptor required | Lua tables are untyped; descriptor carries field list, type IDs, and metatable for struct reconstruction |
| Int64 handling | Native Lua 5.3+ integer | Full 64-bit range without wrappers; `math.mininteger` to `math.maxinteger` |
| Set representation | `table` with `{[elem]=true}` | Standard Lua idiom; no native set type |
| String encoding | Always write UTF-8 | Lua strings are byte sequences; UTF-8 is the natural encoding. Read supports LATIN1/UTF16/UTF-8 |
| Schema hash | MurmurHash3 x64_128 in pure Lua | Required for struct version checking; must produce identical hash to Java/Go/Rust implementations |

---

## Flow Diagrams

### Flow 1: Serialize Lifecycle (Lua)

```
Lua Application
    │
    │  f:serialize(obj)
    ▼
┌─────────────────────────────────────────────────┐
│ Fory.serialize(obj)                              │
│                                                  │
│  1. buffer = Buffer.new(256)                     │
│  2. Write header: 0x02 (xlang=1, null=0)         │
│  3. ref_resolver:write_ref_or_null(buffer, obj)  │
│     ├── obj is nil?                              │
│     │     → write(-3), return                   │
│     ├── ref tracking ON and seen?                │
│     │     → write(-2) + varuint32(ref_id), ret  │
│     ├── ref tracking ON and new?                 │
│     │     → write(0), assign ref_id, continue   │
│     └── ref tracking OFF?                        │
│           → write(-1), continue                  │
│  4. Resolve type → get serializer                │
│  5. Write type meta:                             │
│     ├── Internal type?   → varuint32(type_id)   │
│     ├── User type by ID? → varuint32(internal)  │
│     │                    → varuint32(user_id)   │
│     └── Named type?      → varuint32(NAMED_*id) │
│           ├── meta share OFF? → meta strings    │
│           └── meta share ON?  → shared TypeDef  │
│  6. serializer:write(buffer, obj)                │
│  7. return buffer:to_bytes()                     │
└─────────────────────────────────────────────────┘
```

### Flow 2: Struct Serialization Detail

```
struct_serializer:write(buffer, obj)
    │
    ├── Schema-Consistent Mode:
    │   │
    │   ├── 1. Compute field order (6-group algorithm)
    │   │      ├── Group 1: primitive non-nullable
    │   │      ├── Group 2: primitive nullable
    │   │      ├── Group 3: built-in non-container
    │   │      ├── Group 4: collection (list/set)
    │   │      ├── Group 5: map
    │   │      └── Group 6: other (enum/struct/ext)
    │   │
    │   ├── 2. Write 4-byte schema hash (if version check)
    │   │      └── MurmurHash3 of "<field_id>,<type_id>,<ref>,<nullable>;"
    │   │
    │   └── 3. For each field in Fory order:
    │          ├── Primitive?          → write raw value
    │          ├── Nullable primitive? → write null flag + value
    │          └── Reference type?    → write ref/null flag + type meta + value
    │
    └── Compatible Mode (meta share):
        ├── 1. Write shared TypeDef marker:
        │      ├── First time? → (index << 1) | 0 + TypeDef bytes
        │      └── Seen before? → (index << 1) | 1
        ├── 2. Write fields in Fory order (same as above)
        └── 3. Deserializer matches fields by name/tag,
               skips unknown fields via skip_value()
```

### Flow 3: Metatable Restoration During Deserialization

```
struct_serializer:read(buffer)
    │
    ├── 1. Read type meta → resolve type descriptor
    │      ├── Numeric ID → type_registry.by_id[user_type_id]
    │      └── Named      → type_registry.by_name[namespace..type_name]
    │
    ├── 2. descriptor.create_instance()
    │      └── Returns new table with metatable already set  ← CRITICAL
    │
    ├── 3. ref_resolver:reserve_slot(instance)
    │      └── Enables circular reference support
    │
    ├── 4. Read fields:
    │      ├── Schema-consistent: read in Fory order, assign by position
    │      └── Compatible: read TypeDef, match by name/tag, skip unknown
    │
    ├── 5. ref_resolver:fill_slot(instance)
    │
    └── 6. Return instance (table with metatable + populated fields)
```

### Flow 4: Cross-Language Round-Trip Test

```
┌───────────────────────────────────────────────────────────────┐
│  XlangTestBase.testSimpleStruct()                             │
│                                                               │
│  Java:                                                        │
│    Fory fory = Fory.builder()                                 │
│      .withLanguage(Language.XLANG)                            │
│      .withCompatibleMode(CompatibleMode.COMPATIBLE)           │
│      .build();                                                │
│    fory.register(Color.class, 101);                           │
│    fory.register(Item.class, 102);                            │
│    fory.register(SimpleStruct.class, 103);                    │
│    fory.serialize(buffer, obj);  // Write to DATA_FILE        │
│                                                               │
│                    ▼ DATA_FILE (binary bytes)                 │
│                                                               │
│  Lua peer (xlang_test_main.lua):                              │
│    local f = Fory.new({xlang=true, compatible=true})          │
│    f:register_enum(Color, 101)                                │
│    f:register_struct(Item, 102)                               │
│    f:register_struct(SimpleStruct, 103)                       │
│    local obj = f:deserialize(data)   -- verify fields         │
│    local out = f:serialize(obj)      -- re-serialize          │
│    write_file(DATA_FILE, out)        -- write back            │
│                                                               │
│                    ▼ DATA_FILE (re-serialized)                │
│                                                               │
│  Java:                                                        │
│    MemoryBuffer buffer2 = readBuffer(ctx.dataFile());         │
│    Assert.assertEquals(fory.deserialize(buffer2), obj);       │
└───────────────────────────────────────────────────────────────┘
```

---

## Implementation Plan

### Phase 1 — Core Infrastructure (Weeks 1–3)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 1.1 | `buffer.lua` — Mutable byte buffer with LE read/write, auto-growth, cursor tracking | All `read_*`/`write_*` for int8–int64, float32/64 using `string.pack`/`string.unpack` | Efficient concatenation-based growth in pure Lua; correct LE byte order |
| 1.2 | `varint.lua` — varuint32/64, varint32/64 (ZigZag), tagged_int64, varuint36_small | All varint codecs passing boundary value tests | int64 ZigZag with Lua's signed integer arithmetic; MSB continuation loop |
| 1.3 | `murmur3.lua` — MurmurHash3 x64_128 in pure Lua | Identical hashes to Java/Go implementations for all test vectors | 64-bit multiplication and rotation in Lua 5.3 integers |
| 1.4 | `header.lua` — Fory header bitmap read/write | Correct handling of null/xlang/oob flags | Simple but must match spec exactly |
| 1.5 | `ref_resolver.lua` — Write-side object tracking (identity → ref_id) and read-side (ref_id → object) | All 4 reference flags correctly handled | Lua table identity tracking via `rawequal`; sequential ID assignment |
| 1.6 | `types.lua` — Type ID constants (0–56) | Complete type ID table matching spec | Exact numeric values from spec |
| 1.7 | Buffer + varint cross-language test | Pass `test_buffer`, `test_buffer_var`, `test_murmurhash3` from XlangTestBase | First cross-language validation |

### Phase 2 — Primitive & String Serializers (Weeks 4–5)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 2.1 | `serializers/primitive.lua` — bool, int8–int64, float32/64 with type ID dispatch | All primitive round-trips passing | `math.type()` for integer/float distinction; range validation |
| 2.2 | `serializers/string.lua` — String header `(byte_length << 2) \| encoding` + UTF-8 write, LATIN1/UTF16/UTF8 read | String interop with Java/Python | UTF-16 decode for strings written by Java; LATIN1 decode for Latin chars |
| 2.3 | `serializers/temporal.lua` — Duration (varint64 seconds + int32 nanos), Timestamp (int64 + uint32), Date (int32 days) | Temporal type interop | Nanosecond normalization for negative timestamps |
| 2.4 | `serializers/binary.lua` — Binary as byte string, primitive arrays | Array/binary interop | LE byte reordering for int16/32/64 arrays if needed |
| 2.5 | Cross-language test: `test_cross_language_serializer`, `test_string_serializer` | Primitives + strings passing Java↔Lua | Full primitive coverage including boundary values |

### Phase 3 — Collections & Maps (Weeks 6–7)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 3.1 | `serializers/collection.lua` — List with elements header bits, type info optimization | List interop passing | Elements header bit logic (is_same_type, is_decl_elem_type, has_null, track_ref); type-per-element vs shared type |
| 3.2 | `serializers/map.lua` — Chunk-based map format with KV header | Map interop passing | Chunk-based write with max 255 pairs; separate null key/value chunks; KV header bit packing |
| 3.3 | Set serialization (reuse list format) | Set interop passing | Lua set `{[k]=true}` → list wire format → set reconstruction |
| 3.4 | Cross-language test: `test_list`, `test_map` | Collections passing Java↔Lua | Null element handling; polymorphic element support |

### Phase 4 — Meta String & TypeDef (Weeks 8–9)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 4.1 | `meta_string.lua` — All 5 meta string encoding types + encoding selection algorithm | Correct meta string encoding/decoding | Bit-packing for 5-bit and 6-bit character codes; padding logic; escape for ALL_TO_LOWER_SPECIAL |
| 4.2 | Meta string deduplication | Dedup via `(length << 1) \| flag` marker | Hash-based dedup for large strings (>16 bytes); exact match for small strings |
| 4.3 | `type_def.lua` — TypeDef encoding: 8-byte global header + body | TypeDef round-trip passing | 50-bit hash extraction from uint64 header; field info encoding with header byte layout |
| 4.4 | Shared TypeDef streaming — `(index << 1) \| flag` marker for new vs reference | Shared meta passing in compatible mode | Per-stream meta context map; sequential index assignment |

### Phase 5 — Enum, Struct & Union (Weeks 10–12)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 5.1 | `serializers/enum.lua` — Ordinal varuint32, named enum support | Enum interop passing | Named enum via meta strings when no numeric ID registered |
| 5.2 | `serializers/struct.lua` — Schema-consistent mode: field ordering + schema hash + field serialization | Struct interop (schema-consistent) | 6-group field ordering algorithm must match Java/Go/Rust exactly; MurmurHash3 schema fingerprint |
| 5.3 | `serializers/struct.lua` — Compatible mode: TypeDef + field-by-name matching + unknown field skip | Struct interop (compatible) | TypeDef field name matching; skip unknown fields safely; metatable restoration |
| 5.4 | `serializers/union.lua` — Union with case_id + Any-style payload | Union interop passing | Reading/writing case_value as full xlang value for skip support |
| 5.5 | `serializers/skip.lua` — Skip value for all type IDs | Unknown field/union alternative skip | Must handle every type ID to consume correct byte count |
| 5.6 | Full cross-language test suite: all XlangTestBase tests green | All 30+ test cases from XlangTestBase passing | Schema evolution, nullable field, ref tracking, polymorphic collection tests |

### Phase 6 — Testing, CI & Docs (Weeks 13–14)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 6.1 | `LuaXlangTest.java` — Java test runner extending XlangTestBase | All XlangTestBase test methods overridden and passing | Test method delegation pattern matching GoXlangTest/RustXlangTest |
| 6.2 | Python↔Lua interop tests | Lua serialize → Python deserialize, Python serialize → Lua deserialize | Same wire format verification against second runtime |
| 6.3 | Negative tests — malformed varint, unknown type ID, truncated payload, malformed TypeDef | Graceful error handling for all malformed inputs | Error recovery without crash or infinite loop |
| 6.4 | CI integration — GitHub Actions workflow with `FORY_LUA_JAVA_CI=1` | CI pipeline running all Lua xlang tests automatically | Lua 5.4 installation in CI; artifact caching |
| 6.5 | Lua lint integration | `luacheck` running in CI with config | Clean code style |
| 6.6 | Documentation — `docs/guide/lua_guide.md` + README | Usage guide with registration examples, type mapping table, compatible mode instructions | Clear documentation for end users |
| 6.7 | Performance baseline — pure Lua benchmark report | Benchmark numbers for primitive/struct/collection serialization | Establish baseline for future LuaJIT optimization |

---

## Timeline (Week-by-Week)

| Week | Phase | Activities | Deliverables |
|---|---|---|---|
| **1** | Infrastructure | Buffer, varint, types.lua, header | PR #1: buffer + varint + types passing unit tests |
| **2** | Infrastructure | MurmurHash3, ref_resolver | MurmurHash3 matching Java/Go; ref resolver complete |
| **3** | Infrastructure | First xlang tests: test_buffer, test_buffer_var, test_murmurhash3 | PR #2: LuaXlangTest passing 3 buffer/hash tests |
| **4** | Primitives | Primitive serializers, string serializer | All primitives round-tripping |
| **5** | Primitives | Temporal types, binary, test_cross_language_serializer, test_string_serializer | PR #3: primitives + strings passing Java↔Lua |
| **6** | Collections | List/set serializer with elements header | List tests passing |
| **7** | Collections | Map serializer with chunk format, test_list, test_map | PR #4: collections passing Java↔Lua |
| **8** | Meta | Meta string encoding (all 5 types) + dedup | Meta string unit tests green |
| **9** | Meta | TypeDef encode/decode + shared streaming | PR #5: meta string + TypeDef complete |
| **10** | Struct/Enum | Enum serializer, struct schema-consistent mode with field ordering | Struct + enum schema-consistent tests passing |
| **11** | Struct/Union | Struct compatible mode, union serializer, skip logic | PR #6: struct + enum + union all modes passing |
| **12** | Hardening | Full XlangTestBase suite green, negative tests, schema evolution, ref tracking, nullable fields | PR #7: all 30+ XlangTestBase tests green |
| **13** | CI/Docs | LuaXlangTest.java finalized, CI workflow, Python↔Lua tests | PR #8: CI pipeline green |
| **14** | Polish | Documentation, performance baseline, final review | PR #9: docs + benchmark report |

---

## Challenges & Mitigation

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| **MurmurHash3 correctness** — 64-bit multiply/rotate in pure Lua | High | Schema hash mismatch → struct interop failure | Test against Go/Java hash outputs for identical inputs; Lua 5.3+ has native int64 arithmetic which handles `(a * b) & 0xFFFFFFFFFFFFFFFF` correctly |
| **Field ordering divergence** — Fory's 6-group algorithm has subtle edge cases (compressed numeric category, size descending, type ID tie-breaking) | High | Schema-consistent mode produces wrong byte order | Extract field ordering test vectors from Java `FieldComparator`, verify against Go `field_info.go` sorting; add explicit ordering unit tests |
| **Table type ambiguity** — Same Lua `table` for array, map, and struct | Medium | Wrong type ID written → deserialization failure on peer | Require declared type descriptors for all non-primitive serialization; never auto-infer table type |
| **UTF-16 string decoding** — Java writes LATIN1/UTF16, Lua reads UTF-8 natively | Medium | String corruption for non-ASCII strings from Java | Implement UTF-16 → UTF-8 transcoding in `string_serializer.read`; test with `こんにちは`, `Привет`, `𝄞🎵🎶` |
| **Collection elements header complexity** — 4 header bits × homogeneous/heterogeneous × null/non-null × ref/no-ref combinations | Medium | Wrong element serialization → wire incompatibility | Study `CollectionLikeSerializer.java` line-by-line; replicate exact bit-flag logic; cross-language test every combination |
| **Map chunk boundary** — Max 255 pairs per chunk, separate chunks for null keys/values | Medium | Large map corruption | Test with maps of 256+ entries; test null key and null value maps |
| **LuaJIT compatibility** — LuaJIT is Lua 5.1 based, lacks `string.pack`/`math.type` | Low | LuaJIT users can't use the library | Phase 8 (post-GSoC) adds LuaJIT FFI fast path; initial delivery targets Lua 5.3/5.4 only |
| **350-hour scope** — Full spec coverage is aggressive | Medium | Incomplete union or compatible mode | Phases prioritize by dependency: buffer→primitives→collections→struct→union. Each phase delivers standalone value. Union can be deferred if needed. |

---

## Expected Outcomes & Impact

### Concrete Deliverables

| Deliverable | Type | LOC Estimate |
|---|---|---|
| `buffer.lua` + `varint.lua` | Lua | ~400–500 |
| `murmur3.lua` | Lua | ~150–200 |
| `ref_resolver.lua` + `type_registry.lua` + `type_resolver.lua` | Lua | ~300–400 |
| `meta_string.lua` + `type_def.lua` | Lua | ~500–600 |
| Serializers (primitive, string, temporal, collection, map, enum, struct, union, binary, skip) | Lua | ~1500–2000 |
| `init.lua` (public API) + `types.lua` + `header.lua` | Lua | ~200–300 |
| Lua unit tests | Lua | ~500–700 |
| `xlang_test_main.lua` (peer binary for all XlangTestBase tests) | Lua | ~800–1200 |
| `LuaXlangTest.java` (Java test runner) | Java | ~200–300 |
| CI workflow additions | YAML | ~50–80 |
| Documentation (`lua_guide.md` + README) | Markdown | ~200–300 |
| **Total** | | **~4800–6600** |

### Who Benefits

- **Game developers** using Lua (Roblox, Defold, LÖVE, Corona) who need to exchange structured data with Java/Python backend services at wire speed.
- **OpenResty/Nginx developers** who can serialize/deserialize Fory binary payloads directly in Lua request handlers without JSON overhead.
- **IoT/embedded developers** using NodeMCU/eLua where Fory's compact binary format reduces network bandwidth compared to JSON/MessagePack.
- **Redis module developers** who can use Fory for efficient cross-language data serialization in Redis Lua scripts.
- **Apache Fory project** — Lua becomes the 12th supported language, demonstrating the protocol's language-agnostic design.

### Long-Term Extensibility

- The Lua runtime follows the same module architecture as Go/Rust/Python implementations, making maintenance consistent.
- LuaJIT FFI acceleration can be added as an optional layer without changing the public API or wire format.
- The `xlang_test_main.lua` peer binary ensures any future protocol changes are automatically validated against Lua.
- The `LuaXlangTest.java` test runner ensures CI catches Lua regressions on every commit to the Fory repository.

---

## What I Bring to This Project

I've spent significant time studying the Fory codebase in depth — not just the documentation, but the **actual source code** across multiple language implementations.

**In the specification**, I've read the complete xlang serialization spec (1462 lines) cover-to-cover — every type ID, every varint encoding algorithm, every reference flag, the complete meta string encoding with all 5 encoding types, the TypeDef format with its 8-byte global header, the collection elements header bit layout, the chunk-based map format, the 6-group struct field ordering algorithm, and the union payload encoding. I understand the protocol at the byte level.

**In the implementation guide**, I've read the complete 8-phase checklist for new language implementations and I've structured my implementation plan to follow it precisely — buffer → varints → header → primitives → strings → temporal → references → collections → maps → meta strings → enums → structs → TypeDef → skip → testing.

**In the cross-language test infrastructure**, I've read `XlangTestBase.java` (2904 lines) and understand every test case: `test_buffer`, `test_buffer_var`, `test_murmurhash3`, `test_string_serializer`, `test_cross_language_serializer`, `test_simple_struct`, `test_named_simple_struct`, `test_list`, `test_map`, `test_item`, `test_color`, `test_struct_with_list`, `test_struct_with_map`, `test_version_check`, `test_polymorphic_list`, `test_polymorphic_map`, `test_schema_evolution_compatible`, `test_nullable_field_*`, `test_ref_*`, `test_circular_ref_*`, `test_union_xlang`, and more.

**In the peer test pattern**, I've studied `GoXlangTest.java` (500 lines) and `xlang_test_main.go` (2588 lines) to understand the exact pattern: environment variable gating (`FORY_GO_JAVA_CI=1`), binary build step in `ensurePeerReady()`, `DATA_FILE` passing via environment, case-name dispatching via `--case` flag, read-verify-re-serialize-write-back flow. My `LuaXlangTest.java` and `xlang_test_main.lua` will follow this identical pattern.

**In Go/Rust/Python runtimes**, I've studied the module structure: Go has `buffer.go`, `fory.go`, `struct.go`, `type_resolver.go`, `type_def.go`, `ref_resolver.go`, `meta_string_resolver.go`, `skip.go`, `union.go` — my Lua module layout mirrors this structure exactly for consistency.

**The timeline is realistic** because: Phase 1 (buffer/varint/header) is pure data structures — no protocol complexity. Phase 2 (primitives/strings) is straightforward type mapping. Phase 3 (collections/maps) is the first protocol complexity — but I have the spec and multiple reference implementations. Phase 4 (meta string/TypeDef) is the hardest part — I've allocated 2 weeks and will test against Java/Go outputs byte-for-byte. Phase 5 (struct/enum/union) builds on all prior phases. Phase 6 (CI/docs) is mechanical. Each phase produces independently testable, committable PRs.

I want to make Lua the 12th language in Fory's ecosystem — fully protocol-compliant, CI-validated, and ready for production use.

---

## Communication

In my opinion, two meetings per week is a good frequency for reporting my work. I am available for communication through video calls and via email and text as well. I will keep tracking my progress on a daily basis through my planner. I will also be blogging about my work every week on **Medium** since I have been writing articles on the platform and am already familiar with its workings.

At the start of every new week, I will outline my goals and report them to my mentors. Then, I will commence work on the planned tasks and document any encountered issues or difficulties throughout the period. I will report these issues to my mentors along with proposed solutions. I also plan to seek guidance from the mentors in case I find it troublesome to handle these obstacles myself. Upon completing the week's tasks, I will review my progress to ensure that all planned goals have been met and report my work to the mentors. I will be following this structured cycle every week to meet all the deadlines and accomplish my weekly and monthly quotas effectively and efficiently.

**Other commitments**: I have no commitments over the course of the 14 weeks starting May and I will be able to work 8 hours a day and 40–50 hours a week.

---

## Appendix: Codebase References

| File | What I Learned |
|---|---|
| [xlang_serialization_spec.md](https://github.com/apache/fory/blob/main/docs/specification/xlang_serialization_spec.md) | Complete protocol: header bitmap, ref flags, type IDs (0–56), user_type_id encoding (varuint32 separate from internal ID), varint algorithms, ZigZag, tagged int64, string format, meta string 5 encodings + dedup, collection elements header, map chunk format, TypeDef 8-byte header + body, struct field ordering 6-group algorithm, union case_id + Any-style payload |
| [xlang_implementation_guide.md](https://github.com/apache/fory/blob/main/docs/specification/xlang_implementation_guide.md) | 8-phase implementation checklist; memory optimization guidelines; fast deserialization via field ID switch; language-specific notes for Java/Python/C++/Rust/Go |
| [xlang_type_mapping.md](https://github.com/apache/fory/blob/main/docs/specification/xlang_type_mapping.md) | Type mapping table for all 11 languages; user type ID encoding examples; type annotation patterns |
| [XlangTestBase.java](https://github.com/apache/fory/blob/main/java/fory-core/src/test/java/org/apache/fory/xlang/XlangTestBase.java) | 2904 lines, 30+ test cases: buffer, varint, murmurhash, string, primitives, struct (simple, named, evolving override), list, map, enum, union, ref tracking, circular ref, nullable fields, schema evolution, polymorphic collections, version check, unsigned types |
| [GoXlangTest.java](https://github.com/apache/fory/blob/main/java/fory-core/src/test/java/org/apache/fory/xlang/GoXlangTest.java) | Peer test pattern: `FORY_GO_JAVA_CI` env var gating, `go build` in `ensurePeerReady()`, `./xlang_test_main --case` dispatch, `DATA_FILE` env var |
| [xlang_test_main.go](https://github.com/apache/fory/blob/main/go/fory/tests/xlang/xlang_test_main.go) | 2588 lines: Go peer binary implementing all XlangTestBase test cases; struct/enum type registration; `read_file`/`write_file` pattern; `flag.Parse` case dispatch |
| [Issue #3380](https://github.com/apache/fory/issues/3380) | Official issue: Lua xlang support with design plan, scope, protocol requirements, Lua-specific design points (metatable restoration, table disambiguation, int64 handling), 9 implementation phases, 5 milestone exit criteria |
