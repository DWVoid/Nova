use super::Parser;
use crate::ast::{ArgsKind, DefExpr, ExpKind, StatKind, TopItem, VarKind};
use crate::lexer::Lexer;

fn parse_chunk(input: &str) -> crate::ast::Chunk {
    let tokens = Lexer::new(input).lex_all().unwrap();
    Parser::new(tokens).parse_chunk().unwrap()
}

#[test]
fn parses_minimal_compilation_unit() {
    let src = r#"
        namespace Example;
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.uses.len(), 0);
    assert_eq!(chunk.namespace.path.len(), 1);
    assert!(chunk.items.is_empty());
}

#[test]
fn parses_use_decl_with_selector() {
    let src = r#"
        use Present.{State, Canvas as C};
        namespace Example;
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.uses.len(), 1);
    let use_decl = &chunk.uses[0];
    assert_eq!(use_decl.path.len(), 1);
    assert!(use_decl.tail.is_some());
}

#[test]
fn parses_simple_definition_lambda() {
    let src = r#"
        namespace Example;
        define f (): unit
            return 1
        end
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.items.len(), 1);
    match &chunk.items[0] {
        TopItem::Definition(def) => match &def.expr {
            DefExpr::Exp(exp) => match &exp.kind {
                ExpKind::Lambda(_) => {}
                _ => panic!("expected lambda expression"),
            },
            _ => panic!("expected expression definition"),
        },
        _ => panic!("expected definition"),
    }
}

#[test]
fn parses_struct_enum_variant_trait() {
    let src = r#"
        namespace Example;
        define A struct
            x: integer;
        end
        define E enum: integer
            A = 1;
        end
        define V variant
            A: integer;
        end
        define T trait
            method(a: integer): unit;
        end
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.items.len(), 4);
}

#[test]
fn parses_assign_with_var_decl() {
    let src = r#"
        namespace Example;
        define f (): unit
            var x: integer = 1
            val y = 2
            x = x + y
            return x
        end
    "#;
    let chunk = parse_chunk(src);
    let def = match &chunk.items[0] {
        TopItem::Definition(def) => def,
        _ => panic!("expected definition"),
    };
    let DefExpr::Exp(exp) = &def.expr else { panic!("expected exp"); };
    let ExpKind::Lambda(lambda) = &exp.kind else { panic!("expected lambda"); };
    let stat = &lambda.block.stats[0];
    match &stat.kind {
        StatKind::Assign { vars, .. } => match &vars[0].kind {
            VarKind::Decl { .. } => {}
            _ => panic!("expected var decl"),
        },
        _ => panic!("expected assign"),
    }
}

#[test]
fn parses_invoke_and_method_call() {
    let src = r#"
        namespace Example;
        define f (): unit
            a(1, 2)
            obj:method({})
        end
    "#;
    let chunk = parse_chunk(src);
    let def = match &chunk.items[0] {
        TopItem::Definition(def) => def,
        _ => panic!("expected definition"),
    };
    let DefExpr::Exp(exp) = &def.expr else { panic!("expected exp"); };
    let ExpKind::Lambda(lambda) = &exp.kind else { panic!("expected lambda"); };
    assert_eq!(lambda.block.stats.len(), 2);
    match &lambda.block.stats[0].kind {
        StatKind::Call { call } => match &call.args.kind {
            ArgsKind::ExpList(list) => assert_eq!(list.len(), 2),
            _ => panic!("expected exp list"),
        },
        _ => panic!("expected call stat"),
    }
}

#[test]
fn parses_control_flow_and_continue() {
    let src = r#"
        namespace Example;
        define f (): unit
            for i = 1, 3 do
                if i == 2 then
                    continue
                end
            end
            return 0
        end
    "#;
    let chunk = parse_chunk(src);
    let def = match &chunk.items[0] {
        TopItem::Definition(def) => def,
        _ => panic!("expected definition"),
    };
    let DefExpr::Exp(exp) = &def.expr else { panic!("expected exp"); };
    let ExpKind::Lambda(lambda) = &exp.kind else { panic!("expected lambda"); };
    assert!(lambda.block.ret.is_some());
}

#[test]
fn collects_comments_at_chunk_level() {
    let src = r#"
        -- leading
        namespace Example;
        -- trailing
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.comments.len(), 2);
}