use super::name::Name;
use super::top::TopItem;
use super::use_decl::UseDecl;
use crate::lexical::{Keyword, Span, Symbol, Token, TokenKind, Trivia as LexTrivia};
use crate::syntax::parse::Parsable;
use crate::syntax::parse::Parser;
use serde::{Deserialize, Serialize};
use crate::syntax::syntax::SyntaxError;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamespaceDecl {
    pub span: Span,
    pub path: Vec<Name>,
}

impl Parsable for NamespaceDecl {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let token = p.expect_keyword(Keyword::Namespace)?;
        let path = p.parse_namespace_path()?;
        let semi = p.expect_symbol(Symbol::Semi)?;
        let span = token.span.merge(semi.span);
        Ok(NamespaceDecl { span, path })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub span: Span,
    pub trivia: Vec<LexTrivia>,
    pub uses: Vec<UseDecl>,
    pub namespace: NamespaceDecl,
    pub items: Vec<TopItem>,
}

fn expect_eof(p: &mut Parser) -> Result<Token, SyntaxError> {
    let token = p.current().clone();
    if matches!(token.kind, TokenKind::Eof) {
        p.advance();
        return Ok(token);
    }
    Err(SyntaxError {
        message: "expected EOF".to_string(),
        position: token.span.start,
    })
}

impl Parsable for Chunk {
    fn parse(p: &mut Parser) -> Result<Self, SyntaxError> {
        let trivia = std::mem::take(&mut p.trivia);

        let mut uses = Vec::new();
        while p.is_keyword(Keyword::Use) {
            uses.push(UseDecl::parse(p)?);
        }

        let namespace = NamespaceDecl::parse(p)?;

        let mut items = Vec::new();
        while !matches!(p.current().kind, TokenKind::Eof) {
            items.push(TopItem::parse(p)?);
        }

        let eof = expect_eof(p)?;
        let span = if let Some(last) = items.last() {
            let last_span = match last {
                TopItem::Definition(def) => def.span,
                TopItem::Implementation(imp) => imp.span,
            };
            namespace.span.merge(last_span).merge(eof.span)
        } else if let Some(last_use) = uses.last() {
            namespace.span.merge(last_use.span).merge(eof.span)
        } else {
            namespace.span.merge(eof.span)
        };

        Ok(Chunk {
            span,
            trivia,
            uses,
            namespace,
            items,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexical::transform;
    use crate::syntax::parse::Parser;

    fn parser(src: &str) -> Parser {
        let r = transform(src).unwrap();
        Parser::new(r.tokens, r.trivia)
    }

    // ── NamespaceDecl ─────────────────────────────────────────────────────

    #[test]
    fn parses_namespace() {
        let n = NamespaceDecl::parse(&mut parser("namespace Foo.Bar;")).unwrap();
        assert_eq!(n.path.len(), 2);
    }

    #[test]
    fn namespace_rejects_missing_semi() {
        assert!(NamespaceDecl::parse(&mut parser("namespace Foo")).is_err());
    }

    // ── Chunk ─────────────────────────────────────────────────────────────

    #[test]
    fn parses_minimal_chunk() {
        let mut p = parser("namespace Foo;");
        let c = Chunk::parse(&mut p).unwrap();
        assert_eq!(c.namespace.path[0].value, "Foo");
        assert!(c.uses.is_empty());
        assert!(c.items.is_empty());
    }

    #[test]
    fn parses_chunk_with_use() {
        let mut p = parser("use Bar; namespace Foo;");
        let c = Chunk::parse(&mut p).unwrap();
        assert_eq!(c.uses.len(), 1);
    }
}
