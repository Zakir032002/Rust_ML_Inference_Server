# Apache Fory C++ & Rust gRPC Integration — GSoC 2026 Proposal

> **Project**: [GSOC-320](https://issues.apache.org/jira/browse/GSOC-320) — Apache Fory C++ & Rust gRPC Integration
> **Organization**: Apache Software Foundation
> **Mentors**: Chaokun Yang (chaokunyang@apache.org), Weipeng Wang
> **Size**: ~350 hours (Large) | **Difficulty**: Medium to Hard
> **Issues**: [#3266](https://github.com/apache/fory/issues/3266) (Parent) · [#3276](https://github.com/apache/fory/issues/3276) (C++) · [#3275](https://github.com/apache/fory/issues/3275) (Rust)

---

## 🧠 Project Synopsis

Apache Fory is a blazingly fast multi-language serialization framework powered by JIT and zero-copy, supporting Java, Python, C++, Go, Rust, and more. Its compiler (`foryc`) parses `.proto`, `.fbs`, and `.fdl` schemas and generates high-performance, native-language model code. The compiler already parses **service definitions** — including unary, client-streaming, server-streaming, and bidirectional-streaming RPCs — into its Intermediate Representation (IR). However, **no gRPC service code is generated for any language**.

This project implements **end-to-end gRPC code generation for C++ and Rust** in the Fory compiler. The generated bindings use **Fory's own binary serialization** as the gRPC wire format rather than protobuf, delivering lower serialization overhead and zero-copy deserialization. For C++, this produces `greeter_service.h`, `greeter_service.grpc.h`, and `greeter_service.grpc.cc`. For Rust, this produces `greeter_service.rs` and `greeter_service_grpc.rs` with `tonic`-compatible async stubs.

**Impact**: A single `foryc --grpc --lang cpp,rust` invocation produces production-ready gRPC stubs. C++↔Rust cross-language interoperability is validated end-to-end, and the architecture established here becomes the template for Java, Go, and Python gRPC support in the future.

---

## 🔍 Problem Deep Dive

### Current State — Based on Actual Codebase Analysis

The Fory compiler has a three-stage pipeline I've verified by reading the complete source code:

```
IDL Source (.proto / .fbs / .fdl)
        │
        ▼
  ┌──────────────────┐     ┌─────────────────────┐     ┌───────────────────────────┐
  │  Frontend         │────▶│   IR Layer           │────▶│    Code Generators        │
  │  • ProtoTranslator│     │   Schema {           │     │    CppGenerator → .h      │
  │  • FbsTranslator  │     │     messages, enums, │     │    RustGenerator → .rs    │
  │  • FdlTranslator  │     │     unions, services │     │    + 5 more languages     │
  └──────────────────┘     │   }                  │     └───────────────────────────┘
                            │   Service {          │
                            │     methods: [       │
                            │       RpcMethod {    │
                            │         request_type,│
                            │         response_type│
                            │         client_      │
                            │           streaming, │
                            │         server_      │
                            │           streaming  │
                            │       }              │
                            │     ]                │
                            │   }                  │
                            └─────────────────────┘
```

#### What's Already Implemented (Verified via Code Reading)

| Component | Status | Evidence |
|---|---|---|
| **IR Service nodes** | ✅ Complete | `Service` and `RpcMethod` dataclasses in [ast.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/ir/ast.py). `Schema.services: List[Service]` field exists. |
| **Proto service parsing** | ✅ Complete | `ProtoTranslator` correctly translates `service` blocks. Tests in [test_proto_service.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/tests/test_proto_service.py) verify unary, client-streaming, server-streaming, bidi, and options. |
| **FBS service parsing** | ✅ Complete | `FbsTranslator` handles `rpc_service` blocks with `(streaming: "client"/"server"/"bidi")`. Tests in [test_fbs_service.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/tests/test_fbs_service.py) verify all patterns. |
| **`--grpc` CLI flag** | ✅ Complete | [cli.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/cli.py): `--grpc` → `compile_file(grpc=True)` → `GeneratorOptions(grpc=True)` → calls `generator.generate_services()` and appends results. |
| **`generate_services()` hook** | ✅ Exists (empty) | [BaseGenerator.generate_services()](https://github.com/apache/fory/blob/main/compiler/fory_compiler/generators/base.py) returns `[]`. Neither `CppGenerator` nor `RustGenerator` overrides it. |
| **C++ model codegen** | ✅ Mature | Generates header-only `.h` with `FORY_STRUCT` macros, `enum class`, `std::variant`-based unions, `register_types()`, thread-safe singleton via `detail::get_fory()`. |
| **Rust model codegen** | ✅ Mature | Generates `.rs` with `#[derive(ForyObject)]` structs, `pub mod` nested types, tagged enum unions, `register_types()`, `OnceLock` singleton. |

#### What's NOT Implemented (The Gap This Project Fills)

| Gap | Impact |
|---|---|
| **`CppGenerator.generate_services()`** | No gRPC C++ code generated — zero service output |
| **`RustGenerator.generate_services()`** | No gRPC Rust code generated — zero service output |
| **`ForySerializationTraits<T>` (C++)** | No codec bridge between `grpc::ByteBuffer` and Fory serialization |
| **`ForyCodec` (Rust/tonic)** | No codec bridge between `tonic::codec::Codec` and Fory serialization |
| **Service validation** | `SchemaValidator` validates messages/enums/unions but does **not** validate services |
| **Import merging for services** | `resolve_imports()` merges enums/messages/unions but does **not** merge `services` from imported schemas |
| **Zero-copy deserialization** | No `ByteBuffer ↔ Fory::Buffer` or `bytes::Bytes ↔ fory::Buffer` adapter exists |

### Why This Problem is Non-Trivial

1. **Two distinct codegen targets** with fundamentally different idioms: C++ abstract classes with `::grpc::Status` returns vs. Rust `async fn` traits with `tonic::Request`/`tonic::Response` wrappers.
2. **Custom codec integration**: Both need codec implementations that replace protobuf — touching buffer management, framing, and zero-copy semantics at the gRPC transport layer.
3. **Four RPC patterns × two languages**: Unary, client-streaming, server-streaming, bidirectional — each with different type signatures (`ServerReader`, `ServerWriter`, `ServerReaderWriter` in C++; `tonic::Streaming<T>`, `impl Stream<Item=Result<T>>` in Rust).
4. **Binary compatibility**: The Fory wire format from C++ `Fory::serialize()` → `Result<vector<uint8_t>, Error>` must be byte-identical to Rust `fory::serialize()` → `Result<Vec<u8>, Error>` for cross-language interop.
5. **Code generation integration**: The new `generate_services()` must follow the exact conventions of the existing generators — line-by-line `List[str]` assembly, `GeneratedFile` objects, namespace handling, import resolution, type mapping.

---

## 🏗️ System Architecture

The implementation extends the existing compiler pipeline at **exactly one hook point** — `BaseGenerator.generate_services()` — without modifying any existing method signatures or IR types.

### Component Diagram — Generation Pipeline

```
┌──────────────────────────────────────────────────────────────────────────────────┐
│                        FORY COMPILER (Python)                                    │
│                                                                                  │
│   CLI (cli.py)                                                                   │
│   foryc --grpc --lang cpp,rust schema.proto                                      │
│       ↓                                                                          │
│   parse_idl_file() → ProtoTranslator / FbsTranslator / FdlTranslator            │
│       ↓                                                                          │
│   resolve_imports() → merged Schema (messages + enums + unions + services ★)    │
│       ↓                                                                          │
│   SchemaValidator.validate()  ←── [NEW] validate services ★                     │
│       ↓                                                                          │
│   for lang in [cpp, rust]:                                                       │
│       generator = CppGenerator(schema, GeneratorOptions(grpc=True))              │
│       files         = generator.generate()           # existing model code       │
│       service_files = generator.generate_services()  # ← [NEW] gRPC output      │
│       generator.write_files(files + service_files)                               │
│                                                                                  │
│   ★ = this project's additions                                                   │
└────────────────────────────────┬─────────────────────────────────────────────────┘
                                 │
                ┌────────────────┴─────────────────┐
                ▼                                  ▼
┌──────────────────────────────────┐   ┌──────────────────────────────────┐
│  C++ Generated Output            │   │  Rust Generated Output           │
│                                  │   │                                  │
│  greeter.h            (existing) │   │  greeter.rs            (existing)│
│  greeter_service.h         [NEW] │   │  greeter_service.rs        [NEW] │
│  greeter_service.grpc.h    [NEW] │   │  greeter_service_grpc.rs   [NEW] │
│  greeter_service.grpc.cc   [NEW] │   │                                  │
└──────────────────────────────────┘   └──────────────────────────────────┘
```

### Generated Code Architecture — C++

```
greeter_service.h                      ← Service abstraction (no gRPC dependency)
├── class GreeterServiceBase
│   ├── virtual grpc::Status SayHello(const HelloRequest&, HelloReply*) = 0;
│   ├── virtual grpc::Status LotsOfReplies(const HelloRequest&,
│   │       grpc::ServerWriter<HelloReply>*) = 0;
│   ├── virtual grpc::Status LotsOfGreetings(grpc::ServerReader<HelloRequest>*,
│   │       HelloReply*) = 0;
│   └── virtual grpc::Status BidiHello(
│           grpc::ServerReaderWriter<HelloReply, HelloRequest>*) = 0;
├── class GreeterStub                  ← Client stub (wraps grpc::Channel)
│   └── grpc::Status SayHello(const HelloRequest&, HelloReply*);
└── ForyCodecHelper<T>                 ← Thin wrapper over detail::get_fory()

greeter_service.grpc.h                 ← gRPC-specific declarations
├── static method descriptors (grpc::internal::RpcMethod constants)
├── class GreeterService final : public grpc::Service
│   └── RequestSayHello(), RequestLotsOfReplies(), ...
└── class GreeterStubImpl : public GreeterStub

greeter_service.grpc.cc                ← Implementation
├── GreeterService constructor (registers all methods with gRPC)
├── GreeterStubImpl method bodies (serialize → channel → deserialize)
├── template<> struct grpc::SerializationTraits<HelloRequest> {
│       static grpc::Status Serialize(const HelloRequest& obj,
│           grpc::ByteBuffer* buf) {
│           auto r = detail::get_fory().serialize(obj);
│           *buf = grpc::ByteBuffer(r.value().data(), r.value().size());
│           return grpc::Status::OK;
│       }
│       static grpc::Status Deserialize(grpc::ByteBuffer* buf,
│           HelloRequest* obj) {
│           // zero-copy path: ByteBuffer::Dump() → contiguous ptr
│           // fallback path:  TryCopyTo() → vector<uint8_t>
│           auto r = detail::get_fory().deserialize<HelloRequest>(ptr, size);
│           *obj = std::move(r.value());
│           return grpc::Status::OK;
│       }
│   }
```

### Generated Code Architecture — Rust

```
greeter_service.rs                     ← Service abstraction (no tonic dependency)
├── #[async_trait]
│   pub trait Greeter: Send + Sync + 'static {
│       async fn say_hello(&self,
│           req: tonic::Request<HelloRequest>)
│           -> Result<tonic::Response<HelloReply>, tonic::Status>;
│       async fn lots_of_replies(&self,
│           req: tonic::Request<HelloRequest>)
│           -> Result<tonic::Response<
│               tonic::codec::Streaming<HelloReply>>, tonic::Status>;
│   }
├── pub struct GreeterClient<T> { inner: tonic::client::Grpc<T> }
│   └── pub async fn say_hello(&mut self, req: HelloRequest)
│           -> Result<HelloReply, tonic::Status>
└── pub struct ForyCodec<E, D>(PhantomData<(E, D)>);
    ├── impl<T: ForyObject> tonic::codec::Encoder for ForyEncoder<T>
    │   └── fn encode(&mut self, item: T, dst: &mut EncodeBuf<'_>)
    │           → detail::get_fory().serialize(&item) → dst.put_slice(&bytes)
    └── impl<T: ForyObject> tonic::codec::Decoder for ForyDecoder<T>
        └── fn decode(&mut self, src: &mut DecodeBuf<'_>)
                → src.to_bytes() → detail::get_fory().deserialize::<T>(&bytes)

greeter_service_grpc.rs                ← tonic transport bindings
├── pub mod greeter_server {
│       pub struct GreeterServer<T: Greeter> { inner: Arc<T> }
│       impl<T: Greeter> tonic::codegen::Service<
│           http::Request<tonic::body::BoxBody>> for GreeterServer<T>
│       impl<T: Greeter> tonic::server::NamedService for GreeterServer<T>
│           { const NAME: &'static str = "Greeter"; }
│   }
└── pub mod greeter_client {
        pub struct GreeterClient { inner: tonic::client::Grpc<Channel> }
        pub async fn say_hello(&mut self, req: HelloRequest)
            → Result<HelloReply, tonic::Status>
    }
```

### Technology Choices & Trade-offs

| Decision | Choice | Rationale |
|---|---|---|
| C++ gRPC library | `grpc++` (official) | Only production-grade C++ gRPC library; `grpc::SerializationTraits<T>` is the documented extension point for custom codecs |
| Rust gRPC library | `tonic` | De facto Rust gRPC library; trait-based design aligns with Fory's `#[derive(ForyObject)]`; `tonic::codec::Codec` is the documented custom codec extension point |
| Wire format | Fory binary | 10–170× faster than protobuf per Fory benchmarks; zero-copy support; already validated cross-language in existing Fory tests |
| C++ Fory API | `detail::get_fory().serialize(obj)` → `Result<vector<uint8_t>, Error>` | Exact API generated by current `CppGenerator` for `to_fory_bytes()` / `from_fory_bytes()` helpers |
| Rust Fory API | `detail::get_fory().serialize(&obj)` → `Result<Vec<u8>, fory::Error>` | Same `OnceLock` singleton pattern as current generated Rust code |
| Zero-copy strategy | Best-effort with safe fallback | `grpc::ByteBuffer::Dump()` for contiguous buffers; `bytes::Bytes` slice reuse in Rust; copy-fallback when buffer is scattered |

---

## 🔄 Flow Diagrams

### Flow 1: IDL → Running gRPC Service (End-to-End)

A single `foryc --grpc --lang cpp,rust` invocation produces all artifacts needed to build and run a cross-language gRPC service.

```
    ┌────────────────────────────────┐
    │  Developer writes schema.proto │
    │                                │
    │  service Greeter {             │
    │    rpc SayHello (HelloRequest) │
    │        returns (HelloReply);   │
    │    rpc LotsOfReplies (...)     │
    │        returns (stream ...);   │
    │  }                             │
    └───────────┬────────────────────┘
                │
                ▼
    ┌────────────────────────────────┐
    │  $ foryc --grpc                │
    │    --lang cpp,rust             │
    │    --cpp_out ./gen/cpp         │
    │    --rust_out ./gen/rust       │
    │    schema.proto                │
    └───────────┬────────────────────┘
                │
      ┌─────────┴──────────┐
      ▼                    ▼
 ┌──────────────┐   ┌──────────────┐
 │  C++ Output  │   │ Rust Output  │
 │  greeter.h   │   │ greeter.rs   │
 │ *_service.h  │   │ *_service.rs │
 │ *.grpc.h     │   │ *_grpc.rs    │
 │ *.grpc.cc    │   │              │
 └──────┬───────┘   └──────┬───────┘
        │                  │
        ▼                  ▼
 ┌──────────────┐   ┌──────────────┐
 │  Link with:  │   │  Compile:    │
 │  • libfory   │   │  • fory crate│
 │  • libgrpc++ │   │  • tonic     │
 │  • CMake     │   │  • Cargo     │
 └──────┬───────┘   └──────┬───────┘
        │                  │
        ▼                  ▼
 ┌──────────────┐   ┌──────────────┐
 │  C++ Server  │◀──── Fory ─────▶│ Rust Client  │
 │  implements  │   wire format   │  generated   │
 │ GreeterBase  │  (NOT protobuf) │ GreeterClient│
 └──────────────┘                 └──────────────┘
```

### Flow 2: C++ Request/Response Lifecycle

On the C++ path, `ForySerializationTraits<T>` is the sole bridge between `grpc::ByteBuffer` and the Fory serializer — no protobuf types are touched at any point.

```
C++ Client                      gRPC Framework              C++ Server
    │                                │                           │
    │  stub.SayHello(req, &resp)     │                           │
    │─────┐                          │                           │
    │     │ SerializationTraits      │                           │
    │     │   ::Serialize(req, &buf) │                           │
    │     │ ┌────────────────────┐   │                           │
    │     │ │detail::get_fory()  │   │                           │
    │     │ │.serialize(req)     │   │                           │
    │     │ │→ Result<vec<u8>>   │   │                           │
    │     │ │→ ByteBuffer::Copy  │   │                           │
    │     │ └────────────────────┘   │                           │
    │     └──────────────────────────▶                           │
    │                                │──── HTTP/2 frame ────────▶│
    │                                │                           │
    │                                │                           │ SerializationTraits
    │                                │                           │  ::Deserialize(&buf,&req)
    │                                │                           │ ┌────────────────────┐
    │                                │                           │ │ByteBuffer.Dump()   │
    │                                │                           │ │→ contiguous bytes  │
    │                                │                           │ │detail::get_fory()  │
    │                                │                           │ │.deserialize<Req>() │
    │                                │                           │ │→ Result<Req, Err>  │
    │                                │                           │ └────────────────────┘
    │                                │                           │
    │                                │                           │ GreeterBase::SayHello(req, &resp)
    │                                │                           │ → user implementation
    │                                │                           │ → fills resp
    │                                │                           │ → returns grpc::Status::OK
    │                                │                           │
    │                                │                           │ SerializationTraits
    │                                │                           │  ::Serialize(resp, &buf)
    │                                │◀──── HTTP/2 frame ────────│
    │◀───────────────────────────────│                           │
    │  SerializationTraits           │                           │
    │    ::Deserialize(&buf, &resp)  │                           │
    │  → resp populated              │                           │
```

### Flow 3: Rust Request/Response Lifecycle

On the Rust path, `ForyCodec<E,D>` implements `tonic::codec::Codec` — the same extension point tonic uses internally for `ProstCodec` — and replaces it entirely with Fory.

```
Rust Client                       tonic Runtime               Rust Server
    │                                  │                           │
    │  client.say_hello(req).await     │                           │
    │──────┐                           │                           │
    │      │ ForyCodec::encoder()      │                           │
    │      │ .encode(req, &mut buf)    │                           │
    │      │ ┌─────────────────────┐   │                           │
    │      │ │detail::get_fory()   │   │                           │
    │      │ │.serialize(&req)     │   │                           │
    │      │ │→ Result<Vec<u8>>    │   │                           │
    │      │ │buf.put_slice(&bytes)│   │                           │
    │      │ └─────────────────────┘   │                           │
    │      └────────────────────────── ▶                           │
    │                                  │── HTTP/2 frame ──────────▶│
    │                                  │                           │
    │                                  │                           │ ForyCodec::decoder()
    │                                  │                           │ .decode(&mut buf)
    │                                  │                           │ ┌─────────────────────┐
    │                                  │                           │ │buf.to_bytes()       │
    │                                  │                           │ │→ Bytes (zero-copy?) │
    │                                  │                           │ │detail::get_fory()   │
    │                                  │                           │ │.deserialize(&bytes) │
    │                                  │                           │ │→ Result<Req, Err>   │
    │                                  │                           │ └─────────────────────┘
    │                                  │                           │
    │                                  │                           │ Greeter::say_hello(req).await
    │                                  │                           │ → user async implementation
    │                                  │                           │ → Ok(Response::new(resp))
    │                                  │                           │
    │                                  │                           │ ForyCodec::encoder()
    │                                  │                           │ .encode(resp, &mut buf)
    │                                  │◀── HTTP/2 frame ──────────│
    │◀─────────────────────────────────│                           │
    │  ForyCodec::decoder()            │                           │
    │  .decode(&mut buf) → resp        │                           │
```

### Flow 4: Zero-Copy Deserialization Decision

Zero-copy is attempted on every inbound payload and fails safely — the fallback path is always implemented and tested first, then the optimized path is layered on top.

```
┌────────────────────────────────────────────────────────────────────┐
│                    Inbound gRPC Payload                            │
│                                                                    │
│  ┌────────────────────┐                                           │
│  │ grpc::ByteBuffer    │  (C++)                                   │
│  │ OR bytes::Bytes     │  (Rust)                                  │
│  └────────┬───────────┘                                           │
│           │                                                        │
│           ▼                                                        │
│  ┌─────────────────────────────────┐                             │
│  │ Is buffer a single contiguous   │                             │
│  │ memory region?                  │                             │
│  └──────┬──────────────┬───────────┘                             │
│         │ YES          │ NO                                       │
│         ▼              ▼                                          │
│  ┌─────────────┐  ┌──────────────────────┐                       │
│  │ ZERO-COPY   │  │  FALLBACK (COPY)     │                       │
│  │             │  │                      │                       │
│  │ C++:        │  │  C++:                │                       │
│  │  Dump() →   │  │   buf.TryCopyTo()    │                       │
│  │  uint8_t*   │  │   → vector<uint8_t>  │                       │
│  │  wrap in    │  │   then deserialize   │                       │
│  │  Buffer     │  │                      │                       │
│  │  (no copy)  │  │  Rust:               │                       │
│  │             │  │   buf.to_vec()       │                       │
│  │ Rust:       │  │   → Vec<u8>          │                       │
│  │  Bytes ref  │  │   then deserialize   │                       │
│  │  (no copy)  │  │                      │                       │
│  └──────┬──────┘  └──────────┬───────────┘                       │
│         │                    │                                    │
│         └──────────┬─────────┘                                    │
│                    ▼                                               │
│         ┌─────────────────────┐                                   │
│         │  fory.deserialize   │                                   │
│         │  <T>(data, size)    │                                   │
│         │  → Result<T, Error> │                                   │
│         └─────────────────────┘                                   │
└────────────────────────────────────────────────────────────────────┘
```

---

## ⚙️ Implementation Plan

The project is structured in three phases, each delivering standalone value so that an incomplete final phase does not invalidate earlier work.

### Phase 1 — Foundation & Unary RPC (Weeks 1–5)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 1.1 | Fix `resolve_imports()` to merge `services` from imported schemas | Services available in merged schema | Small change in `cli.py` where `Schema` is constructed |
| 1.2 | Add service validation in `SchemaValidator` | Unique service names, unique method names, request/response types resolve to messages | Follow existing `_check_messages()` pattern |
| 1.3 | `CppGenerator.generate_services()` → `service.h` | `class GreeterBase` with `virtual grpc::Status SayHello(const Req&, Resp*) = 0;` per unary RPC | C++ gRPC idioms: `grpc::Status` returns, const-ref request, pointer response |
| 1.4 | `CppGenerator` → `service.grpc.h` + `service.grpc.cc` | gRPC service class, stub class, `ForySerializationTraits<T>` specialization | `grpc::SerializationTraits` template specialization for `ByteBuffer` |
| 1.5 | `RustGenerator.generate_services()` → `service.rs` | `#[async_trait] pub trait Greeter` + `ForyCodec<E,D>` implementing `tonic::codec::Codec` | Correct `async_trait` generation, `Encoder`/`Decoder` bounds |
| 1.6 | `RustGenerator` → `service_grpc.rs` | `greeter_server` module (`GreeterServer<T>` + `tonic::codegen::Service` impl), `greeter_client` module | Matching `tonic-build` codegen patterns exactly |
| 1.7 | End-to-end compile test: `.proto` with `--grpc` → verify output files exist, compile, link | Compilable C++ and Rust gRPC stubs for unary RPCs | Build system integration (CMake + Cargo) |

### Phase 2 — Streaming & Zero-Copy (Weeks 6–8)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 2.1 | C++ client-streaming (`grpc::ServerReader<Req>`) | Generated read loop receiving streamed `Req` from client | Correct `ServerReader` template instantiation in generated code |
| 2.2 | C++ server-streaming (`grpc::ServerWriter<Resp>`) | Generated write pattern sending streamed `Resp` to client | Buffer management across multiple `Serialize` calls |
| 2.3 | C++ bidi-streaming (`grpc::ServerReaderWriter<Resp, Req>`) | Combined reader/writer handling | RAII-scoped ownership of reader/writer in generated code |
| 2.4 | Rust streaming: `tonic::Streaming<Req>` input, `impl Stream<Item=Result<Resp>>` output | All 4 RPC patterns for Rust | Pin-boxing streams, `Send + 'static` bounds |
| 2.5 | C++ zero-copy: `ByteBuffer::Dump()` → `Buffer(ptr, size, /*owns=*/false)` | Avoid `memcpy` on contiguous buffers | Detecting contiguous vs. scattered `ByteBuffer` slices |
| 2.6 | Rust zero-copy: `Bytes` slice reuse → `fory::deserialize()` without `.to_vec()` | Avoid heap allocation on Rust decode path | Ownership transfer from tonic's buffer pool |
| 2.7 | Golden codegen tests | Snapshot tests for file names + key method signatures, both languages | Platform-stable golden files (cross-platform line endings) |

### Phase 3 — Testing, Interop & Polish (Weeks 9–12)

| # | Task | Output | Key Challenge |
|---|---|---|---|
| 3.1 | Runtime round-trip tests (C++) | `serialize → ByteBuffer → deserialize` passes for all message types | C++ gRPC test fixture setup |
| 3.2 | Runtime round-trip tests (Rust) | `encode → DecodeBuf → decode` passes for all message types | Async test runtime with tonic in-process channels |
| 3.3 | C++↔Rust interop: C++ server + Rust client | Cross-language unary + streaming RPCs verified end-to-end | Binary format identity between C++ and Rust Fory |
| 3.4 | C++↔Rust interop: Rust server + C++ client | Reverse direction verified | Same binary format assertion |
| 3.5 | Runnable C++ example (CMake project) | Server + client demo with generated bindings | `find_package(Fory)` + `find_package(gRPC)` in CMakeLists.txt |
| 3.6 | Runnable Rust example (Cargo project) | Async server + client with tonic + fory | Cargo.toml dependency management |
| 3.7 | CI integration | GitHub Actions: codegen tests + runtime tests + regression detection; fail on changed golden | API signature regression checks |
| 3.8 | Documentation | Compiler guide + C++/Rust language guides for `--grpc` flag, build setup, generated code examples | Accurate docs matching actual generated output |

---

## 📅 Timeline (Week-by-Week)

| Week | Phase | Activities | Deliverables |
|---|---|---|---|
| **1** | Foundation | Deep-dive existing generators; prototype `ForySerializationTraits` and `ForyCodec` to validate APIs | PR #1: service validation + import merging fix |
| **2** | Foundation | C++ `service.h` generation: `GreeterBase` abstract class with unary method signatures | `GreeterBase` class generating correctly |
| **3** | Foundation | C++ `service.grpc.h` + `.grpc.cc` + `ForySerializationTraits<T>` | Compilable C++ gRPC stubs |
| **4** | Foundation | Rust `service.rs`: `#[async_trait]` trait + `ForyCodec` struct | Rust service trait + codec generating correctly |
| **5** | Foundation | Rust `service_grpc.rs`: tonic server/client modules; end-to-end compilation test | **PR #2**: complete unary RPC generation, both languages |
| **6** | Streaming | C++ streaming RPCs: all 3 patterns (`ServerReader`, `ServerWriter`, `ServerReaderWriter`) | All 4 C++ RPC patterns working |
| **7** | Streaming | Rust streaming RPCs + C++ zero-copy path | All 4 Rust RPC patterns; C++ zero-copy deserialization |
| **8** | Streaming | Rust zero-copy + golden codegen tests | **PR #3**: streaming + zero-copy + golden tests |
| **9** | Testing | Runtime round-trip tests: C++ and Rust | Codec correctness validated |
| **10** | Interop | Cross-language interop tests (both directions) | C++↔Rust communication verified |
| **11** | Polish | Runnable C++ + Rust examples; CI pipelines | Complete example projects; CI configured |
| **12** | Polish | Documentation + final review + buffer | **PR #4**: docs + examples + CI — all tests green |

---

## ⚠️ Challenges & Mitigation

| Risk | Probability | Impact | Mitigation Strategy |
|---|---|---|---|
| **`grpc::SerializationTraits<T>` complexity** — Template specialization for custom codecs is poorly documented | High | ~1 week delay on C++ codec | Study gRPC's own `ProtoBufferReader`/`ProtoBufferWriter` as reference; prototype in Week 1 before main codegen work |
| **tonic codec API changes** — `tonic::codec::Codec` trait may change between minor versions | Medium | Breaking generated code | Pin exact tonic version; follow `tonic-build`'s own output as canonical reference |
| **Zero-copy edge cases** — Scattered `ByteBuffer` slices (C++) or non-contiguous `Bytes` (Rust) block zero-copy | High | Performance degradation on some payloads | Implement safe fallback first; zero-copy is optimization-only; measure copy frequency in tests |
| **Wire format mismatch C++ ↔ Rust** — Endianness, alignment, header flags | Medium | Interop test failures | Use Fory's existing cross-language test vectors; add explicit binary snapshot tests |
| **Streaming lifecycle (C++)** — `ServerReader`/`Writer` are borrowed references with complex ownership | Medium | Memory safety bugs in generated code | Generate RAII-scoped patterns; review with mentors before merging |
| **Build system integration** — Users need CMake/Bazel for C++ gRPC + Cargo for Rust tonic | Low | Developer friction | Ship example `CMakeLists.txt` and `Cargo.toml`; document in guides |
| **350-hour scope risk** | Medium | Incomplete deliverables | Phase 1 (unary, both langs) is the MVP. Phase 2 (streaming) is value-add. Phase 3 (interop/docs) is polish. Each phase ships standalone. |

---

## 📈 Expected Outcomes & Impact

### Concrete Deliverables

| Deliverable | Type | LOC Estimate |
|---|---|---|
| `CppGenerator.generate_services()` in `cpp.py` | Python (compiler) | ~400–600 |
| `RustGenerator.generate_services()` in `rust.py` | Python (compiler) | ~300–500 |
| Service validation in `validator.py` | Python (compiler) | ~50–80 |
| Import merging fix in `cli.py` | Python (compiler) | ~5–10 |
| Golden codegen tests | Python (tests) | ~200–300 |
| Runtime round-trip tests (C++) | C++ (tests) | ~150–200 |
| Runtime round-trip tests (Rust) | Rust (tests) | ~150–200 |
| C++↔Rust interop tests | C++ + Rust + harness | ~200–300 |
| Example projects | C++ (CMake) + Rust (Cargo) | ~200–300 |
| Documentation updates | Markdown | ~100–200 |

### Who Benefits

- **Systems programmers** building C++ microservices who want faster RPC serialization than protobuf.
- **Rust async ecosystem users** who want `tonic` services without a protobuf dependency.
- **Cross-language platform teams** needing C++↔Rust service interop with a shared IDL.
- **Apache Fory project** — this is the first gRPC output for *any* language, establishing the `generate_services()` pattern for Java, Go, Python, C#, and Swift.

### Long-Term Extensibility

- The `generate_services()` → `List[GeneratedFile]` pattern extends to all 7 target languages with no IR changes.
- `ForySerializationTraits<T>` (C++) and `ForyCodec<E,D>` (Rust) become reusable library components.
- Golden test CI ensures API signature stability across all future compiler updates.
- Issue [#3266](https://github.com/apache/fory/issues/3266) explicitly plans Java, Python, Go backends — this project lays the foundation.

---

## 🎤 What I Bring to This Project

I've spent significant time studying the Fory compiler codebase in detail — not just the docs, but the **actual source code**.

**In the compiler**, I've read every method of `CppGenerator` (17 source chunks — `generate_message_definition()`, `generate_union_serializer()`, `generate_registration()`, the `FORY_STRUCT` macro emission, the `detail::get_fory()` singleton pattern) and `RustGenerator` (8 chunks — `#[derive(ForyObject)]`, `pub mod` nesting, `OnceLock` singleton, `register_types()`). I understand the code generation convention: line-by-line `List[str]` assembly, `GeneratedFile` objects, namespace resolution, import handling.

**In the CLI**, I've traced the entire `--grpc` flag path: `parse_args()` → `compile_file(grpc=True)` → `GeneratorOptions(grpc=True)` → `generator.generate_services()` → appended to output files. I know exactly where to plug in.

**In the IR**, I've verified that `Service` and `RpcMethod` exist with `client_streaming`/`server_streaming` booleans, that `Schema.services` is populated by all three frontends (proto, fbs, fdl), and that the validator currently ignores services — a gap I will fix in Week 1.

**In the runtimes**, I've read `fory.h` (C++ — `Fory::serialize()` → `Result<vector<uint8_t>, Error>`, `deserialize<T>()`, `ThreadSafeFory`, `ForyBuilder`) and the Rust crate structure (`fory` wraps `fory-core` + `fory-derive`). I know the exact serialization APIs I'll call from the generated codec code.

**The timeline is realistic**: Phase 1 (unary) is the essential MVP — if streaming takes longer than planned, I deliver unary for both languages plus partial streaming. Each phase has standalone value. I want to build something that **ships in Fory's next release** — not a proof-of-concept branch.

---

## Appendix: Codebase References

| File | What I Verified |
|---|---|
| [ast.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/ir/ast.py) | `Service`, `RpcMethod`, `Schema.services` — all exist, ready for codegen |
| [base.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/generators/base.py) | `BaseGenerator.generate_services() → []`, `GeneratorOptions.grpc: bool` |
| [cpp.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/generators/cpp.py) | `CppGenerator`: 700+ lines, does NOT override `generate_services()` |
| [rust.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/generators/rust.py) | `RustGenerator`: 500+ lines, does NOT override `generate_services()` |
| [cli.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/cli.py) | `--grpc` flag path traced; `resolve_imports()` does not merge services |
| [validator.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/ir/validator.py) | Validates messages/enums/unions — NOT services; gap to fill |
| [test_proto_service.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/tests/test_proto_service.py) | Proto service parsing verified: unary, all streaming patterns, options |
| [test_fbs_service.py](https://github.com/apache/fory/blob/main/compiler/fory_compiler/tests/test_fbs_service.py) | FBS `rpc_service` parsing verified: unary + streaming attributes |
| [fory.h](https://github.com/apache/fory/blob/main/cpp/fory/serialization/fory.h) | C++ API: `Fory::serialize<T>()`, `deserialize<T>()`, `ThreadSafeFory`, `ForyBuilder`, `Buffer` |
| [Issue #3266](https://github.com/apache/fory/issues/3266) | Parent design doc: goals, IR changes, file layouts, testing plan, codegen footprint |
| [Issue #3276](https://github.com/apache/fory/issues/3276) | C++ gRPC codegen tracking issue |
| [Issue #3275](https://github.com/apache/fory/issues/3275) | Rust gRPC codegen tracking issue |
