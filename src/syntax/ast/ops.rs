use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum UnOp {
    Neg,
    Not,
    Len,
    BitNot,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
