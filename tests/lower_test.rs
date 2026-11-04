use pain::ir::{BitflagBindingKind, WireOp};
use pain::lower::lower_schema;
use pain::parser::parse_schema;

#[test]
fn lowers_core_ci_fixture() {
    let ast = parse_schema(include_str!("fixtures/core.pain")).unwrap();
    let ir = lower_schema(&ast).unwrap();

    assert!(ir.frame.is_some());
    assert_eq!(ir.enums.len(), 2);
    assert_eq!(ir.structs.len(), 1);
    assert_eq!(ir.choices.len(), 1);
    assert_eq!(ir.messages.len(), 3);

    let summary = ir.messages.iter().find(|m| m.name == "Summary").unwrap();
    assert_eq!(summary.fields.len(), 4);
    assert!(matches!(
        &summary.ops[1],
        WireOp::BitflagsHeader { links, .. }
            if links.len() == 3
                && links[0].kind == BitflagBindingKind::InlinedBool
                && links[1].kind == BitflagBindingKind::OptionalValue
                && links[2].kind == BitflagBindingKind::OptionalList
    ));
}

#[test]
fn lowers_example_schemas() {
    let rpc_ast = parse_schema(include_str!("../examples/rpc_session.pain")).unwrap();
    let rpc_ir = lower_schema(&rpc_ast).unwrap();
    assert_eq!(rpc_ir.messages.len(), 9);

    let handshake = rpc_ir
        .messages
        .iter()
        .find(|m| m.name == "Handshake")
        .unwrap();
    assert!(
        matches!(handshake.ops.last().unwrap(), WireOp::TailGroup { fields } if fields.len() == 1)
    );

    let tel_ast = parse_schema(include_str!("../examples/telemetry_stream.pain")).unwrap();
    let tel_ir = lower_schema(&tel_ast).unwrap();
    assert_eq!(tel_ir.messages.len(), 6);
}

#[test]
fn rejects_non_tail_after_tail_field() {
    let src =
        "schema bad;\nstruct S {\n  a: optional<string> @tail @sentinel(\"\");\n  b: u8;\n}\n";
    let ast = parse_schema(src).unwrap();
    let err = lower_schema(&ast).unwrap_err();
    assert!(
        err.message
            .contains("non-@tail field cannot follow a @tail field")
    );
}
