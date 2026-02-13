use super::Parser;
use crate::ast::{ArgsKind, ExpKind, StatKind};
use crate::lexer::Lexer;

fn parse_chunk(input: &str) -> crate::ast::Chunk {
    let tokens = Lexer::new(input).lex_all().unwrap();
    Parser::new(tokens).parse_chunk().unwrap()
}

#[test]
fn parses_empty_chunk() {
    let chunk = parse_chunk("");
    assert!(chunk.block.stats.is_empty());
}

#[test]
fn parses_empty_statement() {
    let chunk = parse_chunk(";");
    assert_eq!(chunk.block.stats.len(), 1);
}

#[test]
fn parses_return_without_expr() {
    let chunk = parse_chunk("return");
    assert!(chunk.block.ret.is_some());
}

#[test]
fn parses_return_with_exprs() {
    let chunk = parse_chunk("return 1, 2");
    let ret = chunk.block.ret.unwrap();
    assert_eq!(ret.exprs.len(), 2);
}

#[test]
fn parses_precedence_mul_over_add() {
    let chunk = parse_chunk("return 1 + 2 * 3");
    let ret = chunk.block.ret.unwrap();
    let expr = &ret.exprs[0];
    match &expr.kind {
        ExpKind::Binary { op, left: _, right } => {
            assert_eq!(*op, crate::ast::BinOp::Add);
            match right.kind {
                ExpKind::Binary { op: inner_op, .. } => assert_eq!(inner_op, crate::ast::BinOp::Mul),
                _ => panic!("expected mul on right"),
            }
        }
        _ => panic!("expected binary expr"),
    }
}

#[test]
fn parses_right_assoc_pow() {
    let chunk = parse_chunk("return 2 ^ 3 ^ 4");
    let ret = chunk.block.ret.unwrap();
    let expr = &ret.exprs[0];
    match &expr.kind {
        ExpKind::Binary { op, left: _, right } => {
            assert_eq!(*op, crate::ast::BinOp::Pow);
            match right.kind {
                ExpKind::Binary { op: inner_op, .. } => assert_eq!(inner_op, crate::ast::BinOp::Pow),
                _ => panic!("expected right assoc pow"),
            }
        }
        _ => panic!("expected binary expr"),
    }
}

#[test]
fn parses_table_constructor() {
    let chunk = parse_chunk("return {a=1, [2]=3, 4}");
    let ret = chunk.block.ret.unwrap();
    let expr = &ret.exprs[0];
    match &expr.kind {
        ExpKind::Table(table) => assert_eq!(table.fields.len(), 3),
        _ => panic!("expected table constructor"),
    }
}

#[test]
fn parses_prefix_field_and_index() {
    let chunk = parse_chunk("return a.b[1]");
    let ret = chunk.block.ret.unwrap();
    match &ret.exprs[0].kind {
        ExpKind::Prefix(prefix) => match &prefix.kind {
            crate::ast::PrefixExpKind::Var(_) => {}
            _ => panic!("expected var prefix"),
        },
        _ => panic!("expected prefix exp"),
    }
}

#[test]
fn parses_function_call_args() {
    let chunk = parse_chunk("return f(1, 2)");
    let ret = chunk.block.ret.unwrap();
    match &ret.exprs[0].kind {
        ExpKind::Prefix(prefix) => match &prefix.kind {
            crate::ast::PrefixExpKind::Call(call) => match &call.args.kind {
                ArgsKind::ExpList(exprs) => assert_eq!(exprs.len(), 2),
                _ => panic!("expected arg list"),
            },
            _ => panic!("expected call"),
        },
        _ => panic!("expected prefix exp"),
    }
}

#[test]
fn parses_method_call() {
    let chunk = parse_chunk("return obj:method(1)");
    let ret = chunk.block.ret.unwrap();
    match &ret.exprs[0].kind {
        ExpKind::Prefix(prefix) => match &prefix.kind {
            crate::ast::PrefixExpKind::Call(call) => assert!(call.method.is_some()),
            _ => panic!("expected call"),
        },
        _ => panic!("expected prefix exp"),
    }
}

#[test]
fn errors_on_vararg_outside_function() {
    let tokens = Lexer::new("return ...").lex_all().unwrap();
    let err = Parser::new(tokens).parse_chunk().unwrap_err();
    assert!(err.message.contains("vararg"));
}

#[test]
fn parses_function_expression_with_vararg() {
    let chunk = parse_chunk("return function(a, ...) return ... end");
    let ret = chunk.block.ret.unwrap();
    match &ret.exprs[0].kind {
        ExpKind::FuncDef(func) => assert!(func.is_vararg),
        _ => panic!("expected function expression"),
    }
}

#[test]
fn parses_local_assignment() {
    let chunk = parse_chunk("local x, y = 1, 2");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::LocalAssign { names, exprs } => {
            assert_eq!(names.len(), 2);
            assert_eq!(exprs.len(), 2);
        }
        _ => panic!("expected local assignment"),
    }
}

#[test]
fn parses_assignment_statement() {
    let chunk = parse_chunk("x, y = 3, 4");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::Assign { vars, exprs } => {
            assert_eq!(vars.len(), 2);
            assert_eq!(exprs.len(), 2);
        }
        _ => panic!("expected assignment"),
    }
}

#[test]
fn parses_call_statement() {
    let chunk = parse_chunk("f(1)");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::Call { call } => assert!(matches!(call.args.kind, ArgsKind::ExpList(_))),
        _ => panic!("expected call statement"),
    }
}

#[test]
fn parses_if_else_statement() {
    let chunk = parse_chunk("if a then return 1 else return 2 end");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::If { clauses, else_block } => {
            assert_eq!(clauses.len(), 1);
            assert!(else_block.is_some());
        }
        _ => panic!("expected if statement"),
    }
}

#[test]
fn parses_for_numeric_statement() {
    let chunk = parse_chunk("for i = 1, 10, 2 do break end");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::ForNumeric { step, .. } => assert!(step.is_some()),
        _ => panic!("expected numeric for"),
    }
}

#[test]
fn parses_for_generic_statement() {
    let chunk = parse_chunk("for k, v in pairs(t) do end");
    let stat = &chunk.block.stats[0];
    match &stat.kind {
        StatKind::ForGeneric { names, exprs, .. } => {
            assert_eq!(names.len(), 2);
            assert_eq!(exprs.len(), 1);
        }
        _ => panic!("expected generic for"),
    }
}

#[test]
fn attaches_trailing_comment_to_statement() {
    let chunk = parse_chunk("x = 1 -- trailing");
    assert!(chunk.comments.iter().any(|c| c.text.contains("trailing")));
}

#[test]
fn attaches_trailing_comment_to_expression() {
    let chunk = parse_chunk("return { a = 1 -- trailing\n }");
    assert!(chunk.comments.iter().any(|c| c.text.contains("trailing")));
}

#[test]
fn collects_detached_comments_in_block() {
    let chunk = parse_chunk("-- a\n\n-- b\nreturn 1");
    assert_eq!(chunk.comments.len(), 2);
    assert!(chunk.comments.iter().any(|c| c.text.contains("-- a")));
    assert!(chunk.comments.iter().any(|c| c.text.contains("-- b")));
}

#[test]
fn parses_label_with_leading_comment() {
    let chunk = parse_chunk("-- label\n::lbl::");
    assert!(chunk.comments.iter().any(|c| c.text.contains("label")));
}

#[test]
fn parses_local_attr_with_comment() {
    let chunk = parse_chunk("-- local\nlocal x <const> = 1");
    assert!(chunk.comments.iter().any(|c| c.text.contains("local")));
}

#[test]
fn e2e_parses_full_control_flow_chunk() {
    let src = r#"
        local x = 1
        while x < 10 do
            if x % 2 == 0 then
                x = x + 1
            else
                x = x + 2
            end
        end
        return x
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.block.stats.len(), 2);
    assert!(chunk.block.ret.is_some());
}

#[test]
fn e2e_parses_functions_tables_and_calls() {
    let src = r#"
        local function sum(a, b)
            return a + b
        end
        local t = { a = 1, [2] = 3, 4 }
        t:push(sum(1, 2))
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.block.stats.len(), 3);
    let output = crate::emit::Emitter::emit_chunk(&chunk);
    assert!(output.contains("StatKind::LocalFunction"));
    assert!(output.contains("StatKind::LocalAssign"));
    assert!(output.contains("StatKind::Call"));
}

#[test]
fn e2e_parses_repeat_and_for_loops() {
    let src = r#"
        local i = 0
        repeat
            i = i + 1
        until i >= 3
        for k, v in pairs(t) do
            break
        end
        for j = 1, 5, 2 do
            goto skip
        end
        ::skip::
        return i
    "#;
    let chunk = parse_chunk(src);
    assert!(chunk.block.stats.len() >= 4);
    assert!(chunk.block.ret.is_some());
}

#[test]
fn e2e_preserves_comments_in_ast() {
    let src = r#"
        -- leading
        local x = 1 -- trailing
        -- detached
        
        return x
    "#;
    let chunk = parse_chunk(src);
    assert_eq!(chunk.block.stats.len(), 1);
    assert_eq!(chunk.comments.len(), 3);
}