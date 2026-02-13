use crate::ast::{
    Args, ArgsKind, BinOp, Block, Chunk, Exp, ExpKind, Field, FieldKey, FuncBody, FuncName,
    FunctionCall, LocalAttr, LocalName, Name, PrefixExp, PrefixExpKind, RetStat, Stat, StatKind,
    TableConstructor, UnOp, Var, VarKind,
};
use crate::token::{Comment, CommentKind, Span};

pub struct Emitter {
    out: String,
    indent: usize,
}

impl Emitter {
    pub fn emit_chunk(chunk: &Chunk) -> String {
        let mut emitter = Self {
            out: String::new(),
            indent: 0,
        };
        emitter.write_chunk(chunk);
        emitter.out
    }

    fn write_chunk(&mut self, chunk: &Chunk) {
        self.line("Chunk {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(chunk.span)));
        self.write_comments_field("comments", &chunk.comments);
        self.write_block_field("block", &chunk.block);
        self.indent -= 2;
        self.line("}");
    }

    fn write_block_field(&mut self, label: &str, block: &Block) {
        self.indent_line(&format!("{label}: Block {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(block.span)));
        self.indent_line("stats: [");
        self.indent += 2;
        for stat in &block.stats {
            self.write_stat(stat);
        }
        self.indent -= 2;
        self.line("],");
        self.write_ret_stat_field(&block.ret);
        self.indent -= 2;
        self.line("},");
    }

    fn write_stat(&mut self, stat: &Stat) {
        self.indent_line("Stat {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(stat.span)));
        self.indent_line("kind: ");
        self.write_stat_kind(&stat.kind);
        self.line(",");
        self.indent -= 2;
        self.line("},");
    }

    fn write_stat_kind(&mut self, kind: &StatKind) {
        match kind {
            StatKind::Empty => self.inline("StatKind::Empty"),
            StatKind::Assign { vars, exprs } => {
                self.inline("StatKind::Assign {");
                self.newline();
                self.indent += 2;
                self.write_vars_field("vars", vars);
                self.write_exprs_field("exprs", exprs);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::LocalAssign { names, exprs } => {
                self.inline("StatKind::LocalAssign {");
                self.newline();
                self.indent += 2;
                self.write_local_names_field("names", names);
                self.write_exprs_field("exprs", exprs);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::LocalFunction { name, func } => {
                self.inline("StatKind::LocalFunction {");
                self.newline();
                self.indent += 2;
                self.write_name_field("name", name);
                self.write_func_body_field("func", func);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Function { name, func } => {
                self.inline("StatKind::Function {");
                self.newline();
                self.indent += 2;
                self.write_func_name_field("name", name);
                self.write_func_body_field("func", func);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Do { block } => {
                self.inline("StatKind::Do {");
                self.newline();
                self.indent += 2;
                self.write_block_field("block", block);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::While { cond, block } => {
                self.inline("StatKind::While {");
                self.newline();
                self.indent += 2;
                self.write_exp_field("cond", cond);
                self.write_block_field("block", block);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Repeat { block, cond } => {
                self.inline("StatKind::Repeat {");
                self.newline();
                self.indent += 2;
                self.write_block_field("block", block);
                self.write_exp_field("cond", cond);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::If { clauses, else_block } => {
                self.inline("StatKind::If {");
                self.newline();
                self.indent += 2;
                self.indent_line("clauses: [");
                self.indent += 2;
                for clause in clauses {
                    self.write_if_clause(clause);
                }
                self.indent -= 2;
                self.line("],");
                self.write_optional_block_field("else_block", else_block.as_ref());
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::ForNumeric {
                name,
                start,
                end,
                step,
                block,
            } => {
                self.inline("StatKind::ForNumeric {");
                self.newline();
                self.indent += 2;
                self.write_name_field("name", name);
                self.write_exp_field("start", start);
                self.write_exp_field("end", end);
                self.write_optional_exp_field("step", step.as_ref());
                self.write_block_field("block", block);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::ForGeneric { names, exprs, block } => {
                self.inline("StatKind::ForGeneric {");
                self.newline();
                self.indent += 2;
                self.write_names_field("names", names);
                self.write_exprs_field("exprs", exprs);
                self.write_block_field("block", block);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Break => self.inline("StatKind::Break"),
            StatKind::Goto { label } => {
                self.inline("StatKind::Goto {");
                self.newline();
                self.indent += 2;
                self.write_name_field("label", label);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Label { label } => {
                self.inline("StatKind::Label {");
                self.newline();
                self.indent += 2;
                self.write_name_field("label", label);
                self.indent -= 2;
                self.indent_line("}")
            }
            StatKind::Call { call } => {
                self.inline("StatKind::Call {");
                self.newline();
                self.indent += 2;
                self.write_call_field("call", call);
                self.indent -= 2;
                self.indent_line("}")
            }
        }
    }

    fn write_ret_stat_field(&mut self, ret: &Option<RetStat>) {
        match ret {
            Some(ret) => {
                self.indent_line("ret: RetStat {");
                self.indent += 2;
                self.line(&format!("span: {},", format_span(ret.span)));
                self.write_exprs_field("exprs", &ret.exprs);
                self.indent -= 2;
                self.line("},");
            }
            None => self.line("ret: None,"),
        }
    }

    fn write_if_clause(&mut self, clause: &crate::ast::IfClause) {
        self.indent_line("IfClause {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(clause.span)));
        self.write_exp_field("cond", &clause.cond);
        self.write_block_field("block", &clause.block);
        self.indent -= 2;
        self.line("},");
    }

    fn write_exp(&mut self, exp: &Exp) {
        self.indent_line("Exp {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(exp.span)));
        self.indent_line("kind: ");
        self.write_exp_kind(&exp.kind);
        self.line(",");
        self.indent -= 2;
        self.line("},");
    }

    fn write_exp_kind(&mut self, kind: &ExpKind) {
        match kind {
            ExpKind::Nil => self.inline("ExpKind::Nil"),
            ExpKind::Bool(value) => self.inline(&format!("ExpKind::Bool({value})")),
            ExpKind::Number(text) => self.inline(&format!("ExpKind::Number({})", quoted(text))),
            ExpKind::String(text) => self.inline(&format!("ExpKind::String({})", quoted(text))),
            ExpKind::Vararg => self.inline("ExpKind::Vararg"),
            ExpKind::FuncDef(func) => {
                self.inline("ExpKind::FuncDef {");
                self.newline();
                self.indent += 2;
                self.write_func_body_field("func", func);
                self.indent -= 2;
                self.indent_line("}")
            }
            ExpKind::Table(table) => {
                self.inline("ExpKind::Table {");
                self.newline();
                self.indent += 2;
                self.write_table_field("table", table);
                self.indent -= 2;
                self.indent_line("}")
            }
            ExpKind::Prefix(prefix) => {
                self.inline("ExpKind::Prefix {");
                self.newline();
                self.indent += 2;
                self.write_prefix_field("prefix", prefix);
                self.indent -= 2;
                self.indent_line("}")
            }
            ExpKind::Unary { op, exp } => {
                self.inline("ExpKind::Unary {");
                self.newline();
                self.indent += 2;
                self.write_unary_op_field("op", *op);
                self.write_exp_field("exp", exp);
                self.indent -= 2;
                self.indent_line("}")
            }
            ExpKind::Binary { op, left, right } => {
                self.inline("ExpKind::Binary {");
                self.newline();
                self.indent += 2;
                self.write_binary_op_field("op", *op);
                self.write_exp_field("left", left);
                self.write_exp_field("right", right);
                self.indent -= 2;
                self.indent_line("}")
            }
        }
    }

    fn write_table_field(&mut self, label: &str, table: &TableConstructor) {
        self.indent_line(&format!("{label}: TableConstructor {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(table.span)));
        self.indent_line("fields: [");
        self.indent += 2;
        for field in &table.fields {
            self.write_field(field);
        }
        self.indent -= 2;
        self.line("],");
        self.indent -= 2;
        self.line("},");
    }

    fn write_field(&mut self, field: &Field) {
        self.indent_line("Field {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(field.span)));
        match &field.key {
            Some(key) => {
                self.indent_line("key: ");
                self.write_field_key(key);
                self.line(",");
            }
            None => self.line("key: None,"),
        }
        self.write_exp_field("value", &field.value);
        self.indent -= 2;
        self.line("},");
    }

    fn write_field_key(&mut self, key: &FieldKey) {
        match key {
            FieldKey::Exp(exp) => {
                self.inline("FieldKey::Exp(");
                self.newline();
                self.indent += 2;
                self.write_exp(exp);
                self.indent -= 2;
                self.indent_line(")");
            }
            FieldKey::Name(name) => {
                self.inline("FieldKey::Name(");
                self.newline();
                self.indent += 2;
                self.write_name(name);
                self.indent -= 2;
                self.indent_line(")");
            }
        }
    }

    fn write_func_body_field(&mut self, label: &str, func: &FuncBody) {
        self.indent_line(&format!("{label}: FuncBody {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(func.span)));
        self.write_names_field("params", &func.params);
        self.line(&format!("is_vararg: {},", func.is_vararg));
        self.write_block_field("block", &func.block);
        self.indent -= 2;
        self.line("},");
    }

    fn write_func_name_field(&mut self, label: &str, name: &FuncName) {
        self.indent_line(&format!("{label}: FuncName {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(name.span)));
        self.write_names_field("names", &name.names);
        match &name.method {
            Some(method) => {
                self.indent_line("method: ");
                self.write_name(method);
                self.line(",");
            }
            None => self.line("method: None,"),
        }
        self.indent -= 2;
        self.line("},");
    }

    fn write_prefix_field(&mut self, label: &str, prefix: &PrefixExp) {
        self.indent_line(&format!("{label}: PrefixExp {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(prefix.span)));
        self.indent_line("kind: ");
        match &prefix.kind {
            PrefixExpKind::Var(var) => {
                self.write_var(var);
            }
            PrefixExpKind::Call(call) => {
                self.write_call(call);
            }
            PrefixExpKind::Paren(exp) => {
                self.write_exp(exp);
            }
        }
        self.line(",");
        self.indent -= 2;
        self.line("},");
    }

    fn write_call_field(&mut self, label: &str, call: &FunctionCall) {
        self.indent_line(&format!("{label}: FunctionCall {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(call.span)));
        self.write_prefix_field("prefix", &call.prefix);
        match &call.method {
            Some(method) => {
                self.indent_line("method: ");
                self.write_name(method);
                self.line(",");
            }
            None => self.line("method: None,"),
        }
        self.write_args_field("args", &call.args);
        self.indent -= 2;
        self.line("},");
    }

    fn write_args_field(&mut self, label: &str, args: &Args) {
        self.indent_line(&format!("{label}: Args {{"));
        self.indent += 2;
        self.line(&format!("span: {},", format_span(args.span)));
        self.indent_line("kind: ");
        match &args.kind {
            ArgsKind::ExpList(exprs) => {
                self.inline("ArgsKind::ExpList[");
                if exprs.is_empty() {
                    self.inline("]");
                } else {
                    self.newline();
                    self.indent += 2;
                    for exp in exprs {
                        self.write_exp(exp);
                    }
                    self.indent -= 2;
                    self.indent_line("]");
                }
            }
            ArgsKind::Table(table) => {
                self.inline("ArgsKind::Table {");
                self.newline();
                self.indent += 2;
                self.write_table_field("table", table);
                self.indent -= 2;
                self.indent_line("}")
            }
            ArgsKind::String(text) => {
                self.inline(&format!("ArgsKind::String({})", quoted(text)));
            }
        }
        self.line(",");
        self.indent -= 2;
        self.line("},");
    }

    fn write_var(&mut self, var: &Var) {
        self.indent_line("Var {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(var.span)));
        self.indent_line("kind: ");
        match &var.kind {
            VarKind::Name(name) => self.write_name(name),
            VarKind::Index { prefix, index } => {
                self.inline("VarKind::Index {");
                self.newline();
                self.indent += 2;
                self.write_prefix_field("prefix", prefix);
                self.write_exp_field("index", index);
                self.indent -= 2;
                self.indent_line("}")
            }
            VarKind::Field { prefix, name } => {
                self.inline("VarKind::Field {");
                self.newline();
                self.indent += 2;
                self.write_prefix_field("prefix", prefix);
                self.write_name_field("name", name);
                self.indent -= 2;
                self.indent_line("}")
            }
        }
        self.line(",");
        self.indent -= 2;
        self.line("},");
    }

    fn write_name(&mut self, name: &Name) {
        self.indent_line("Name {");
        self.indent += 2;
        self.line(&format!("value: {},", quoted(&name.value)));
        self.line(&format!("span: {},", format_span(name.span)));
        self.indent -= 2;
        self.line("}");
    }

    fn write_local_names_field(&mut self, label: &str, names: &[LocalName]) {
        self.indent_line(&format!("{label}: ["));
        self.indent += 2;
        for name in names {
            self.write_local_name(name);
        }
        self.indent -= 2;
        self.line("],");
    }

    fn write_local_name(&mut self, local: &LocalName) {
        self.indent_line("LocalName {");
        self.indent += 2;
        self.write_name_field("name", &local.name);
        match local.attr {
            Some(attr) => self.line(&format!("attr: {},", format_local_attr(attr))),
            None => self.line("attr: None,"),
        }
        self.indent -= 2;
        self.line("},");
    }

    fn write_vars_field(&mut self, label: &str, vars: &[Var]) {
        self.indent_line(&format!("{label}: ["));
        self.indent += 2;
        for var in vars {
            self.write_var(var);
        }
        self.indent -= 2;
        self.line("],");
    }

    fn write_names_field(&mut self, label: &str, names: &[Name]) {
        self.indent_line(&format!("{label}: ["));
        self.indent += 2;
        for name in names {
            self.write_name(name);
            self.line(",");
        }
        self.indent -= 2;
        self.line("],");
    }

    fn write_name_field(&mut self, label: &str, name: &Name) {
        self.indent_line(&format!("{label}: "));
        self.write_name(name);
        self.line(",");
    }

    fn write_exp_field(&mut self, label: &str, exp: &Exp) {
        self.indent_line(&format!("{label}: "));
        self.write_exp(exp);
        self.line(",");
    }

    fn write_exprs_field(&mut self, label: &str, exprs: &[Exp]) {
        self.indent_line(&format!("{label}: ["));
        self.indent += 2;
        for exp in exprs {
            self.write_exp(exp);
        }
        self.indent -= 2;
        self.line("],");
    }

    fn write_optional_exp_field(&mut self, label: &str, exp: Option<&Exp>) {
        match exp {
            Some(exp) => self.write_exp_field(label, exp),
            None => self.line(&format!("{label}: None,")),
        }
    }

    fn write_optional_block_field(&mut self, label: &str, block: Option<&Block>) {
        match block {
            Some(block) => self.write_block_field(label, block),
            None => self.line(&format!("{label}: None,")),
        }
    }

    fn write_call(&mut self, call: &FunctionCall) {
        self.indent_line("FunctionCall {");
        self.indent += 2;
        self.line(&format!("span: {},", format_span(call.span)));
        self.write_prefix_field("prefix", &call.prefix);
        match &call.method {
            Some(method) => {
                self.indent_line("method: ");
                self.write_name(method);
                self.line(",");
            }
            None => self.line("method: None,"),
        }
        self.write_args_field("args", &call.args);
        self.indent -= 2;
        self.line("}");
    }

    fn write_comments_field(&mut self, label: &str, comments: &[Comment]) {
        if comments.is_empty() {
            return;
        }
        self.indent_line(&format!("{label}: ["));
        self.indent += 2;
        for comment in comments {
            self.write_comment(comment);
        }
        self.indent -= 2;
        self.line("],");
    }

    fn write_comment(&mut self, comment: &Comment) {
        self.indent_line("Comment {");
        self.indent += 2;
        self.line(&format!("kind: {},", format_comment_kind(comment.kind)));
        self.line(&format!("text: {},", quoted(&comment.text)));
        self.line(&format!("span: {},", format_span(comment.span)));
        self.indent -= 2;
        self.line("},");
    }

    fn write_unary_op_field(&mut self, label: &str, op: UnOp) {
        self.line(&format!("{label}: {},", format_un_op(op)));
    }

    fn write_binary_op_field(&mut self, label: &str, op: BinOp) {
        self.line(&format!("{label}: {},", format_bin_op(op)));
    }

    fn line(&mut self, text: &str) {
        self.indent_line(text);
    }

    fn indent_line(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push(' ');
        }
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn inline(&mut self, text: &str) {
        for _ in 0..self.indent {
            self.out.push(' ');
        }
        self.out.push_str(text);
    }

    fn newline(&mut self) {
        self.out.push('\n');
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
    use crate::ast::{Block, Chunk, Exp, ExpKind, LocalName, Name, Stat, StatKind, TableConstructor};
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
        let mut emitter = Emitter {
            out: String::new(),
            indent: 0,
        };
        emitter.write_comment(&comment);
        assert!(emitter.out.contains("\\\"hi\\\""));
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
        let mut emitter = Emitter {
            out: String::new(),
            indent: 0,
        };
        emitter.write_exp(&exp);
        assert!(emitter.out.contains("TableConstructor"));
    }
}
