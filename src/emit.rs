use crate::ast::{
    Args, ArgsKind, BinOp, Block, Chunk, Decorator, DefExpr, Definition, EnumDef, EnumMember, Exp,
    ExpKind, Field, FieldKey, IfClause, Implementation, Initializer, LambdaExpr, Name, NamespaceDecl,
    Param, PrefixExp, PrefixExpKind, RetStat, Stat, StatKind, StructDef, TraitDef, TraitSig, TypeName,
    TypeSpec, UnOp, UseDecl, UseItem, UseTail, Var, VarDeclKind, VarKind, VariantDef,
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

    fn write_comments(f: &mut Formatter, comments: &[Comment]) {
        let mut list = ListFormatter::new(f);
        for comment in comments {
            list.next(|f| Self::write_comment(f, comment));
        }
    }

    fn write_comment(f: &mut Formatter, comment: &Comment) {
        let mut ftb = TableFormatter::new(f, "Comment");
        ftb.next("kind", |f| f.write(format_comment_kind(comment.kind)));
        ftb.next("text", |f| f.write(&quoted(&comment.text)));
        ftb.next("span", |f| f.write(&format_span(comment.span)));
    }

    fn write_name(f: &mut Formatter, name: &Name) {
        let mut ftb = TableFormatter::new(f, "Name");
        ftb.next("value", |f| f.write(&quoted(&name.value)));
        ftb.next("span", |f| f.write(&format_span(name.span)));
    }

    fn write_vars(f: &mut Formatter, vars: &[Var]) {
        let mut list = ListFormatter::new(f);
        for var in vars {
            list.next(|f| Self::write_var(f, var));
        }
    }

    fn write_exps(f: &mut Formatter, exprs: &[Exp]) {
        let mut list = ListFormatter::new(f);
        for exp in exprs {
            list.next(|f| Self::write_exp(f, exp));
        }
    }

    fn write_block(f: &mut Formatter, block: &Block) {
        let mut ftb = TableFormatter::new(f, "Block");
        ftb.next("span", |f| f.write(&format_span(block.span)));
        ftb.next("stats", |f| {
            let mut list = ListFormatter::new(f);
            for stat in &block.stats {
                list.next(|f| Self::write_stat(f, stat));
            }
        });
        ftb.next("ret", |f| Self::write_ret_stat(f, &block.ret));
    }

    fn write_chunk(f: &mut Formatter, chunk: &Chunk) {
        let mut ftb = TableFormatter::new(f, "Chunk");
        ftb.next("span", |f| f.write(&format_span(chunk.span)));
        ftb.next("comments", |f| Self::write_comments(f, &chunk.comments));
        ftb.next("uses", |f| {
            let mut list = ListFormatter::new(f);
            for use_decl in &chunk.uses {
                list.next(|f| Self::write_use_decl(f, use_decl));
            }
        });
        ftb.next("namespace", |f| Self::write_namespace_decl(f, &chunk.namespace));
        ftb.next("items", |f| {
            let mut list = ListFormatter::new(f);
            for item in &chunk.items {
                list.next(|f| Self::write_top_item(f, item));
            }
        });
    }

    fn write_use_decl(f: &mut Formatter, use_decl: &UseDecl) {
        let mut ftb = TableFormatter::new(f, "UseDecl");
        ftb.next("span", |f| f.write(&format_span(use_decl.span)));
        ftb.next("path", |f| Self::write_name_list(f, &use_decl.path));
        ftb.next("tail", |f| {
            if let Some(tail) = &use_decl.tail {
                match tail {
                    UseTail::Selector(items) => {
                        let mut list = ListFormatter::new_named(f, "UseTail::Selector");
                        for item in items {
                            list.next(|f| Self::write_use_item(f, item));
                        }
                    }
                    UseTail::Alias(alias) => {
                        let mut ftb = TableFormatter::new(f, "UseTail::Alias");
                        ftb.next("name", |f| Self::write_name(f, alias));
                    }
                }
            } else {
                f.write("None")
            }
        });
    }

    fn write_use_item(f: &mut Formatter, item: &UseItem) {
        let mut ftb = TableFormatter::new(f, "UseItem");
        ftb.next("name", |f| Self::write_name(f, &item.name));
        ftb.next("alias", |f| Self::write_optional_name(f, &item.alias));
    }

    fn write_namespace_decl(f: &mut Formatter, namespace: &NamespaceDecl) {
        let mut ftb = TableFormatter::new(f, "NamespaceDecl");
        ftb.next("span", |f| f.write(&format_span(namespace.span)));
        ftb.next("path", |f| Self::write_name_list(f, &namespace.path));
    }

    fn write_top_item(f: &mut Formatter, item: &crate::ast::TopItem) {
        match item {
            crate::ast::TopItem::Definition(def) => Self::write_definition(f, def),
            crate::ast::TopItem::Implementation(imp) => Self::write_implementation(f, imp),
        }
    }

    fn write_definition(f: &mut Formatter, def: &Definition) {
        let mut ftb = TableFormatter::new(f, "Definition");
        ftb.next("span", |f| f.write(&format_span(def.span)));
        ftb.next("decorators", |f| {
            let mut list = ListFormatter::new(f);
            for deco in &def.decorators {
                list.next(|f| Self::write_decorator(f, deco));
            }
        });
        ftb.next("visibility", |f| Self::write_visibility(f, &def.visibility));
        ftb.next("name", |f| Self::write_name(f, &def.name));
        ftb.next("type_spec", |f| Self::write_type_spec_opt(f, &def.type_spec));
        ftb.next("expr", |f| Self::write_def_expr(f, &def.expr));
    }

    fn write_def_expr(f: &mut Formatter, expr: &DefExpr) {
        match expr {
            DefExpr::Struct(def) => Self::write_struct_def(f, def),
            DefExpr::Enum(def) => Self::write_enum_def(f, def),
            DefExpr::Variant(def) => Self::write_variant_def(f, def),
            DefExpr::Trait(def) => Self::write_trait_def(f, def),
            DefExpr::Exp(exp) => Self::write_exp(f, exp),
        }
    }

    fn write_struct_def(f: &mut Formatter, def: &StructDef) {
        let mut ftb = TableFormatter::new(f, "StructDef");
        ftb.next("span", |f| f.write(&format_span(def.span)));
        ftb.next("fields", |f| {
            let mut list = ListFormatter::new(f);
            for field in &def.fields {
                list.next(|f| Self::write_field_decl(f, field));
            }
        });
    }

    fn write_field_decl(f: &mut Formatter, field: &crate::ast::FieldDecl) {
        let mut ftb = TableFormatter::new(f, "FieldDecl");
        ftb.next("name", |f| Self::write_name(f, &field.name));
        ftb.next("type_spec", |f| Self::write_type_spec(f, &field.type_spec));
    }

    fn write_enum_def(f: &mut Formatter, def: &EnumDef) {
        let mut ftb = TableFormatter::new(f, "EnumDef");
        ftb.next("span", |f| f.write(&format_span(def.span)));
        ftb.next("type_spec", |f| Self::write_type_spec(f, &def.type_spec));
        ftb.next("members", |f| {
            let mut list = ListFormatter::new(f);
            for member in &def.members {
                list.next(|f| Self::write_enum_member(f, member));
            }
        });
    }

    fn write_enum_member(f: &mut Formatter, member: &EnumMember) {
        let mut ftb = TableFormatter::new(f, "EnumMember");
        ftb.next("name", |f| Self::write_name(f, &member.name));
        ftb.next("value", |f| Self::write_exp(f, &member.value));
    }

    fn write_variant_def(f: &mut Formatter, def: &VariantDef) {
        let mut ftb = TableFormatter::new(f, "VariantDef");
        ftb.next("span", |f| f.write(&format_span(def.span)));
        ftb.next("members", |f| {
            let mut list = ListFormatter::new(f);
            for member in &def.members {
                list.next(|f| Self::write_variant_member(f, member));
            }
        });
    }

    fn write_variant_member(f: &mut Formatter, member: &crate::ast::VariantMember) {
        let mut ftb = TableFormatter::new(f, "VariantMember");
        ftb.next("name", |f| Self::write_name(f, &member.name));
        ftb.next("type_spec", |f| Self::write_type_spec(f, &member.type_spec));
    }

    fn write_trait_def(f: &mut Formatter, def: &TraitDef) {
        let mut ftb = TableFormatter::new(f, "TraitDef");
        ftb.next("span", |f| f.write(&format_span(def.span)));
        ftb.next("sigs", |f| {
            let mut list = ListFormatter::new(f);
            for sig in &def.sigs {
                list.next(|f| Self::write_trait_sig(f, sig));
            }
        });
    }

    fn write_trait_sig(f: &mut Formatter, sig: &TraitSig) {
        let mut ftb = TableFormatter::new(f, "TraitSig");
        ftb.next("name", |f| Self::write_name(f, &sig.name));
        ftb.next("params", |f| Self::write_params(f, &sig.params));
        ftb.next("return_type", |f| Self::write_type_spec(f, &sig.return_type));
    }

    fn write_implementation(f: &mut Formatter, imp: &Implementation) {
        let mut ftb = TableFormatter::new(f, "Implementation");
        ftb.next("span", |f| f.write(&format_span(imp.span)));
        ftb.next("trait_type", |f| Self::write_type_name_opt(f, &imp.trait_type));
        ftb.next("target", |f| Self::write_type_name(f, &imp.target));
        ftb.next("items", |f| {
            let mut list = ListFormatter::new(f);
            for item in &imp.items {
                list.next(|f| Self::write_definition(f, item));
            }
        });
    }

    fn write_decorator(f: &mut Formatter, deco: &Decorator) {
        let mut ftb = TableFormatter::new(f, "Decorator");
        ftb.next("name", |f| Self::write_name(f, &deco.name));
        ftb.next("args", |f| Self::write_exp_list_opt(f, &deco.args));
    }

    fn write_visibility(f: &mut Formatter, vis: &Option<crate::ast::Visibility>) {
        if let Some(vis) = vis {
            let mut ftb = TableFormatter::new(f, "Visibility");
            ftb.next("span", |f| f.write(&format_span(vis.span)));
            ftb.next("scopes", |f| Self::write_name_list_opt(f, &vis.scopes));
        } else {
            f.write("None")
        }
    }

    fn write_type_spec_opt(f: &mut Formatter, spec: &Option<TypeSpec>) {
        if let Some(spec) = spec {
            Self::write_type_spec(f, spec)
        } else {
            f.write("None")
        }
    }

    fn write_type_spec(f: &mut Formatter, spec: &TypeSpec) {
        let mut ftb = TableFormatter::new(f, "TypeSpec");
        ftb.next("span", |f| f.write(&format_span(spec.span)));
        ftb.next("ty", |f| Self::write_type_name(f, &spec.ty));
    }

    fn write_type_name_opt(f: &mut Formatter, name: &Option<TypeName>) {
        if let Some(name) = name {
            Self::write_type_name(f, name)
        } else {
            f.write("None")
        }
    }

    fn write_type_name(f: &mut Formatter, name: &TypeName) {
        let mut ftb = TableFormatter::new(f, "TypeName");
        ftb.next("parts", |f| Self::write_name_list(f, &name.parts));
    }

    fn write_params(f: &mut Formatter, params: &[Param]) {
        let mut list = ListFormatter::new(f);
        for param in params {
            list.next(|f| {
                let mut ftb = TableFormatter::new(f, "Param");
                ftb.next("name", |f| Self::write_name(f, &param.name));
                ftb.next("type_spec", |f| Self::write_type_spec_opt(f, &param.type_spec));
            });
        }
    }

    fn write_exp_list_opt(f: &mut Formatter, exprs: &Option<Vec<Exp>>) {
        if let Some(exprs) = exprs {
            Self::write_exps(f, exprs)
        } else {
            f.write("None")
        }
    }

    fn write_name_list_opt(f: &mut Formatter, names: &Option<Vec<Name>>) {
        if let Some(names) = names {
            Self::write_name_list(f, names)
        } else {
            f.write("None")
        }
    }

    fn write_name_list(f: &mut Formatter, names: &[Name]) {
        let mut list = ListFormatter::new(f);
        for name in names {
            list.next(|f| Self::write_name(f, name));
        }
    }

    fn write_optional_name(f: &mut Formatter, name: &Option<Name>) {
        if let Some(name) = name {
            Self::write_name(f, name)
        } else {
            f.write("None")
        }
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
            StatKind::If { clauses, else_block } => {
                let mut ftb = TableFormatter::new(f, "StatKind::If");
                ftb.next("clauses", |f| {
                    let mut list = ListFormatter::new(f);
                    for clause in clauses {
                        list.next(|f| Self::write_if_clause(f, clause));
                    }
                });
                ftb.next("else_block", |f| Self::write_block_opt(f, else_block));
            }
            StatKind::ForNumeric { name, start, end, step, block } => {
                let mut ftb = TableFormatter::new(f, "StatKind::ForNumeric");
                ftb.next("name", |f| Self::write_name(f, name));
                ftb.next("start", |f| Self::write_exp(f, start));
                ftb.next("end", |f| Self::write_exp(f, end));
                ftb.next("step", |f| Self::write_exp_opt(f, step));
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::ForGeneric { names, exprs, block } => {
                let mut ftb = TableFormatter::new(f, "StatKind::ForGeneric");
                ftb.next("names", |f| {
                    let mut list = ListFormatter::new(f);
                    for name in names {
                        list.next(|f| Self::write_name(f, name));
                    }
                });
                ftb.next("exprs", |f| Self::write_exps(f, exprs));
                ftb.next("block", |f| Self::write_block(f, block));
            }
            StatKind::Break => f.write("StatKind::Break"),
            StatKind::Continue => f.write("StatKind::Continue"),
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

    fn write_if_clause(f: &mut Formatter, clause: &IfClause) {
        let mut ftb = TableFormatter::new(f, "IfClause");
        ftb.next("cond", |f| Self::write_exp(f, &clause.cond));
        ftb.next("block", |f| Self::write_block(f, &clause.block));
    }

    fn write_exp(f: &mut Formatter, exp: &Exp) {
        let mut ftb = TableFormatter::new(f, "Exp");
        ftb.next("span", |f| f.write(&format_span(exp.span)));
        ftb.next("kind", |f| Self::write_exp_kind(f, &exp.kind));
    }

    fn write_exp_kind(f: &mut Formatter, kind: &ExpKind) {
        match kind {
            ExpKind::Nil => f.write("ExpKind::Nil"),
            ExpKind::Bool(value) => f.write(&format!("ExpKind::Bool({})", value)),
            ExpKind::Number(text) => f.write(&format!("ExpKind::Number({})", quoted(text))),
            ExpKind::String(text) => f.write(&format!("ExpKind::String({})", quoted(text))),
            ExpKind::Prefix(prefix) => Self::write_prefix_exp(f, prefix),
            ExpKind::Lambda(lambda) => Self::write_lambda_expr(f, lambda),
            ExpKind::Unary { op, exp } => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Unary");
                ftb.next("op", |f| f.write(format_unop(*op)));
                ftb.next("exp", |f| Self::write_exp(f, exp));
            }
            ExpKind::Binary { op, left, right } => {
                let mut ftb = TableFormatter::new(f, "ExpKind::Binary");
                ftb.next("op", |f| f.write(format_binop(*op)));
                ftb.next("left", |f| Self::write_exp(f, left));
                ftb.next("right", |f| Self::write_exp(f, right));
            }
        }
    }

    fn write_lambda_expr(f: &mut Formatter, lambda: &LambdaExpr) {
        let mut ftb = TableFormatter::new(f, "LambdaExpr");
        ftb.next("span", |f| f.write(&format_span(lambda.span)));
        ftb.next("is_const", |f| f.write(&format!("{}", lambda.is_const)));
        ftb.next("params", |f| Self::write_params(f, &lambda.params));
        ftb.next("return_type", |f| Self::write_type_spec(f, &lambda.return_type));
        ftb.next("block", |f| Self::write_block(f, &lambda.block));
    }

    fn write_prefix_exp(f: &mut Formatter, prefix: &PrefixExp) {
        let mut ftb = TableFormatter::new(f, "PrefixExp");
        ftb.next("span", |f| f.write(&format_span(prefix.span)));
        ftb.next("kind", |f| Self::write_prefix_kind(f, &prefix.kind));
    }

    fn write_prefix_kind(f: &mut Formatter, kind: &PrefixExpKind) {
        match kind {
            PrefixExpKind::Var(var) => Self::write_var(f, var),
            PrefixExpKind::Call(call) => Self::write_function_call(f, call),
            PrefixExpKind::Paren(exp) => Self::write_exp(f, exp),
        }
    }

    fn write_var(f: &mut Formatter, var: &Var) {
        let mut ftb = TableFormatter::new(f, "Var");
        ftb.next("span", |f| f.write(&format_span(var.span)));
        ftb.next("kind", |f| Self::write_var_kind(f, &var.kind));
    }

    fn write_var_kind(f: &mut Formatter, kind: &VarKind) {
        match kind {
            VarKind::Name(name) => {
                let mut ftb = TableFormatter::new(f, "VarKind::Name");
                ftb.next("name", |f| Self::write_name(f, name));
            }
            VarKind::Index { prefix, index } => {
                let mut ftb = TableFormatter::new(f, "VarKind::Index");
                ftb.next("prefix", |f| Self::write_prefix_exp(f, prefix));
                ftb.next("index", |f| Self::write_exp(f, index));
            }
            VarKind::Field { prefix, name } => {
                let mut ftb = TableFormatter::new(f, "VarKind::Field");
                ftb.next("prefix", |f| Self::write_prefix_exp(f, prefix));
                ftb.next("name", |f| Self::write_name(f, name));
            }
            VarKind::Decl { kind, name, type_spec } => {
                let mut ftb = TableFormatter::new(f, "VarKind::Decl");
                ftb.next("kind", |f| f.write(format_var_decl_kind(*kind)));
                ftb.next("name", |f| Self::write_name(f, name));
                ftb.next("type_spec", |f| Self::write_type_spec_opt(f, type_spec));
            }
        }
    }

    fn write_function_call(f: &mut Formatter, call: &crate::ast::FunctionCall) {
        let mut ftb = TableFormatter::new(f, "FunctionCall");
        ftb.next("span", |f| f.write(&format_span(call.span)));
        ftb.next("prefix", |f| Self::write_prefix_exp(f, &call.prefix));
        ftb.next("method", |f| Self::write_optional_name(f, &call.method));
        ftb.next("args", |f| Self::write_args(f, &call.args));
    }

    fn write_args(f: &mut Formatter, args: &Args) {
        let mut ftb = TableFormatter::new(f, "Args");
        ftb.next("span", |f| f.write(&format_span(args.span)));
        ftb.next("kind", |f| {
            match &args.kind {
                ArgsKind::ExpList(exprs) => {
                    let mut list = ListFormatter::new_named(f, "ArgsKind::ExpList");
                    for expr in exprs {
                        list.next(|f| Self::write_exp(f, expr));
                    }
                }
                ArgsKind::Initializer(init) => {
                    let mut ftb = TableFormatter::new(f, "ArgsKind::Initializer");
                    ftb.next("initializer", |f| Self::write_initializer(f, init));
                }
            }
        });
    }

    fn write_initializer(f: &mut Formatter, init: &Initializer) {
        let mut ftb = TableFormatter::new(f, "Initializer");
        ftb.next("span", |f| f.write(&format_span(init.span)));
        ftb.next("fields", |f| {
            let mut list = ListFormatter::new(f);
            for field in &init.fields {
                list.next(|f| Self::write_field(f, field));
            }
        });
    }

    fn write_field(f: &mut Formatter, field: &Field) {
        let mut ftb = TableFormatter::new(f, "Field");
        ftb.next("key", |f| Self::write_field_key(f, &field.key));
        ftb.next("value", |f| Self::write_exp(f, &field.value));
    }

    fn write_field_key(f: &mut Formatter, key: &Option<FieldKey>) {
        if let Some(key) = key {
            match key {
                FieldKey::Exp(exp) => {
                    let mut ftb = TableFormatter::new(f, "FieldKey::Exp");
                    ftb.next("exp", |f| Self::write_exp(f, exp));
                }
                FieldKey::Name(name) => {
                    let mut ftb = TableFormatter::new(f, "FieldKey::Name");
                    ftb.next("name", |f| Self::write_name(f, name));
                }
            }
        } else {
            f.write("None")
        }
    }

    fn write_block_opt(f: &mut Formatter, block: &Option<Block>) {
        if let Some(block) = block {
            Self::write_block(f, block)
        } else {
            f.write("None")
        }
    }

    fn write_exp_opt(f: &mut Formatter, exp: &Option<Exp>) {
        if let Some(exp) = exp {
            Self::write_exp(f, exp)
        } else {
            f.write("None")
        }
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
    format!("Span({})", span)
}

fn format_comment_kind(kind: CommentKind) -> &'static str {
    match kind {
        CommentKind::Line => "Line",
        CommentKind::Block => "Block",
    }
}

fn format_unop(op: UnOp) -> &'static str {
    match op {
        UnOp::Neg => "UnOp::Neg",
        UnOp::Not => "UnOp::Not",
        UnOp::Len => "UnOp::Len",
        UnOp::BitNot => "UnOp::BitNot",
    }
}

fn format_binop(op: BinOp) -> &'static str {
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

fn format_var_decl_kind(kind: VarDeclKind) -> &'static str {
    match kind {
        VarDeclKind::Var => "VarDeclKind::Var",
        VarDeclKind::Val => "VarDeclKind::Val",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        Block, Chunk, DefExpr, Definition, Exp, ExpKind, LambdaExpr, Name,
        NamespaceDecl, Stat, StatKind, TopItem, TypeName, TypeSpec, UseDecl, Var, VarDeclKind,
        VarKind,
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
            kind: StatKind::Assign {
                vars: vec![Var {
                    span: span(),
                    kind: VarKind::Decl {
                        kind: VarDeclKind::Var,
                        name,
                        type_spec: None,
                    },
                }],
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
            uses: vec![UseDecl {
                span: span(),
                path: vec![Name {
                    value: "System".to_string(),
                    span: span(),
                }],
                tail: None,
            }],
            namespace: NamespaceDecl {
                span: span(),
                path: vec![Name {
                    value: "Example".to_string(),
                    span: span(),
                }],
            },
            items: vec![TopItem::Definition(Definition {
                span: span(),
                decorators: Vec::new(),
                visibility: None,
                name: Name {
                    value: "f".to_string(),
                    span: span(),
                },
                type_spec: None,
                expr: DefExpr::Exp(Exp {
                    span: span(),
                    kind: ExpKind::Lambda(LambdaExpr {
                        span: span(),
                        is_const: false,
                        params: Vec::new(),
                        return_type: TypeSpec {
                            span: span(),
                            ty: TypeName {
                                span: span(),
                                parts: vec![Name {
                                    value: "unit".to_string(),
                                    span: span(),
                                }],
                            },
                        },
                        block,
                    }),
                }),
            })],
        };
        let text = Emitter::emit_chunk(&chunk);
        assert!(text.contains("Chunk"));
        assert!(text.contains("Definition"));
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
}
