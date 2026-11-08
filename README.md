# Pain

`pain` compiles a `.pain` binary wire schema into dependency-free encoders and decoders for Rust, Python, C#, Zig 0.14.0, and Go.

Each generated module reads and writes identical byte layouts with no runtime reflection and no external serialization libraries.

## Schema example

A `.pain` file declares the package name, optional stream framing, data structures, and tagged wire messages.

```pain
schema Example.Rpc.Protocol;

frame RpcFrame {
	length: u32be @max(16777216);
	tag: RpcMessageType;
}

enum RpcMessageType : u8 {
	Handshake    = 0x01,
	HandshakeAck = 0x02,
}

message Handshake @tag(RpcMessageType.Handshake) {
	worker_id: guid;
	capabilities: list<string> @max(1024);
	auth_token: optional<string> @tail @sentinel("");
}

message HandshakeAck @tag(RpcMessageType.HandshakeAck) {
	accepted: bool;
}
```

## Build from source

Build the `pain` CLI binary with Cargo:

```bash
cargo build --release
```

Cargo writes the compiled binary to `target/release/pain`.

## Validate and generate code

1. Validate a schema file and print declaration counts:

```bash
pain check examples/rpc_session.pain
```

2. Generate source files for one or more target languages:

```bash
pain generate examples/rpc_session.pain \
	--rust out/rpc_session.rs \
	--python out/rpc_session.py \
	--csharp out/RpcSession.g.cs \
	--zig out/rpc_session.zig \
	--go out/rpc_session.go
```

You can pass any combination of `--rust`, `--python`, `--csharp`, `--zig`, and `--go` in a single invocation.

## Generated API overview

Every target generator emits types for enums, structs, choices, messages, and an optional frame helper.

| Operation | Rust | Python | C# | Zig 0.14.0 | Go |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Encode a message | `msg.to_bytes()` | `msg.to_bytes()` | `msg.ToBytes()` | `try msg.toBytes(alloc)` | `msg.ToBytes()` |
| Decode a message | `Msg::from_bytes(buf)` | `Msg.from_bytes(buf)` | `Msg.FromBytes(buf)` | `try Msg.fromBytes(buf, alloc)` | `MsgFromBytes(buf)` |
| Add a frame header | `encode_frame(&bytes)` | `encode_frame(raw)` | `Frame.EncodeFrame(bytes)` | `try Frame.encodeFrame(alloc, bytes)` | `EncodeFrame(bytes)` |
| Strip a frame header | `decode_frame(&framed)` | `decode_frame(framed)` | `Frame.DecodeFrame(framed)` | `try Frame.decodeFrame(framed)` | `DecodeFrame(framed)` |

## Run the test suite

Run the unit tests, target code generation tests, and cross-language byte-equivalence tests:

```bash
cargo test
```

The integration suite compiles and executes the generated Rust, Python 3, .NET 10 C#, Zig 0.14.0, and Go outputs and verifies that all five targets produce identical bytes.

## Documentation and examples

- [Language and wire specification](docs/spec.md)
- [RPC control-plane schema](examples/rpc_session.pain)
- [Bitflag-guarded telemetry schema](examples/telemetry_stream.pain)

## License

MIT. See [LICENSE](LICENSE).
