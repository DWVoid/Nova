use crate::token::{Comment, Span};

pub type Comments = Vec<Comment>;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub span: Span,
    pub leading_comments: Comments,
    pub block: Block,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub span: Span,
    pub leading_comments: Comments,
    pub stats: Vec<Stat>,
    pub ret: Option<RetStat>,
    pub detached_comments: Comments,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stat {
    pub span: Span,
    pub leading_comments: Comments,
    pub kind: StatKind,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StatKind {
    Empty,
    Assign { vars: Vec<Var>, exprs: Vec<Exp> },
    LocalAssign { names: Vec<LocalName>, exprs: Vec<Exp> },
    LocalFunction { name: Name, func: FuncBody },
    Function { name: FuncName, func: FuncBody },
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
    Goto { label: Name },
    Label { label: Name },
    Call { call: FunctionCall },
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetStat {
    pub span: Span,
    pub leading_comments: Comments,
    pub exprs: Vec<Exp>,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IfClause {
    pub span: Span,
    pub leading_comments: Comments,
    pub cond: Exp,
    pub block: Block,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalName {
    pub name: Name,
    pub attr: Option<LocalAttr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAttr {
    Const,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Name {
    pub value: String,
    pub span: Span,
    pub leading_comments: Comments,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncName {
    pub span: Span,
    pub leading_comments: Comments,
    pub names: Vec<Name>,
    pub method: Option<Name>,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Var {
    pub span: Span,
    pub leading_comments: Comments,
    pub kind: VarKind,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VarKind {
    Name(Name),
    Index { prefix: Box<PrefixExp>, index: Box<Exp> },
    Field { prefix: Box<PrefixExp>, name: Name },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PrefixExp {
    pub span: Span,
    pub leading_comments: Comments,
    pub kind: PrefixExpKind,
    pub trailing_comments: Comments,
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
    pub leading_comments: Comments,
    pub prefix: Box<PrefixExp>,
    pub method: Option<Name>,
    pub args: Args,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub span: Span,
    pub leading_comments: Comments,
    pub kind: ArgsKind,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgsKind {
    ExpList(Vec<Exp>),
    Table(TableConstructor),
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Exp {
    pub span: Span,
    pub leading_comments: Comments,
    pub kind: ExpKind,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExpKind {
    Nil,
    Bool(bool),
    Number(String),
    String(String),
    Vararg,
    FuncDef(FuncBody),
    Table(TableConstructor),
    Prefix(PrefixExp),
    Unary { op: UnOp, exp: Box<Exp> },
    Binary { op: BinOp, left: Box<Exp>, right: Box<Exp> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncBody {
    pub span: Span,
    pub leading_comments: Comments,
    pub params: Vec<Name>,
    pub is_vararg: bool,
    pub block: Block,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableConstructor {
    pub span: Span,
    pub leading_comments: Comments,
    pub fields: Vec<Field>,
    pub trailing_comments: Comments,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub span: Span,
    pub leading_comments: Comments,
    pub key: Option<FieldKey>,
    pub value: Exp,
    pub trailing_comments: Comments,
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
            leading_comments: vec![],
            trailing_comments: vec![],
        };
        let exp = Exp {
            span: span(),
            leading_comments: vec![],
            kind: ExpKind::Number("1".to_string()),
            trailing_comments: vec![],
        };
        let stat = Stat {
            span: span(),
            leading_comments: vec![comment.clone()],
            kind: StatKind::LocalAssign {
                names: vec![LocalName { name, attr: None }],
                exprs: vec![exp],
            },
            trailing_comments: vec![],
        };
        let block = Block {
            span: span(),
            leading_comments: vec![],
            stats: vec![stat],
            ret: None,
            detached_comments: vec![],
            trailing_comments: vec![],
        };
        let chunk = Chunk {
            span: span(),
            leading_comments: vec![comment],
            block,
            trailing_comments: vec![],
        };
        assert_eq!(chunk.block.stats.len(), 1);
    }
}
