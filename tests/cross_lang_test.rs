use std::fs;
use std::process::Command;

use pain::codegen::CodeGenerator;
use pain::codegen::csharp::CSharpGenerator;
use pain::codegen::python::PythonGenerator;
use pain::codegen::rust::RustGenerator;
use pain::codegen::zig::ZigGenerator;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

#[test]
fn cross_language_byte_for_byte_equivalence() {
    let dir = std::env::temp_dir().join("pain_cross_lang_test");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let core_ir = lower_schema(&parse_schema(include_str!("fixtures/core.pain")).unwrap()).unwrap();
    let rpc_ir =
        lower_schema(&parse_schema(include_str!("../examples/rpc_session.pain")).unwrap()).unwrap();
    let tel_ir =
        lower_schema(&parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap())
            .unwrap();

    let rust_core = RustGenerator.generate(&core_ir);
    let py_core = PythonGenerator.generate(&core_ir);
    let cs_core = CSharpGenerator.generate(&core_ir);
    let cs_rpc = CSharpGenerator.generate(&rpc_ir);
    let cs_tel = CSharpGenerator.generate(&tel_ir);
    let zig_core = ZigGenerator.generate(&core_ir);

    let rust_src = format!(
        r#"{rust_core}
fn to_hex(bytes: &[u8]) -> String {{
    bytes.iter().map(|b| format!("{{b:02x}}")).collect()
}}
fn main() {{
    let summary = Summary {{
        urgent: true,
        count: 300,
        featured: Some(Item {{
            id: 0x12345678,
            label: "primary".into(),
            raw: vec![0xAA, 0xBB, 0xCC],
        }}),
        items: Some(vec![Item {{
            id: 99,
            label: "sub".into(),
            raw: vec![0x01],
        }}]),
    }};
    let hello = Hello {{
        session_id: "11111111-2222-3333-4444-555555555555".into(),
        mode: Mode::Fast,
        seq: 1024,
        nickname: None,
        retry_ms: Some(250),
        parent_id: None,
        token: Some("tok-99".into()),
    }};
    let action = Action {{
        id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
        payload: Payload::Tags(PayloadTags {{
            values: vec!["x".into(), "y".into()],
        }}),
        ids: vec!["one".into(), "two".into()],
    }};
    println!("{{}}", to_hex(&encode_frame(&summary.to_bytes()).unwrap()));
    println!("{{}}", to_hex(&encode_frame(&hello.to_bytes()).unwrap()));
    println!("{{}}", to_hex(&encode_frame(&action.to_bytes()).unwrap()));
}}
"#
    );

    let rust_file = dir.join("emit.rs");
    let rust_bin = dir.join("emit_bin");
    fs::write(&rust_file, rust_src).unwrap();
    let rustc = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&rust_file)
        .arg("-o")
        .arg(&rust_bin)
        .output()
        .unwrap();
    assert!(
        rustc.status.success(),
        "rustc failed: {}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let rust_out = String::from_utf8(Command::new(&rust_bin).output().unwrap().stdout).unwrap();
    let rust_lines: Vec<&str> = rust_out.lines().collect();
    assert_eq!(rust_lines.len(), 3);

    let py_src = format!(
        r#"{py_core}
summary = Summary(
    urgent=True,
    count=300,
    featured=Item(id=0x12345678, label="primary", raw=bytes([0xAA, 0xBB, 0xCC])),
    items=[Item(id=99, label="sub", raw=bytes([0x01]))],
)
hello = Hello(
    session_id="11111111-2222-3333-4444-555555555555",
    mode=Mode.Fast,
    seq=1024,
    nickname=None,
    retry_ms=250,
    parent_id=None,
    token="tok-99",
)
action = Action(
    id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    payload=Payload.Tags(values=["x", "y"]),
    ids=["one", "two"],
)
print(encode_frame(summary.to_bytes()).hex())
print(encode_frame(hello.to_bytes()).hex())
print(encode_frame(action.to_bytes()).hex())
"#
    );
    let py_file = dir.join("emit.py");
    fs::write(&py_file, py_src).unwrap();
    let py_run = Command::new("python3").arg(&py_file).output().unwrap();
    assert!(
        py_run.status.success(),
        "python3 failed: {}",
        String::from_utf8_lossy(&py_run.stderr)
    );
    let py_out = String::from_utf8(py_run.stdout).unwrap();
    let py_lines: Vec<&str> = py_out.lines().collect();
    assert_eq!(rust_lines, py_lines);

    let cs_dir = dir.join("cs_proj");
    fs::create_dir_all(&cs_dir).unwrap();
    fs::write(
        cs_dir.join("Test.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>disable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
  </PropertyGroup>
</Project>
"#,
    )
    .unwrap();
    fs::write(cs_dir.join("Core.g.cs"), cs_core).unwrap();
    fs::write(cs_dir.join("RpcSession.g.cs"), cs_rpc).unwrap();
    fs::write(cs_dir.join("TelemetryStream.g.cs"), cs_tel).unwrap();
    fs::write(
        cs_dir.join("Program.cs"),
        format!(
            r#"using System;
using System.Collections.Generic;
using Test.Core;
using Example.Rpc.Protocol;

var summary = new Summary
{{
    Urgent = true,
    Count = 300,
    Featured = new Item
    {{
        Id = 0x12345678,
        Label = "primary",
        Raw = new byte[] {{ 0xAA, 0xBB, 0xCC }},
    }},
    Items = new List<Item>
    {{
        new Item {{ Id = 99, Label = "sub", Raw = new byte[] {{ 0x01 }} }}
    }},
}};
var hello = new Hello
{{
    SessionId = Guid.Parse("11111111-2222-3333-4444-555555555555"),
    Mode = Mode.Fast,
    Seq = 1024,
    Nickname = null,
    RetryMs = 250,
    ParentId = null,
    Token = "tok-99",
}};
var action = new Test.Core.Action
{{
    Id = Guid.Parse("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
    Payload = new PayloadTags {{ Values = new List<string> {{ "x", "y" }} }},
    Ids = new List<string> {{ "one", "two" }},
}};

var sFramed = PacketFrame.EncodeFrame(summary.ToBytes());
var hFramed = PacketFrame.EncodeFrame(hello.ToBytes());
var aFramed = PacketFrame.EncodeFrame(action.ToBytes());

var (sTag, sBody) = PacketFrame.DecodeFrame(Convert.FromHexString("{}"));
if (sTag != PacketType.Summary) throw new Exception("Bad tag");
var sDec = Summary.FromBytes(summary.ToBytes());
if (!sDec.Urgent || sDec.Count != 300 || sDec.Featured?.Label != "primary") throw new Exception("Mismatch");

var hs = new Handshake
{{
    WorkerId = Guid.Parse("12345678-1234-1234-1234-123456789abc"),
    Capabilities = new List<string> {{ "tcp", "http" }},
    AuthToken = "s3cr3t",
}};
var hsRoundtrip = Handshake.FromBytes(hs.ToBytes());
if (hsRoundtrip.AuthToken != "s3cr3t" || hsRoundtrip.Capabilities.Count != 2)
    throw new Exception("Handshake roundtrip failed");

Console.WriteLine(Convert.ToHexString(sFramed).ToLowerInvariant());
Console.WriteLine(Convert.ToHexString(hFramed).ToLowerInvariant());
Console.WriteLine(Convert.ToHexString(aFramed).ToLowerInvariant());
"#,
            rust_lines[0]
        ),
    )
    .unwrap();

    let cs_run = Command::new("dotnet")
        .arg("run")
        .arg("--project")
        .arg(cs_dir.join("Test.csproj"))
        .output()
        .unwrap();
    assert!(
        cs_run.status.success(),
        "dotnet run failed:\n{}\n{}",
        String::from_utf8_lossy(&cs_run.stdout),
        String::from_utf8_lossy(&cs_run.stderr)
    );
    let cs_out = String::from_utf8(cs_run.stdout).unwrap();
    let cs_lines: Vec<&str> = cs_out.lines().collect();
    assert_eq!(rust_lines, cs_lines);

    let zig_src = format!(
        r#"{zig_core}
pub fn main() !void {{
    var arena = std.heap.ArenaAllocator.init(std.heap.page_allocator);
    defer arena.deinit();
    const alloc = arena.allocator();
    const stdout = std.io.getStdOut().writer();

    const items_list = [_]Item{{
        .{{ .id = 99, .label = "sub", .raw = &[_]u8{{0x01}} }},
    }};
    const summary = Summary{{
        .urgent = true,
        .count = 300,
        .featured = .{{
            .id = 0x12345678,
            .label = "primary",
            .raw = &[_]u8{{ 0xAA, 0xBB, 0xCC }},
        }},
        .items = &items_list,
    }};
    const hello = Hello{{
        .session_id = "11111111-2222-3333-4444-555555555555",
        .mode = .Fast,
        .seq = 1024,
        .nickname = null,
        .retry_ms = 250,
        .parent_id = null,
        .token = "tok-99",
    }};
    const tag_vals = [_][]const u8{{ "x", "y" }};
    const id_vals = [_][]const u8{{ "one", "two" }};
    const action = Action{{
        .id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        .payload = .{{ .Tags = .{{ .values = &tag_vals }} }},
        .ids = &id_vals,
    }};

    const f1 = try PacketFrame.encodeFrame(alloc, try summary.toBytes(alloc));
    const f2 = try PacketFrame.encodeFrame(alloc, try hello.toBytes(alloc));
    const f3 = try PacketFrame.encodeFrame(alloc, try action.toBytes(alloc));

    try stdout.print("{{}}\n", .{{std.fmt.fmtSliceHexLower(f1)}});
    try stdout.print("{{}}\n", .{{std.fmt.fmtSliceHexLower(f2)}});
    try stdout.print("{{}}\n", .{{std.fmt.fmtSliceHexLower(f3)}});
}}
"#
    );
    let zig_file = dir.join("emit.zig");
    fs::write(&zig_file, zig_src).unwrap();
    let zig_run = Command::new("zig")
        .arg("run")
        .arg(&zig_file)
        .output()
        .unwrap();
    assert!(
        zig_run.status.success(),
        "zig run failed:\n{}\n{}",
        String::from_utf8_lossy(&zig_run.stdout),
        String::from_utf8_lossy(&zig_run.stderr)
    );
    let zig_out = String::from_utf8(zig_run.stdout).unwrap();
    let zig_lines: Vec<&str> = zig_out.lines().collect();
    assert_eq!(rust_lines, zig_lines);

    let _ = fs::remove_dir_all(&dir);
}
