use std::fs;
use std::process::Command;

use pain::codegen::CodeGenerator;
use pain::codegen::zig::ZigGenerator;
use pain::lower::lower_schema;
use pain::parser::parse_schema;

fn run_zig_test(generated: &str, harness: &str, test_name: &str) {
    let dir = std::env::temp_dir().join(format!("pain_zig_test_{test_name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    let src_path = dir.join("test_wire.zig");
    fs::write(&src_path, format!("{generated}\n\n{harness}\n")).unwrap();

    let run = Command::new("zig")
        .arg("test")
        .arg(&src_path)
        .output()
        .unwrap();

    assert!(
        run.status.success(),
        "zig test failed for {test_name}:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn zig_codegen_roundtrips_core_fixture() {
    let ast = parse_schema(include_str!("fixtures/core.pain")).unwrap();
    let ir = lower_schema(&ast).unwrap();
    let code = ZigGenerator.generate(&ir);

    let harness = r#"
test "core roundtrips" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const alloc = arena.allocator();

    const h1 = Hello{
        .session_id = "11111111-2222-3333-4444-555555555555",
        .mode = .Safe,
        .seq = 9001,
        .nickname = null,
        .retry_ms = null,
        .parent_id = null,
        .token = null,
    };
    const h1_bytes = try h1.toBytes(alloc);
    try std.testing.expectEqual(@as(u8, 0x01), h1_bytes[0]);

    const framed = try PacketFrame.encodeFrame(alloc, h1_bytes);
    const decoded_frame = try PacketFrame.decodeFrame(framed);
    try std.testing.expectEqual(PacketType.Hello, decoded_frame.tag);
    try std.testing.expectEqualSlices(u8, h1_bytes[1..], decoded_frame.body);

    const h1_dec = try Hello.fromBytes(h1_bytes, alloc);
    try std.testing.expectEqualStrings(h1.session_id, h1_dec.session_id);
    try std.testing.expectEqual(h1.mode, h1_dec.mode);
    try std.testing.expectEqual(h1.seq, h1_dec.seq);
    try std.testing.expect(h1_dec.nickname == null);
    try std.testing.expect(h1_dec.retry_ms == null);
    try std.testing.expect(h1_dec.parent_id == null);
    try std.testing.expect(h1_dec.token == null);

    const h2 = Hello{
        .session_id = "11111111-2222-3333-4444-555555555555",
        .mode = .Fast,
        .seq = 42,
        .nickname = "neo",
        .retry_ms = 500,
        .parent_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
        .token = "secret-token",
    };
    const h2_dec = try Hello.fromBytes(try h2.toBytes(alloc), alloc);
    try std.testing.expectEqualStrings("neo", h2_dec.nickname.?);
    try std.testing.expectEqual(@as(u64, 500), h2_dec.retry_ms.?);
    try std.testing.expectEqualStrings("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", h2_dec.parent_id.?);
    try std.testing.expectEqualStrings("secret-token", h2_dec.token.?);

    const items_list = [_]Item{
        .{ .id = 0xDEADBEEF, .label = "item-1", .raw = &[_]u8{ 1, 2, 3, 4 } },
    };
    const s = Summary{
        .urgent = true,
        .count = 7,
        .featured = null,
        .items = &items_list,
    };
    const s_bytes = try s.toBytes(alloc);
    try std.testing.expectEqual(@as(u8, 0x02), s_bytes[0]);
    try std.testing.expectEqual(@as(u8, 0x01 | 0x04), s_bytes[1]);
    const s_dec = try Summary.fromBytes(s_bytes, alloc);
    try std.testing.expect(s_dec.urgent);
    try std.testing.expectEqual(@as(u16, 7), s_dec.count);
    try std.testing.expect(s_dec.featured == null);
    try std.testing.expectEqual(@as(usize, 1), s_dec.items.?.len);
    try std.testing.expectEqualStrings("item-1", s_dec.items.?[0].label);

    const tag_vals = [_][]const u8{ "alpha", "beta" };
    const id_vals = [_][]const u8{ "m1", "m2" };
    const act = Action{
        .id = "99999999-8888-7777-6666-555555555555",
        .payload = .{ .Tags = .{ .values = &tag_vals } },
        .ids = &id_vals,
    };
    const act_dec = try Action.fromBytes(try act.toBytes(alloc), alloc);
    try std.testing.expectEqualStrings("99999999-8888-7777-6666-555555555555", act_dec.id);
    try std.testing.expectEqual(@as(usize, 2), act_dec.ids.len);
    switch (act_dec.payload) {
        .Tags => |t| try std.testing.expectEqualStrings("beta", t.values[1]),
        else => return error.TestUnexpectedResult,
    }
}
"#;

    run_zig_test(&code, harness, "core");
}

#[test]
fn zig_codegen_compiles_and_roundtrips_example_schemas() {
    let rpc_ast = parse_schema(include_str!("../examples/rpc_session.pain")).unwrap();
    let rpc_ir = lower_schema(&rpc_ast).unwrap();
    let rpc_code = ZigGenerator.generate(&rpc_ir);

    let rpc_harness = r#"
test "rpc session roundtrips" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const alloc = arena.allocator();

    const caps = [_][]const u8{ "http", "tcp" };
    const hs_no_token = Handshake{
        .worker_id = "12345678-1234-1234-1234-123456789abc",
        .capabilities = &caps,
        .auth_token = null,
    };
    const hs1_dec = try Handshake.fromBytes(try hs_no_token.toBytes(alloc), alloc);
    try std.testing.expect(hs1_dec.auth_token == null);
    try std.testing.expectEqual(@as(usize, 2), hs1_dec.capabilities.len);

    const hs_with_token = Handshake{
        .worker_id = "12345678-1234-1234-1234-123456789abc",
        .capabilities = &caps,
        .auth_token = "top-secret",
    };
    const hs2_dec = try Handshake.fromBytes(try hs_with_token.toBytes(alloc), alloc);
    try std.testing.expectEqualStrings("top-secret", hs2_dec.auth_token.?);

    const cmd_args = [_][]const u8{"status"};
    const jd = JobDispatch{
        .job_id = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        .worker_id = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
        .payload = .{
            .Command = .{
                .opcode = 10,
                .arguments = &cmd_args,
                .working_dir = null,
                .digest_sha256 = null,
                .artifact_bytes = 0,
            },
        },
    };
    const jd_dec = try JobDispatch.fromBytes(try jd.toBytes(alloc), alloc);
    try std.testing.expectEqualStrings(jd.job_id, jd_dec.job_id);
}
"#;
    run_zig_test(&rpc_code, rpc_harness, "rpc_session");

    let tel_ast = parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap();
    let tel_ir = lower_schema(&tel_ast).unwrap();
    let tel_code = ZigGenerator.generate(&tel_ir);

    let tel_harness = r#"
test "telemetry stream roundtrips" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const alloc = arena.allocator();

    const mounts = [_][]const u8{"/"};
    const feats = [_][]const u8{"core"};
    const pubkey = [_]u8{7} ** 32;
    const req = EnrollRequest{
        .streaming = true,
        .device_id = "11111111-1111-1111-1111-111111111111",
        .previous_session_id = null,
        .os = .Linux,
        .hostname = "node-01",
        .arch = .X86_64,
        .public_key = &pubkey,
        .flush_interval_sec = 5,
        .inventory = .{
            .mount_points = &mounts,
            .enabled_features = &feats,
        },
    };
    const bytes = try req.toBytes(alloc);
    try std.testing.expectEqual(@as(u8, 0x01), bytes[0]);
    try std.testing.expectEqual(@as(u8, 0x01 | 0x02), bytes[1]);
    const req_dec = try EnrollRequest.fromBytes(bytes, alloc);
    try std.testing.expect(req_dec.streaming);
    try std.testing.expectEqual(HostOs.Linux, req_dec.os);
    try std.testing.expectEqual(@as(u64, 5), req_dec.flush_interval_sec.?);
}
"#;
    run_zig_test(&tel_code, tel_harness, "telemetry_stream");
}
