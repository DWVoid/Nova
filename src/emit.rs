use crate::ast::{
    Args, ArgsKind, BinOp, Block, Chunk, Exp, ExpKind, Field, FieldKey, FuncBody, FuncName,
    FunctionCall, LocalAttr, LocalName, Name, PrefixExp, PrefixExpKind, RetStat, Stat, StatKind,
    TableConstructor, UnOp, Var, VarKind,
};
use crate::token::{Comment, CommentKind, Span};

struct Formatter {
    out: String,
    indent: usize,
    newline: bool,
}

impl Formatter {
    fn new() -> Self {
        Self {
            out: String::new(),
            indent: 0,
            newline: true,
        }
    }

    fn write(&mut self, text: &str) {
        if self.newline {
            for _ in 0..self.indent {
                self.out.push(' ');
            }
            self.newline = false;
        }
        self.out.push_str(text);
    }

    fn write_line(&mut self, text: &str) {
        if self.newline {
            for _ in 0..self.indent {
                self.out.push(' ');
            }
        }
        self.out.push_str(text);
        self.out.push('\n');
        self.newline = true;
    }

    fn indent(&mut self) {
        self.indent += 2;
    }

    fn unindent(&mut self) {
        self.indent -= 2;
    }
}

struct ListFormatter<'a> {
    fmt: &'a mut Formatter,
    first: bool,
}

impl<'a> ListFormatter<'a> {
    fn new(fmt: &'a mut Formatter) -> Self {
        let res = Self { fmt, first: true };
        res.fmt.write("[");
        res
    }

    fn new_named(fmt: &'a mut Formatter, name: &str) -> Self {
        let res = Self { fmt, first: true };
        res.fmt.write(name);
        res.fmt.write(" [");
        res
    }

    fn next(&mut self, field: impl Fn(&mut Formatter)) {
        if self.first {
            self.fmt.write_line("");
            self.fmt.indent();
            self.first = false
        } else {
            self.fmt.write_line(",")
        }
        field(self.fmt);
    }
}

impl Drop for ListFormatter<'_> {
    fn drop(&mut self) {
        if !self.first {
            self.fmt.write_line("");
            self.fmt.unindent();
        }
        self.fmt.write("]")
    }
}

struct TableFormatter<'a> {
    fmt: &'a mut Formatter,
    first: bool,
}

impl<'a> TableFormatter<'a> {
    fn new(fmt: &'a mut Formatter, name: &str) -> Self {
        let res = Self { fmt, first: true };
        res.fmt.write(name);
        res.fmt.write(" {");
        res
    }

    fn next(&mut self, name: &str, field: impl Fn(&mut Formatter)) {
        if self.first {
            self.fmt.write_line("");
            self.fmt.indent();
            self.first = false
        } else {
            self.fmt.write_line(",")
        }
        self.fmt.write(name);
        self.fmt.write(": ");
        field(self.fmt);
    }
}

impl Drop for TableFormatter<'_> {
    fn drop(&mut self) {
        if !self.first {
            self.fmt.write_line("");
            self.fmt.unindent();
        }
        self.fmt.write("}")
    }
}
pub struct Emitter {}

impl Emitter {
    pub fn emit_chunk(chunk: &Chunk) -> String {
        let mut fmt = Formatter::new();
        Self::write_chunk(&mut fmt, chunk);
        fmt.out
    }

    fn write_chunk(f: &mut Formatter, chunk: &Chunk) {
        let mut ftb = TableFormatter::new(f, "Chunk");
        ftb.next("span", |f| f.write(&format_span(chunk.span)));
        ftb.next("comments", |f| Self::write_comments(f, &chunk.comments));
        ftb.next("block", |f| Self::write_block(f, &chunk.block));
    }

    fn write_comments(f: &mut Formatter, comments: &[Comment]) {
        let mut ftb = ListFormatter::new(f);
        for comment in comments {
            ftb.next(|f| Self::write_comment(f, comment));
        }
    }

    fn write_comment(f: &mut Formatter, comment: &Comment) {
        let mut ftb = TableFormatter::new(f, "Comment");
        ftb.next("kind", |f| f.write(format_comment_kind(comment.kind)));
        ftb.next("text", |f| f.write(&quoted(&comment.text)));
        ftb.next("span", |f| f.write(&*format_span(comment.span)));
    }

    fn write_block(f: &mut Formatter, block: &Block) {
        let mut ftb = TableFormatter::new(f, "Block");
        ftb.next("span", |f| f.write(&format_span(block.span)));
        ftb.next("stats", |f| {
            let mut ftb = ListFormatter::new(f);
            for stat in &block.stats {
                ftb.next(|f| Self::write_stat(f, stat));
            }
        });
        ftb.next("ret", |f| Self::write_ret_stat(f, &block.ret));
    }

    fn write_stat(f: &mut Formatter, stat: &Stat) {
        let mut ftb = TableFormatter::new(f, "Stat");
        ftb.next("span", |f| f.write(&format_span(stat.span)));
        ftb.next("kind", |f| Self::write_stat_kind(f, &stat.kind));
    }

    fn write_stat_kind(f: &mut Formatter, kind: &StatKind) {
        match kind {
            StatKind::Empty => f.write("StatKind::Empty"),
            StatKind::Assign { vars, exprs } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Assign");
                ftb.next("vars", |f| Self::write_vars(f, vars));
                ftb.next("exprs", |f| Self::write_exps(f, exprs));
            }
            StatKind::LocalAssign { names, exprs } => {
                let mut ftb = TableFormatter::new(f, "StatKind::LocalAssign");
                ftb.next("names", |f| Self::write_local_names(f, names));
                ftb.next("exprs", |f| Self::write_exps(f, exprs));
            }
            StatKind::LocalFunction { name, func } => {
                let mut ftb = TableFormatter::new(f, "StatKind::LocalFunction");
                ftb.next("name", |f| Self::write_name(f, name));
                ftb.next("func", |f| Self::write_func_body(f, func));
            }
            StatKind::Function { name, func } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Function");
                ftb.next("name", |f| Self::write_func_name(f, name));
                ftb.next("func", |f| Self::write_func_body(f, func));
            }
            StatKind::Do { block } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Do");
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::While { cond, block } => {
                let mut ftb = TableFormatter::new(f, "StatKind::While");
                ftb.next("cond", |f| Self::write_exp(f, cond));
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::Repeat { block, cond } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Repeat");
                ftb.next("block", |f| Self::write_block(f, block));
                ftb.next("cond", |f| Self::write_exp(f, cond));
            }
            StatKind::If {
                clauses,
                else_block,
            } => {
                let mut ftb = TableFormatter::new(f, "StatKind::If");
                ftb.next("clauses", |f| {
                    let mut ftb = ListFormatter::new(f);
                    for clause in clauses {
                        ftb.next(|f| Self::write_if_clause(f, clause));
                    }
                });
                ftb.next("else_block", |f| {
                    Self::write_optional_block(f, else_block.as_ref())
                });
            }
            StatKind::ForNumeric {
                name,
                start,
                end,
                step,
                block,
            } => {
                let mut ftb = TableFormatter::new(f, "StatKind::ForNumeric");
                ftb.next("name", |f| Self::write_name(f, name));
                ftb.next("start", |f| Self::write_exp(f, start));
                ftb.next("end", |f| Self::write_exp(f, end));
                ftb.next("step", |f| Self::write_optional_exp(f, step.as_ref()));
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::ForGeneric {
                names,
                exprs,
                block,
            } => {
                let mut ftb = TableFormatter::new(f, "StatKind::ForGeneric");
                ftb.next("names", |f| Self::write_names(f, names));
                ftb.next("exprs", |f| Self::write_exps(f, exprs));
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::Break => f.write("StatKind::Break"),
            StatKind::Goto { label } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Goto");
                ftb.next("label", |f| Self::write_name(f, label));
            }
            StatKind::Label { label } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Label");
                ftb.next("label", |f| Self::write_name(f, label));
            }
            StatKind::Call { call } => {
                let mut ftb = TableFormatter::new(f, "StatKind::Call");
                ftb.next("call", |f| Self::write_function_call(f, call));
            }
        }
    }

    fn write_vars(f: &mut Formatter, vars: &[Var]) {
        let mut ftb = ListFormatter::new(f);
        for var in vars {
            ftb.next(|f| Self::write_var(f, var));
        }
    }

    fn write_var(f: &mut Formatter, var: &Var) {
        let mut ftb = TableFormatter::new(f, "Var");
        ftb.next("span", |f| f.write(&format_span(var.span)));
        ftb.next("kind", |f| match &var.kind {
            VarKind::Name(name) => Self::write_name(f, name),
            VarKind::Index { prefix, index } => {
                let mut ftb = TableFormatter::new(f, "VarKind::Index");
                ftb.next("prefix", |f| Self::write_prefix(f, prefix));
                ftb.next("index", |f| Self::write_exp(f, index));
            }
            VarKind::Field { prefix, name } => {
                let mut ftb = TableFormatter::new(f, "VarKind::Field");
                ftb.next("prefix", |f| Self::write_prefix(f, prefix));
                ftb.next("name", |f| Self::write_name(f, name));
            }
        });
    }

    fn write_names(f: &mut Formatter, names: &[Name]) {
        let mut ftb = ListFormatter::new(f);
        for name in names {
            ftb.next(|f| Self::write_name(f, name));
        }
    }

    fn write_name(f: &mut Formatter, name: &Name) {
        let mut ftb = TableFormatter::new(f, "Name");
        ftb.next("span", |f| f.write(&format_span(name.span)));
        ftb.next("value", |f| f.write(&quoted(&name.value)));
    }

    fn write_prefix(f: &mut Formatter, prefix: &PrefixExp) {
        let mut ftb = TableFormatter::new(f, "PrefixExp");
        ftb.next("span", |f| f.write(&format_span(prefix.span)));
        ftb.next("kind", |f| match &prefix.kind {
            PrefixExpKind::Var(var) => Self::write_var(f, var),
            PrefixExpKind::Call(call) => Self::write_call(f, call),
            PrefixExpKind::Paren(exp) => Self::write_exp(f, exp),
        });
    }

    fn write_call(f: &mut Formatter, call: &FunctionCall) {
        let mut ftb = TableFormatter::new(f, "FunctionCall");
        ftb.next("span", |f| f.write(&format_span(call.span)));
        ftb.next("prefix", |f| Self::write_prefix(f, &call.prefix));
        ftb.next("kind", |f| match &call.method {
            Some(method) => Self::write_name(f, method),
            None => f.write("None"),
        });
        ftb.next("args", |f| Self::write_args(f, &call.args));
    }

    fn write_args(f: &mut Formatter, args: &Args) {
        let mut ftb = TableFormatter::new(f, "Args");
        ftb.next("span", |f| f.write(&format_span(args.span)));
        ftb.next("kind", |f| match &args.kind {
            ArgsKind::ExpList(exprs) => {
                let mut ftb = ListFormatter::new_named(f, "ArgsKind::ExpList");
                for exp in exprs {
                    ftb.next(|f| Self::write_exp(f, exp));
                }
            }
            ArgsKind::Table(table) => {
                let mut ftb = TableFormatter::new(f, "ArgsKind::Table");
                ftb.next("table", |f| Self::write_table(f, table));
            }
            ArgsKind::String(text) => {
                f.write(&format!("ArgsKind::String({})", quoted(text)));
            }
        });
    }

    fn write_exps(f: &mut Formatter, exps: &[Exp]) {
        let mut ftb = ListFormatter::new(f);
        for exp in exps {
            ftb.next(|f| Self::write_exp(f, exp));
        }
    }

    fn write_exp(f: &mut Formatter, exp: &Exp) {
        let mut ftb = TableFormatter::new(f, "Exp");
        ftb.next("span", |f| f.write(&format_span(exp.span)));
        ftb.next("kind", |f| match &exp.kind {
            ExpKind::Nil => f.write("ExpKind::Nil"),
            ExpKind::Bool(value) => f.write(&format!("ExpKind::Bool({value})")),
            ExpKind::Number(text) => f.write(&format!("ExpKind::Number({})", quoted(text))),
            ExpKind::String(text) => f.write(&format!("ExpKind::String({})", quoted(text))),
            ExpKind::Vararg => f.write("ExpKind::Vararg"),
            ExpKind::FuncDef(func) => {
                let mut ftb = TableFormatter::new(f, "ExpKind::FuncDef");
                ftb.next("func", |f| Self::write_func_body(f, func));
            }
            ExpKind::Table(table) => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Table");
                ftb.next("table", |f| Self::write_table(f, table));
            }
            ExpKind::Prefix(prefix) => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Prefix");
                ftb.next("prefix", |f| Self::write_prefix(f, prefix));
            }
            ExpKind::Unary { op, exp } => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Unary");
                ftb.next("op", |f| f.write(format_un_op(*op)));
                ftb.next("exp", |f| Self::write_exp(f, exp));
            }
            ExpKind::Binary { op, left, right } => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Binary");
                ftb.next("op", |f| f.write(format_bin_op(*op)));
                ftb.next("left", |f| Self::write_exp(f, left));
                ftb.next("right", |f| Self::write_exp(f, right));
            }
        });
    }

    fn write_func_name(f: &mut Formatter, name: &FuncName) {
        let mut ftb = TableFormatter::new(f, "FuncName");
        ftb.next("span", |f| f.write(&format_span(name.span)));
        ftb.next("names", |f| Self::write_names(f, &name.names));
        ftb.next("method", |f| match &name.method {
            Some(method) => Self::write_name(f, method),
            None => f.write("None"),
        });
    }

    fn write_func_body(f: &mut Formatter, func: &FuncBody) {
        let mut ftb = TableFormatter::new(f, "FuncBody");
        ftb.next("span", |f| f.write(&format_span(func.span)));
        ftb.next("params", |f| Self::write_names(f, &func.params));
        ftb.next("is_vararg", |f| f.write(&format!("{}", func.is_vararg)));
        ftb.next("block", |f| Self::write_block(f, &func.block));
    }

    fn write_table(f: &mut Formatter, table: &TableConstructor) {
        let mut ftb = TableFormatter::new(f, "TableConstructor");
        ftb.next("span", |f| f.write(&format_span(table.span)));
        ftb.next("fields", |f| {
            let mut ftb = ListFormatter::new(f);
            for field in &table.fields {
                ftb.next(|f| Self::write_field(f, field));
            }
        });
    }

    fn write_field(f: &mut Formatter, field: &Field) {
        let mut ftb = TableFormatter::new(f, "Field");
        ftb.next("span", |f| f.write(&format_span(field.span)));
        ftb.next("key", |f| match &field.key {
            Some(key) => match key {
                FieldKey::Exp(exp) => {
                    f.write_line("FieldKey::Exp(");
                    f.indent();
                    Self::write_exp(f, exp);
                    f.unindent();
                    f.write(")");
                }
                FieldKey::Name(name) => {
                    f.write_line("FieldKey::Name(");
                    f.indent();
                    Self::write_name(f, name);
                    f.unindent();
                    f.write(")");
                }
            },
            None => f.write("None"),
        });
        ftb.next("value", |f| Self::write_exp(f, &field.value));
    }

    fn write_local_names(f: &mut Formatter, names: &[LocalName]) {
        let mut ftb = ListFormatter::new(f);
        for name in names {
            ftb.next(|f| Self::write_local_name(f, name));
        }
    }

    fn write_local_name(f: &mut Formatter, local: &LocalName) {
        let mut ftb = TableFormatter::new(f, "LocalName");
        ftb.next("name", |f| Self::write_name(f, &local.name));
        ftb.next("attr", |f| match local.attr {
            Some(attr) => f.write(format_local_attr(attr)),
            None => f.write("None"),
        });
    }

    fn write_if_clause(f: &mut Formatter, clause: &crate::ast::IfClause) {
        let mut ftb = TableFormatter::new(f, "IfClause");
        ftb.next("span", |f| f.write(&format_span(clause.span)));
        ftb.next("cond", |f| Self::write_exp(f, &clause.cond));
        ftb.next("block", |f| Self::write_block(f, &clause.block));
    }

    fn write_optional_block(f: &mut Formatter, block: Option<&Block>) {
        match block {
            Some(block) => Self::write_block(f, block),
            None => f.write("None"),
        }
    }

    fn write_optional_exp(f: &mut Formatter, exp: Option<&Exp>) {
        match exp {
            Some(exp) => Self::write_exp(f, exp),
            None => f.write("None"),
        }
    }

    fn write_function_call(f: &mut Formatter, call: &FunctionCall) {
        let mut ftb = TableFormatter::new(f, "FunctionCall");
        ftb.next("span", |f| f.write(&format_span(call.span)));
        ftb.next("prefix", |f| Self::write_prefix(f, &call.prefix));
        ftb.next("method", |f| match &call.method {
            Some(method) => Self::write_name(f, method),
            None => f.write("None"),
        });
        ftb.next("args", |f| Self::write_args(f, &call.args));
    }

    fn write_ret_stat(fmt: &mut Formatter, ret: &Option<RetStat>) {
        match ret {
            Some(ret) => {
                let mut ftb = TableFormatter::new(fmt, "StatKind::RetStat");
                ftb.next("span", |f| f.write(&format_span(ret.span)));
                ftb.next("exprs", |f| Self::write_exps(f, &ret.exprs));
            }
            None => fmt.write("None"),
        }
    }
}

fn quoted(text: &str) -> String {
    let mut out = String::new();
    out.push('"');
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn format_span(span: Span) -> String {
    format!("{}..{}", span.start.grapheme, span.end.grapheme)
}

fn format_comment_kind(kind: CommentKind) -> &'static str {
    match kind {
        CommentKind::Line => "Line",
        CommentKind::Block => "Block",
    }
}

fn format_local_attr(attr: LocalAttr) -> &'static str {
    match attr {
        LocalAttr::Const => "Const",
        LocalAttr::Close => "Close",
    }
}

fn format_un_op(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "UnOp::Neg",
        UnOp::Not => "UnOp::Not",
        UnOp::Len => "UnOp::Len",
        UnOp::BitNot => "UnOp::BitNot",
    }
}

fn format_bin_op(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "BinOp::Or",
        BinOp::And => "BinOp::And",
        BinOp::Eq => "BinOp::Eq",
        BinOp::NotEq => "BinOp::NotEq",
        BinOp::Less => "BinOp::Less",
        BinOp::LessEq => "BinOp::LessEq",
        BinOp::Greater => "BinOp::Greater",
        BinOp::GreaterEq => "BinOp::GreaterEq",
        BinOp::BitOr => "BinOp::BitOr",
        BinOp::BitXor => "BinOp::BitXor",
        BinOp::BitAnd => "BinOp::BitAnd",
        BinOp::ShiftLeft => "BinOp::ShiftLeft",
        BinOp::ShiftRight => "BinOp::ShiftRight",
        BinOp::Concat => "BinOp::Concat",
        BinOp::Add => "BinOp::Add",
        BinOp::Sub => "BinOp::Sub",
        BinOp::Mul => "BinOp::Mul",
        BinOp::Div => "BinOp::Div",
        BinOp::FloorDiv => "BinOp::FloorDiv",
        BinOp::Mod => "BinOp::Mod",
        BinOp::Pow => "BinOp::Pow",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        Block, Chunk, Exp, ExpKind, LocalName, Name, Stat, StatKind, TableConstructor,
    };
    use crate::token::{Comment, CommentKind, Position, Span};

    fn span() -> Span {
        Span::new(Position::start(), Position::start())
    }

    #[test]
    fn emits_simple_chunk() {
        let name = Name {
            value: "x".to_string(),
            span: span(),
        };
        let exp = Exp {
            span: span(),
            kind: ExpKind::Number("1".to_string()),
        };
        let stat = Stat {
            span: span(),
            kind: StatKind::LocalAssign {
                names: vec![LocalName { name, attr: None }],
                exprs: vec![exp],
            },
        };
        let block = Block {
            span: span(),
            stats: vec![stat],
            ret: None,
        };
        let chunk = Chunk {
            span: span(),
            comments: vec![Comment {
                kind: CommentKind::Line,
                text: "-- hello".to_string(),
                span: span(),
            }],
            block,
        };
        let text = Emitter::emit_chunk(&chunk);
        assert!(text.contains("Chunk"));
        assert!(text.contains("LocalAssign"));
    }

    #[test]
    fn quotes_comment_text() {
        let comment = Comment {
            kind: CommentKind::Line,
            text: "-- \"hi\"".to_string(),
            span: span(),
        };
        let mut f = Formatter::new();
        Emitter::write_comment(&mut f, &comment);
        assert!(f.out.contains("\\\"hi\\\""));
    }

    #[test]
    fn emits_table_constructor() {
        let table = TableConstructor {
            span: span(),
            fields: vec![],
        };
        let exp = Exp {
            span: span(),
            kind: ExpKind::Table(table),
        };
        let mut f = Formatter::new();
        Emitter::write_exp(&mut f, &exp);
        assert!(f.out.contains("TableConstructor"));
    }
}
