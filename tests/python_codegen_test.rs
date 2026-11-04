use std::fs;
use std::process::Command;

use pain::codegen::CodeGenerator;
use pain::codegen::python::PythonGenerator;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

fn run_python(generated: &str, harness: &str, test_name: &str) {
    let dir = std::env::temp_dir().join(format!("pain_py_test_{test_name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let script_path = dir.join("test_mod.py");
    fs::write(&script_path, format!("{generated}\n\n{harness}\n")).unwrap();

    let run = Command::new("python3").arg(&script_path).output().unwrap();
    assert!(
        run.status.success(),
        "python3 failed for {test_name}:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn python_codegen_roundtrips_core_fixture() {
    let ast = parse_schema(include_str!("fixtures/core.pain")).unwrap();
    let ir = lower_schema(&ast).unwrap();
    let code = PythonGenerator.generate(&ir);

    let harness = r#"
h1 = Hello(
    session_id="11111111-2222-3333-4444-555555555555",
    mode=Mode.Safe,
    seq=9001,
    nickname=None,
    retry_ms=None,
    parent_id=None,
    token=None,
)
b1 = h1.to_bytes()
assert b1[0] == 0x01
framed = encode_frame(b1)
tag, body = decode_frame(framed)
assert tag == PacketType.Hello
assert body == b1[1:]
assert Hello.from_bytes(b1) == h1

h2 = Hello(
    session_id="11111111-2222-3333-4444-555555555555",
    mode=Mode.Fast,
    seq=42,
    nickname="neo",
    retry_ms=500,
    parent_id="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
    token="secret-token",
)
assert Hello.from_bytes(h2.to_bytes()) == h2

s = Summary(
    urgent=True,
    count=7,
    featured=None,
    items=[Item(id=0xDEADBEEF, label="item-1", raw=b"\x01\x02\x03\x04")],
)
sb = s.to_bytes()
assert sb[0] == 0x02
assert sb[1] == (0x01 | 0x04)
assert Summary.from_bytes(sb) == s

act = Action(
    id="99999999-8888-7777-6666-555555555555",
    payload=Payload.Tags(values=["alpha", "beta"]),
    ids=["m1", "m2"],
)
assert Action.from_bytes(act.to_bytes()) == act
"#;

    run_python(&code, harness, "core");
}

#[test]
fn python_codegen_roundtrips_example_schemas() {
    let rpc_ast = parse_schema(include_str!("../examples/rpc_session.pain")).unwrap();
    let rpc_ir = lower_schema(&rpc_ast).unwrap();
    let rpc_code = PythonGenerator.generate(&rpc_ir);

    let rpc_harness = r#"
hs_msg = Handshake(
    worker_id="12345678-1234-1234-1234-123456789abc",
    capabilities=["http", "tcp"],
    auth_token="top-secret",
)
assert Handshake.from_bytes(hs_msg.to_bytes()) == hs_msg

jd = JobDispatch(
    job_id="aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
    worker_id="bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
    payload=JobPayload.Plugin(
        plugin_kind=12,
        abi_version="1.0",
        size=1024,
        digest_sha256="deadbeef",
        artifact_bytes=1024,
        arguments=JobArguments.RawBytes(data=b"\xca\xfe"),
    ),
)
assert JobDispatch.from_bytes(jd.to_bytes()) == jd
"#;
    run_python(&rpc_code, rpc_harness, "rpc_session");

    let tel_ast = parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap();
    let tel_ir = lower_schema(&tel_ast).unwrap();
    let tel_code = PythonGenerator.generate(&tel_ir);

    let tel_harness = r#"
req = EnrollRequest(
    streaming=True,
    device_id="11111111-1111-1111-1111-111111111111",
    previous_session_id=None,
    os=HostOs.Linux,
    hostname="node-01",
    arch=HostArch.X86_64,
    public_key=b"\x07" * 32,
    flush_interval_sec=5,
    inventory=HostInventory(
        mount_points=["/"],
        enabled_features=["core"],
    ),
)
raw = req.to_bytes()
assert raw[0] == 0x01
assert raw[1] == (0x01 | 0x02)
assert EnrollRequest.from_bytes(raw) == req
"#;
    run_python(&tel_code, tel_harness, "telemetry_stream");
}
