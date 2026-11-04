use pain::ast::Decl;
use pain::parser::parse_schema;

#[test]
fn parses_core_ci_fixture() {
    let src = include_str!("fixtures/core.pain");
    let ast = parse_schema(src).unwrap();
    assert_eq!(ast.package, "test.core");
    assert_eq!(ast.decls.len(), 8);
}

#[test]
fn parses_rpc_session_example() {
    let src = include_str!("../examples/rpc_session.pain");
    let ast = parse_schema(src).unwrap();
    assert_eq!(ast.package, "Example.Rpc.Protocol");

    let messages = ast
        .decls
        .iter()
        .filter(|d| matches!(d, Decl::Message(_)))
        .count();
    assert_eq!(messages, 9);
}

#[test]
fn parses_telemetry_stream_example() {
    let src = include_str!("../examples/telemetry_stream.pain");
    let ast = parse_schema(src).unwrap();
    assert_eq!(ast.package, "Example.Telemetry.Protocol");

    let messages = ast
        .decls
        .iter()
        .filter(|d| matches!(d, Decl::Message(_)))
        .count();
    assert_eq!(messages, 6);
}

#[test]
fn reports_line_and_col_on_syntax_error() {
    let bad = "schema test;\nstruct Foo {\n  x: u8 @bogus;\n}\n";
    let err = parse_schema(bad).unwrap_err();
    assert_eq!(err.line, 3);
    assert!(err.message.contains("unknown annotation '@bogus'"));
}
