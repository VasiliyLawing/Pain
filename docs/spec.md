# Pain language and wire specification

This reference defines the `.pain` schema grammar, binary wire encodings, field annotations, target language mappings, and compiler validation rules.

## File structure

Every `.pain` file starts with a `schema` package declaration followed by zero or more type declarations.

```pain
schema Example.Telemetry.Protocol;
```

The compiler maps the package name to the target module structure:

- **C#.** Emits `namespace Example.Telemetry.Protocol;`.
- **Rust, Python, and Zig.** Emits top-level declarations in the generated file.

Line comments begin with `//` and run to the end of the line.

## Primitive types

The encoder writes fields in declaration order with no implicit field tags and no alignment padding.

| Schema type | Wire encoding | Rust type | Python type | C# type | Zig 0.14.0 type |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `u8` | 1-byte unsigned integer | `u8` | `int` | `byte` | `u8` |
| `u16be` | 2-byte big-endian unsigned integer | `u16` | `int` | `ushort` | `u16` |
| `u16le` | 2-byte little-endian unsigned integer | `u16` | `int` | `ushort` | `u16` |
| `u32be` | 4-byte big-endian unsigned integer | `u32` | `int` | `uint` | `u32` |
| `u32le` | 4-byte little-endian unsigned integer | `u32` | `int` | `uint` | `u32` |
| `u64be` | 8-byte big-endian unsigned integer | `u64` | `int` | `ulong` | `u64` |
| `u64le` | 8-byte little-endian unsigned integer | `u64` | `int` | `ulong` | `u64` |
| `varint` | Unsigned LEB128, 1 to 10 bytes | `u64` | `int` | `ulong` | `u64` |
| `bool` | 1 byte: `0x00` for false, `0x01` for true | `bool` | `bool` | `bool` | `bool` |
| `bytes` | Length prefix followed by raw bytes | `Vec<u8>` | `bytes` | `byte[]` | `[]const u8` |
| `string` | `varint` byte length followed by UTF-8 bytes | `String` | `str` | `string` | `[]const u8` |
| `guid` | 36-byte lowercase hyphenated UUID string | `String` | `str` | `Guid` | `[]const u8` |
| `bitflags<u8>` | 1-byte bitmask computed from `@flag` fields | Omitted from struct | Omitted from class | Omitted from class | Omitted from struct |

### Unsigned LEB128 varint rules

A `varint` encodes a 64-bit unsigned integer 7 bits per byte from least significant to most significant.
Bits 0 through 6 of each byte store payload bits.
Bit 7 is `1` when more bytes follow and `0` on the final byte.
Decoders reject varints that exceed 64 bits of shift as malformed input.

## Collection and optional types

| Schema type | Default wire encoding |
| :--- | :--- |
| `list<T>` | `varint` element count followed by each element encoded in order |
| `optional<T>` | Controlled by `@flag` or `@sentinel` annotation on the field |
| `optional<list<T>>` | Controlled by `@flag` annotation on the field |

Every `optional<T>` and `optional<list<T>>` field requires either a `@flag` annotation or a `@sentinel` annotation.

## Composite declarations

### Enum

An `enum` binds named variants to `u8` or `varint` discriminants.

```pain
enum HostOs : u8 {
	Linux   = 0x01,
	Windows = 0x02,
	MacOS   = 0x03,
}
```

Decoders reject discriminant values that do not match a declared variant.

### Struct

A `struct` groups fields that serialize sequentially without a leading type tag.

```pain
struct MultiplexFrame {
	channel_id: u32be;
	op: ChannelOp;
	payload: bytes @prefix(u16be) @max(65535);
}
```

### Choice

A `choice` is a tagged union with a `u8` or `varint` discriminant followed by the active variant fields.

```pain
choice JobArguments : u8 {
	None = 0 {},
	RawBytes = 1 {
		data: bytes;
	},
	StringList = 2 {
		items: list<string> @max(4096);
	},
}
```

### Message

A `message` is a top-level wire record prefixed by a 1-byte `@tag` value.
The tag argument accepts an integer literal or an `EnumName.Variant` reference.

```pain
message KeepAlive @tag(TelemetryMessageType.KeepAlive) {}
```

Calling `to_bytes` on a message writes the 1-byte tag followed by the encoded fields.
Calling `from_bytes` verifies that the first byte matches the declared tag before decoding the fields.

### Frame

A schema can declare at most one `frame` block.
The `frame` block defines a length-prefixed envelope around encoded messages.

```pain
frame RpcFrame {
	length: u32be @max(16777216);
	tag: RpcMessageType;
}
```

A `frame` requires two fields:

1. `length` with type `u16be`, `u16le`, `u32be`, `u32le`, or `varint`, and an optional `@max` bound.
2. `tag` referencing a declared `enum` type.

`encode_frame` takes an already-encoded message buffer, checks that its byte length does not exceed `@max`, and prepends the encoded length.
`decode_frame` reads the length prefix, validates `@max` and the remaining buffer length, reads the `tag` enum, and returns the tag alongside the message payload slice.

## Field annotations

| Annotation | Allowed field types | Wire behavior |
| :--- | :--- | :--- |
| `@prefix(u16be)` | `bytes`, `list<T>` | Uses a 2-byte big-endian length or element count prefix instead of `varint`. |
| `@prefix(u32be)` | `bytes`, `list<T>` | Uses a 4-byte big-endian length or element count prefix instead of `varint`. |
| `@prefix(varint)` | `bytes`, `list<T>` | Uses an unsigned LEB128 length or element count prefix. |
| `@max(N)` | `bytes`, `list<T>`, `frame.length` | Rejects lengths or counts greater than `N` during decoding and frame encoding. |
| `@sentinel("")` | `optional<string>` | Encodes an absent value as a 0-length string and decodes `""` as absent. |
| `@sentinel(0)` | `optional<varint>` | Encodes an absent value as `0` and decodes `0` as absent. |
| `@sentinel(empty)` | `optional<guid>` | Encodes an absent value as `00000000-0000-0000-0000-000000000000` and decodes that UUID as absent. |
| `@flag(header, mask)` | `bool`, `optional<T>`, `optional<list<T>>` | Binds the field to bit `mask` in the `bitflags<u8>` field named `header`. |
| `@tail` | Trailing fields in a `struct`, `message`, or `choice` variant | Writes trailing fields only when at least one `@tail` field is present or non-zero. Reads each `@tail` field only when unread bytes remain in the buffer. |

### Bitflags header behavior

A `bitflags<u8>` field is a wire-only header byte and does not appear as a property on generated structs or classes.

- **Inlined boolean.** When a `bool` field carries `@flag(flags, mask)`, the encoder sets `mask` in `flags` when the boolean is `true` and writes zero body bytes for that field.
- **Guarded optional value.** When an `optional<T>` field carries `@flag(flags, mask)`, the encoder sets `mask` in `flags` and writes the field body only when the value is present.
- **Guarded optional list.** When an `optional<list<T>>` field carries `@flag(flags, mask)`, the encoder sets `mask` in `flags` and writes the list only when the list is present and non-empty.

### Tail field group behavior

All `@tail` fields in a record must appear at the end of the field list.
The compiler rejects any non-`@tail` field that follows a `@tail` field.

When encoding a `@tail` group, the encoder checks whether any field in the group has a non-default value.
If all `@tail` fields are absent or zero, the encoder omits the entire tail group.
If at least one `@tail` field has a value, the encoder writes every field in the tail group in declaration order, using each field's sentinel or zero value for absent entries.

When decoding a `@tail` group, the decoder checks before each field whether unread bytes remain in the input slice.
If the input slice is at the end, the decoder assigns `None`, `null`, or `0` to the remaining `@tail` fields without raising an EOF error.

## Validation and error conditions

The compiler rejects schemas during `pain check` and `pain generate` when they violate any of the following rules:

- **Unknown annotation.** An annotation name is not one of `@tag`, `@prefix`, `@max`, `@sentinel`, `@flag`, or `@tail`.
- **Missing `@flag` target.** A `@flag(header, mask)` annotation references a `header` field that does not exist in the same record or does not have type `bitflags<u8>`.
- **Non-trailing `@tail` field.** A field without `@tail` appears after a field with `@tail` in the same field list.
- **Unannotated optional.** An `optional<T>` field has neither `@flag` nor `@sentinel`, and is not a `@tail` field with an implicit default.

Generated decoders reject malformed inputs with `Result::Err` in Rust, `ValueError` in Python, `InvalidDataException` in C#, and `WireError` in Zig under any of the following conditions:

- The input buffer ends before a required field or length prefix completes.
- A `varint` exceeds 64 bits.
- A `bytes`, `list<T>`, or `frame.length` prefix exceeds its `@max(N)` bound.
- A message tag or enum discriminant does not match a declared value.
- A `string` or `guid` field contains invalid UTF-8 bytes, or a `guid` fails UUID parsing in C#.
