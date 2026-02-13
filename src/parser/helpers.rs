use super::{Assoc, BlockEnd, ParseError, Parser};
use crate::token::{Keyword, Symbol, Token, TokenKind};

impl Parser {
    pub(crate) fn advance(&mut self) -> Token {
        let token = self.current().clone();
        self.index += 1;
        token
    }

    pub(crate) fn peek(&self, offset: usize) -> &Token {
        let idx = (self.index + offset).min(self.tokens.len() - 1);
        &self.tokens[idx]
    }

    pub(crate) fn peek_is_symbol(&self, offset: usize, symbol: Symbol) -> bool {
        matches!(self.peek(offset).kind, TokenKind::Symbol(s) if s == symbol)
    }

    pub(crate) fn expect_symbol(&mut self, symbol: Symbol) -> Result<Token, ParseError> {
        let token = self.current().clone();
        if matches!(token.kind, TokenKind::Symbol(s) if s == symbol) {
            self.index += 1;
            return Ok(token);
        }
        Err(ParseError {
            message: format!("expected symbol {:?}", symbol),
            position: token.span.start,
        })
    }

    pub(crate) fn current(&self) -> &Token {
        &self.tokens[self.index]
    }

    pub(crate) fn is_keyword(&self, keyword: Keyword) -> bool {
        matches!(self.current().kind, TokenKind::Keyword(k) if k == keyword)
    }

    pub(crate) fn is_symbol(&self, symbol: Symbol) -> bool {
        matches!(self.current().kind, TokenKind::Symbol(s) if s == symbol)
    }

    pub(crate) fn is_block_end(&self, end: BlockEnd) -> bool {
        if matches!(self.current().kind, TokenKind::Eof) {
            return true;
        }
        match end {
            BlockEnd::Chunk => matches!(self.current().kind, TokenKind::Eof),
            BlockEnd::Nested => matches!(
                self.current().kind,
                TokenKind::Keyword(Keyword::End)
                    | TokenKind::Keyword(Keyword::Else)
                    | TokenKind::Keyword(Keyword::ElseIf)
                    | TokenKind::Keyword(Keyword::Until)
            ),
        }
    }

    pub(crate) fn expect_keyword(&mut self, keyword: Keyword) -> Result<Token, ParseError> {
        let token = self.current().clone();
        if matches!(token.kind, TokenKind::Keyword(k) if k == keyword) {
            self.index += 1;
            return Ok(token);
        }
        Err(ParseError {
            message: format!("expected keyword {:?}", keyword),
            position: token.span.start,
        })
    }

    pub(crate) fn expect_eof(&mut self) -> Result<Token, ParseError> {
        let token = self.current().clone();
        if matches!(token.kind, TokenKind::Eof) {
            self.index += 1;
            return Ok(token);
        }
        Err(ParseError {
            message: "expected EOF".to_string(),
            position: token.span.start,
        })
    }

    pub(crate) fn peek_unop(&self) -> Option<crate::ast::UnOp> {
        match self.current().kind {
            TokenKind::Keyword(Keyword::Not) => Some(crate::ast::UnOp::Not),
            TokenKind::Symbol(Symbol::Minus) => Some(crate::ast::UnOp::Neg),
            TokenKind::Symbol(Symbol::Hash) => Some(crate::ast::UnOp::Len),
            TokenKind::Symbol(Symbol::Tilde) => Some(crate::ast::UnOp::BitNot),
            _ => None,
        }
    }

    pub(crate) fn peek_binop(&self) -> Option<(crate::ast::BinOp, u8, Assoc)> {
        match self.current().kind {
            TokenKind::Keyword(Keyword::Or) => Some((crate::ast::BinOp::Or, 1, Assoc::Left)),
            TokenKind::Keyword(Keyword::And) => Some((crate::ast::BinOp::And, 2, Assoc::Left)),
            TokenKind::Symbol(Symbol::Less)
            | TokenKind::Symbol(Symbol::LessEq)
            | TokenKind::Symbol(Symbol::Greater)
            | TokenKind::Symbol(Symbol::GreaterEq)
            | TokenKind::Symbol(Symbol::EqEq)
            | TokenKind::Symbol(Symbol::NotEq) => Some((self.binop_from_symbol()?, 3, Assoc::Left)),
            TokenKind::Symbol(Symbol::Pipe) => Some((crate::ast::BinOp::BitOr, 4, Assoc::Left)),
            TokenKind::Symbol(Symbol::Tilde) => Some((crate::ast::BinOp::BitXor, 5, Assoc::Left)),
            TokenKind::Symbol(Symbol::Amp) => Some((crate::ast::BinOp::BitAnd, 6, Assoc::Left)),
            TokenKind::Symbol(Symbol::ShiftLeft) | TokenKind::Symbol(Symbol::ShiftRight) => {
                Some((self.binop_from_symbol()?, 7, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::DotDot) => Some((crate::ast::BinOp::Concat, 8, Assoc::Right)),
            TokenKind::Symbol(Symbol::Plus) | TokenKind::Symbol(Symbol::Minus) => {
                Some((self.binop_from_symbol()?, 9, Assoc::Left))
            }
            TokenKind::Symbol(Symbol::Star)
            | TokenKind::Symbol(Symbol::Slash)
            | TokenKind::Symbol(Symbol::FloorDiv)
            | TokenKind::Symbol(Symbol::Percent) => Some((self.binop_from_symbol()?, 10, Assoc::Left)),
            TokenKind::Symbol(Symbol::Caret) => Some((crate::ast::BinOp::Pow, 12, Assoc::Right)),
            _ => None,
        }
    }

    pub(crate) fn binop_from_symbol(&self) -> Option<crate::ast::BinOp> {
        match self.current().kind {
            TokenKind::Symbol(Symbol::Less) => Some(crate::ast::BinOp::Less),
            TokenKind::Symbol(Symbol::LessEq) => Some(crate::ast::BinOp::LessEq),
            TokenKind::Symbol(Symbol::Greater) => Some(crate::ast::BinOp::Greater),
            TokenKind::Symbol(Symbol::GreaterEq) => Some(crate::ast::BinOp::GreaterEq),
            TokenKind::Symbol(Symbol::EqEq) => Some(crate::ast::BinOp::Eq),
            TokenKind::Symbol(Symbol::NotEq) => Some(crate::ast::BinOp::NotEq),
            TokenKind::Symbol(Symbol::ShiftLeft) => Some(crate::ast::BinOp::ShiftLeft),
            TokenKind::Symbol(Symbol::ShiftRight) => Some(crate::ast::BinOp::ShiftRight),
            TokenKind::Symbol(Symbol::Plus) => Some(crate::ast::BinOp::Add),
            TokenKind::Symbol(Symbol::Minus) => Some(crate::ast::BinOp::Sub),
            TokenKind::Symbol(Symbol::Star) => Some(crate::ast::BinOp::Mul),
            TokenKind::Symbol(Symbol::Slash) => Some(crate::ast::BinOp::Div),
            TokenKind::Symbol(Symbol::FloorDiv) => Some(crate::ast::BinOp::FloorDiv),
            TokenKind::Symbol(Symbol::Percent) => Some(crate::ast::BinOp::Mod),
            _ => None,
        }
    }
}