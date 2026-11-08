use std::fs;
use std::process::Command;

use pain::codegen::CodeGenerator;
use pain::codegen::go::GoGenerator;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

fn run_go_test(generated: &str, pkg: &str, harness: &str, test_name: &str) {
    let dir = std::env::temp_dir().join(format!("pain_go_test_{test_name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    fs::write(dir.join("go.mod"), "module pain_test\n\ngo 1.21\n").unwrap();
    fs::write(dir.join("wire.go"), generated).unwrap();
    fs::write(
        dir.join("wire_test.go"),
        format!("package {pkg}\n\n{harness}\n"),
    )
    .unwrap();

    let vet = Command::new("go")
        .arg("vet")
        .arg("./...")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        vet.status.success(),
        "go vet failed for {test_name}:\n{}\n{}",
        String::from_utf8_lossy(&vet.stdout),
        String::from_utf8_lossy(&vet.stderr)
    );

    let run = Command::new("go")
        .arg("test")
        .arg("./...")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "go test failed for {test_name}:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn go_codegen_roundtrips_core_fixture() {
    let ast = parse_schema(include_str!("fixtures/core.pain")).unwrap();
    let ir = lower_schema(&ast).unwrap();
    let code = GoGenerator.generate(&ir);

    let harness = r#"import (
    "bytes"
    "reflect"
    "testing"
)

func TestCoreRoundtrips(t *testing.T) {
    h1 := Hello{
        SessionId: "11111111-2222-3333-4444-555555555555",
        Mode:      ModeSafe,
        Seq:       9001,
        Nickname:  nil,
        RetryMs:   nil,
        ParentId:  nil,
        Token:     nil,
    }
    b1 := h1.ToBytes()
    if b1[0] != 0x01 {
        t.Fatalf("expected tag 0x01, got 0x%02x", b1[0])
    }
    framed, err := EncodeFrame(b1)
    if err != nil {
        t.Fatal(err)
    }
    tag, body, err := DecodeFrame(framed)
    if err != nil {
        t.Fatal(err)
    }
    if tag != PacketTypeHello {
        t.Fatalf("expected PacketTypeHello, got %v", tag)
    }
    if !bytes.Equal(body, b1[1:]) {
        t.Fatalf("frame body mismatch")
    }
    h1Dec, err := HelloFromBytes(b1)
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(h1, h1Dec) {
        t.Fatalf("h1 roundtrip mismatch: %+v != %+v", h1, h1Dec)
    }

    h2 := Hello{
        SessionId: "11111111-2222-3333-4444-555555555555",
        Mode:      ModeFast,
        Seq:       42,
        Nickname:  Ptr("neo"),
        RetryMs:   Ptr[uint64](500),
        ParentId:  Ptr("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        Token:     Ptr("secret-token"),
    }
    h2Dec, err := HelloFromBytes(h2.ToBytes())
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(h2, h2Dec) {
        t.Fatalf("h2 roundtrip mismatch: %+v != %+v", h2, h2Dec)
    }

    s := Summary{
        Urgent:   true,
        Count:    7,
        Featured: nil,
        Items: []Item{
            {
                Id:    0xDEADBEEF,
                Label: "item-1",
                Raw:   []byte{1, 2, 3, 4},
            },
        },
    }
    sb := s.ToBytes()
    if sb[0] != 0x02 || sb[1] != (0x01|0x04) {
        t.Fatalf("unexpected summary header bytes: %x", sb[:2])
    }
    sDec, err := SummaryFromBytes(sb)
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(s, sDec) {
        t.Fatalf("summary roundtrip mismatch: %+v != %+v", s, sDec)
    }

    act := Action{
        Id: "99999999-8888-7777-6666-555555555555",
        Payload: PayloadTags{
            Values: []string{"alpha", "beta"},
        },
        Ids: []string{"m1", "m2"},
    }
    actDec, err := ActionFromBytes(act.ToBytes())
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(act, actDec) {
        t.Fatalf("action roundtrip mismatch: %+v != %+v", act, actDec)
    }
}
"#;

    run_go_test(&code, "core", harness, "core");
}

#[test]
fn go_codegen_compiles_and_roundtrips_example_schemas() {
    let rpc_ast = parse_schema(include_str!("../examples/rpc_session.pain")).unwrap();
    let rpc_ir = lower_schema(&rpc_ast).unwrap();
    let rpc_code = GoGenerator.generate(&rpc_ir);

    let rpc_harness = r#"import (
    "reflect"
    "testing"
)

func TestRpcSessionRoundtrips(t *testing.T) {
    hsNoToken := Handshake{
        WorkerId:     "12345678-1234-1234-1234-123456789abc",
        Capabilities: []string{"http", "tcp"},
        AuthToken:    nil,
    }
    hs1Dec, err := HandshakeFromBytes(hsNoToken.ToBytes())
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(hsNoToken, hs1Dec) {
        t.Fatalf("hsNoToken mismatch: %+v != %+v", hsNoToken, hs1Dec)
    }

    hsWithToken := Handshake{
        WorkerId:     "12345678-1234-1234-1234-123456789abc",
        Capabilities: []string{"http"},
        AuthToken:    Ptr("top-secret"),
    }
    hs2Dec, err := HandshakeFromBytes(hsWithToken.ToBytes())
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(hsWithToken, hs2Dec) {
        t.Fatalf("hsWithToken mismatch: %+v != %+v", hsWithToken, hs2Dec)
    }

    jd := JobDispatch{
        JobId:    "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        WorkerId: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        Payload: JobPayloadCommand{
            Opcode:        10,
            Arguments:     []string{"status"},
            WorkingDir:    nil,
            DigestSha256:  nil,
            ArtifactBytes: 0,
        },
    }
    jdDec, err := JobDispatchFromBytes(jd.ToBytes())
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(jd, jdDec) {
        t.Fatalf("JobDispatch mismatch: %+v != %+v", jd, jdDec)
    }
}
"#;
    run_go_test(&rpc_code, "protocol", rpc_harness, "rpc_session");

    let tel_ast = parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap();
    let tel_ir = lower_schema(&tel_ast).unwrap();
    let tel_code = GoGenerator.generate(&tel_ir);

    let tel_harness = r#"import (
    "bytes"
    "reflect"
    "testing"
)

func TestTelemetryStreamRoundtrips(t *testing.T) {
    req := EnrollRequest{
        Streaming:         true,
        DeviceId:          "11111111-1111-1111-1111-111111111111",
        PreviousSessionId: nil,
        Os:                HostOsLinux,
        Hostname:          "node-01",
        Arch:              HostArchX86_64,
        PublicKey:         bytes.Repeat([]byte{7}, 32),
        FlushIntervalSec:  Ptr[uint64](5),
        Inventory: &HostInventory{
            MountPoints:     []string{"/"},
            EnabledFeatures: []string{"core"},
        },
    }
    raw := req.ToBytes()
    if raw[0] != 0x01 || raw[1] != (0x01|0x02) {
        t.Fatalf("unexpected header bytes: %x", raw[:2])
    }
    reqDec, err := EnrollRequestFromBytes(raw)
    if err != nil {
        t.Fatal(err)
    }
    if !reflect.DeepEqual(req, reqDec) {
        t.Fatalf("EnrollRequest mismatch: %+v != %+v", req, reqDec)
    }
}
"#;
    run_go_test(&tel_code, "protocol", tel_harness, "telemetry_stream");
}
