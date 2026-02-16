use crate::token::{Comment, Span};

pub type Comments = Vec<Comment>;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub span: Span,
    pub comments: Comments,
    pub uses: Vec<UseDecl>,
    pub namespace: NamespaceDecl,
    pub items: Vec<TopItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TopItem {
    Definition(Definition),
    Implementation(Implementation),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseDecl {
    pub span: Span,
    pub path: Vec<Name>,
    pub tail: Option<UseTail>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UseTail {
    Selector(Vec<UseItem>),
    Alias(Name),
}

#[derive(Clone, Debug, PartialEq)]
pub struct UseItem {
    pub name: Name,
    pub alias: Option<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamespaceDecl {
    pub span: Span,
    pub path: Vec<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Visibility {
    pub span: Span,
    pub scopes: Option<Vec<Name>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub span: Span,
    pub decorators: Vec<Decorator>,
    pub visibility: Option<Visibility>,
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
    pub expr: DefExpr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DefExpr {
    Struct(StructDef),
    Enum(EnumDef),
    Variant(VariantDef),
    Trait(TraitDef),
    Exp(Exp),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decorator {
    pub span: Span,
    pub name: Name,
    pub args: Option<Vec<Exp>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeName {
    pub span: Span,
    pub parts: Vec<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeSpec {
    pub span: Span,
    pub ty: TypeName,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StructDef {
    pub span: Span,
    pub fields: Vec<FieldDecl>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDecl {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumDef {
    pub span: Span,
    pub type_spec: TypeSpec,
    pub members: Vec<EnumMember>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnumMember {
    pub span: Span,
    pub name: Name,
    pub value: Exp,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantDef {
    pub span: Span,
    pub members: Vec<VariantMember>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VariantMember {
    pub span: Span,
    pub name: Name,
    pub type_spec: TypeSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitDef {
    pub span: Span,
    pub sigs: Vec<TraitSig>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraitSig {
    pub span: Span,
    pub name: Name,
    pub params: Vec<Param>,
    pub return_type: TypeSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Implementation {
    pub span: Span,
    pub trait_type: Option<TypeName>,
    pub target: TypeName,
    pub items: Vec<Definition>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub span: Span,
    pub stats: Vec<Stat>,
    pub ret: Option<RetStat>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stat {
    pub span: Span,
    pub kind: StatKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatKind {
    Empty,
    Assign { vars: Vec<Var>, exprs: Vec<Exp> },
    Do { block: Block },
    While { cond: Exp, block: Block },
    Repeat { block: Block, cond: Exp },
    If { clauses: Vec<IfClause>, else_block: Option<Block> },
    ForNumeric {
        name: Name,
        start: Exp,
        end: Exp,
        step: Option<Exp>,
        block: Block,
    },
    ForGeneric { names: Vec<Name>, exprs: Vec<Exp>, block: Block },
    Break,
    Continue,
    Goto { label: Name },
    Label { label: Name },
    Call { call: FunctionCall },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetStat {
    pub span: Span,
    pub exprs: Vec<Exp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IfClause {
    pub span: Span,
    pub cond: Exp,
    pub block: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub value: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: Name,
    pub type_spec: Option<TypeSpec>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub span: Span,
    pub kind: VarKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VarKind {
    Name(Name),
    Index { prefix: Box<PrefixExp>, index: Box<Exp> },
    Field { prefix: Box<PrefixExp>, name: Name },
    Decl { kind: VarDeclKind, name: Name, type_spec: Option<TypeSpec> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VarDeclKind {
    Var,
    Val,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrefixExp {
    pub span: Span,
    pub kind: PrefixExpKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PrefixExpKind {
    Var(Var),
    Call(FunctionCall),
    Paren(Box<Exp>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionCall {
    pub span: Span,
    pub prefix: Box<PrefixExp>,
    pub method: Option<Name>,
    pub args: Args,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub span: Span,
    pub kind: ArgsKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgsKind {
    ExpList(Vec<Exp>),
    Initializer(Initializer),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exp {
    pub span: Span,
    pub kind: ExpKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExpKind {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Initializer(Initializer),
    Prefix(PrefixExp),
    Lambda(LambdaExpr),
    Unary { op: UnOp, exp: Box<Exp> },
    Binary { op: BinOp, left: Box<Exp>, right: Box<Exp> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct LambdaExpr {
    pub span: Span,
    pub is_const: bool,
    pub params: Vec<Param>,
    pub return_type: TypeSpec,
    pub block: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Initializer {
    pub span: Span,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub span: Span,
    pub key: Option<FieldKey>,
    pub value: Exp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKey {
    Exp(Exp),
    Name(Name),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    BitOr,
    BitXor,
    BitAnd,
    ShiftLeft,
    ShiftRight,
    Concat,
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::{CommentKind, Position};

    fn span() -> Span {
        Span::new(Position::start(), Position::start())
    }

    #[test]
    fn constructs_simple_chunk() {
        let comment = Comment {
            kind: CommentKind::Line,
            text: "-- test".to_string(),
            span: span(),
        };
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
            comments: vec![comment],
            uses: Vec::new(),
            namespace: NamespaceDecl {
                span: span(),
                path: Vec::new(),
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
                                parts: Vec::new(),
                            },
                        },
                        block,
                    }),
                }),
            })],
        };
        assert_eq!(chunk.items.len(), 1);
    }
}