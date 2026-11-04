use std::fs;
use std::process::Command;

use pain::codegen::CodeGenerator;
use pain::codegen::rust::RustGenerator;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

fn compile_and_run_rust(generated: &str, harness: &str, test_name: &str) {
    let dir = std::env::temp_dir().join(format!("pain_rust_test_{test_name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let src_path = dir.join("main.rs");
    let bin_path = dir.join("test_bin");
    fs::write(&src_path, format!("{generated}\n\n{harness}\n")).unwrap();

    let compile = Command::new("rustc")
        .arg("--edition=2021")
        .arg(&src_path)
        .arg("-o")
        .arg(&bin_path)
        .output()
        .unwrap();

    assert!(
        compile.status.success(),
        "rustc failed for {test_name}:\n{}",
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&bin_path).output().unwrap();
    assert!(
        run.status.success(),
        "test binary failed for {test_name}:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rust_codegen_roundtrips_core_fixture() {
    let ast = parse_schema(include_str!("fixtures/core.pain")).unwrap();
    let ir = lower_schema(&ast).unwrap();
    let code = RustGenerator.generate(&ir);

    let harness = r#"
fn main() {
    let h1 = Hello {
        session_id: "11111111-2222-3333-4444-555555555555".into(),
        mode: Mode::Safe,
        seq: 9001,
        nickname: None,
        retry_ms: None,
        parent_id: None,
        token: None,
    };
    let bytes = h1.to_bytes();
    assert_eq!(bytes[0], 0x01);
    let framed = encode_frame(&bytes).unwrap();
    let (tag, body) = decode_frame(&framed).unwrap();
    assert_eq!(tag, PacketType::Hello);
    assert_eq!(body, &bytes[1..]);
    let h1_dec = Hello::from_bytes(&bytes).unwrap();
    assert_eq!(h1, h1_dec);

    let h2 = Hello {
        session_id: "11111111-2222-3333-4444-555555555555".into(),
        mode: Mode::Fast,
        seq: 42,
        nickname: Some("neo".into()),
        retry_ms: Some(500),
        parent_id: Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()),
        token: Some("secret-token".into()),
    };
    assert_eq!(h2, Hello::from_bytes(&h2.to_bytes()).unwrap());

    let s = Summary {
        urgent: true,
        count: 7,
        featured: None,
        items: Some(vec![Item {
            id: 0xDEAD_BEEF,
            label: "item-1".into(),
            raw: vec![1, 2, 3, 4],
        }]),
    };
    let s_bytes = s.to_bytes();
    assert_eq!(s_bytes[0], 0x02);
    assert_eq!(s_bytes[1], 0x01 | 0x04);
    assert_eq!(s, Summary::from_bytes(&s_bytes).unwrap());

    let act = Action {
        id: "99999999-8888-7777-6666-555555555555".into(),
        payload: Payload::Tags(PayloadTags {
            values: vec!["alpha".into(), "beta".into()],
        }),
        ids: vec!["m1".into(), "m2".into()],
    };
    assert_eq!(act, Action::from_bytes(&act.to_bytes()).unwrap());
}
"#;

    compile_and_run_rust(&code, harness, "core");
}

#[test]
fn rust_codegen_compiles_and_roundtrips_example_schemas() {
    let rpc_ast = parse_schema(include_str!("../examples/rpc_session.pain")).unwrap();
    let rpc_ir = lower_schema(&rpc_ast).unwrap();
    let rpc_code = RustGenerator.generate(&rpc_ir);

    let rpc_harness = r#"
fn main() {
    let hs_no_token = Handshake {
        worker_id: "12345678-1234-1234-1234-123456789abc".into(),
        capabilities: vec!["http".into(), "tcp".into()],
        auth_token: None,
    };
    assert_eq!(hs_no_token, Handshake::from_bytes(&hs_no_token.to_bytes()).unwrap());

    let hs_with_token = Handshake {
        worker_id: "12345678-1234-1234-1234-123456789abc".into(),
        capabilities: vec!["http".into()],
        auth_token: Some("top-secret".into()),
    };
    assert_eq!(hs_with_token, Handshake::from_bytes(&hs_with_token.to_bytes()).unwrap());

    let jd = JobDispatch {
        job_id: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".into(),
        worker_id: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".into(),
        payload: JobPayload::Command(JobPayloadCommand {
            opcode: 10,
            arguments: vec!["status".into()],
            working_dir: None,
            digest_sha256: None,
            artifact_bytes: 0,
        }),
    };
    assert_eq!(jd, JobDispatch::from_bytes(&jd.to_bytes()).unwrap());
}
"#;
    compile_and_run_rust(&rpc_code, rpc_harness, "rpc_session");

    let tel_ast = parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap();
    let tel_ir = lower_schema(&tel_ast).unwrap();
    let tel_code = RustGenerator.generate(&tel_ir);

    let tel_harness = r#"
fn main() {
    let req = EnrollRequest {
        streaming: true,
        device_id: "11111111-1111-1111-1111-111111111111".into(),
        previous_session_id: None,
        os: HostOs::Linux,
        hostname: "node-01".into(),
        arch: HostArch::X86_64,
        public_key: vec![7; 32],
        flush_interval_sec: Some(5),
        inventory: Some(HostInventory {
            mount_points: vec!["/".into()],
            enabled_features: vec!["core".into()],
        }),
    };
    let bytes = req.to_bytes();
    assert_eq!(bytes[0], 0x01);
    assert_eq!(bytes[1], 0x01 | 0x02);
    assert_eq!(req, EnrollRequest::from_bytes(&bytes).unwrap());
}
"#;
    compile_and_run_rust(&tel_code, tel_harness, "telemetry_stream");
}
