# Nova Lexical Structure (Current Implementation)

This document describes the lexical structures implemented by the current lexer. It is based on the actual behavior in `src/lexer.rs` and uses standardized English plus BNF-style productions. The intent is descriptive, not prescriptive.

## 1. Source Text and Positions

- The input is a byte string interpreted as UTF-8.
- Positions track byte offsets, grapheme-cluster offsets, and line/column counts.
- Line breaks are recognized as CRLF (`\r\n`), LF (`\n`), or CR (`\r`).
- Grapheme-cluster offsets advance by one per Unicode grapheme cluster; line/column counters reset on any line break.

BNF (conceptual):
```
<source> ::= { <token> | <trivia> }
<trivia> ::= <whitespace> | <comment>
```

## 2. Whitespace

- Whitespace includes: space (`U+0020`), horizontal tab (`U+0009`), vertical tab (`U+000B`), and form feed (`U+000C`).
- Line breaks are treated separately (see Section 1) and update line/column counters.
- Whitespace and line breaks are skipped and do not form tokens.

BNF:
```
<whitespace> ::= " " | "\t" | "\v" | "\f"
<linebreak> ::= "\r\n" | "\n" | "\r"
```

## 3. Comments

- Line comments start with `--` and continue until a line break or EOF.
- Long (block) comments start with `--[` followed by zero or more `=` and then `[`.
  - The close delimiter is `]` followed by the same number of `=` and then `]`.
  - The lexer skips the first line break after the opening delimiter, if present.
- Comments are stored as trivia and attached to tokens as leading or trailing:
  - If there is no newline since the previous token, a comment becomes trailing trivia on the previous token.
  - Otherwise, the comment becomes leading trivia for the next token (or EOF).

BNF:
```
<comment> ::= <line-comment> | <long-comment>
<line-comment> ::= "--" { <non-linebreak> }
<long-comment> ::= "--" <long-bracket>

<long-bracket> ::= "[" <equals> "[" <long-body> "]" <equals> "]"
<equals> ::= { "=" }
<long-body> ::= { <long-char> }
```

Notes:
- `<non-linebreak>` is any character except a line break.
- `<long-char>` is any character; the lexer scans until the matching closing delimiter.

## 4. Identifiers and Keywords

- Identifiers are ASCII-only.
- An identifier starts with a letter `A-Z` or `a-z`, or underscore `_`.
- Subsequent characters may be letters, digits `0-9`, or underscore `_`.
- Keywords are recognized and returned as keyword tokens instead of identifiers.

Keywords:
```
and break do else elseif end false for function goto if in local nil not or repeat return then true until while
```

BNF:
```
<identifier> ::= <ident-start> { <ident-continue> }
<ident-start> ::= "_" | <ascii-letter>
<ident-continue> ::= <ident-start> | <digit>
<ascii-letter> ::= "A".."Z" | "a".."z"
<digit> ::= "0".."9"
<keyword> ::= "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for" | "function" | "goto" | "if" | "in" | "local" | "nil" | "not" | "or" | "repeat" | "return" | "then" | "true" | "until" | "while"
```

## 5. Numerals

The lexer supports decimal and hexadecimal numerals. Underscores are not supported.

### 5.1 Decimal Numerals

- Start with a digit, or a dot followed by a digit.
- Optional fractional part after `.`.
- Optional exponent part: `e` or `E`, optional `+` or `-`, followed by decimal digits.

BNF:
```
<decimal> ::= <dec-int> [ "." <dec-frac> ] [ <dec-exp> ]
<dec-int> ::= <digit> { <digit> }
<dec-frac> ::= { <digit> }
<dec-exp> ::= ("e" | "E") ["+" | "-"] <dec-int>
```

### 5.2 Hexadecimal Numerals

- Start with `0x` or `0X`.
- Hex digits for integer part.
- Optional fractional part after `.` (hex digits).
- Optional binary exponent part: `p` or `P`, optional `+` or `-`, followed by decimal digits.

BNF:
```
<hex> ::= ("0x" | "0X") <hex-int> [ "." <hex-frac> ] [ <hex-exp> ]
<hex-int> ::= <hex-digit> { <hex-digit> }
<hex-frac> ::= { <hex-digit> }
<hex-exp> ::= ("p" | "P") ["+" | "-"] <dec-int>
<hex-digit> ::= <digit> | "a".."f" | "A".."F"
```

## 6. String Literals

The lexer supports short-quoted strings and long-bracket strings.

### 6.1 Short Strings

- Delimiters: single quote `'` or double quote `"`.
- Backslash escape sequences are accepted but not decoded; the lexer preserves them as two characters (e.g., `"\n"` remains `\\n` in the token text).
- A line break inside a short string is an error.

BNF:
```
<short-string> ::= "\"" { <short-char> } "\"" | "'" { <short-char> } "'"
<short-char> ::= <escape> | <non-quote-non-linebreak>
<escape> ::= "\\" <any-char>
```

### 6.2 Long Strings

- Start with `[` followed by zero or more `=` and then `[`.
- End with `]` followed by the same number of `=` and then `]`.
- The first line break after the opening delimiter is skipped.
- Line breaks inside the long string are normalized to `\n` in the token text.

BNF:
```
<long-string> ::= <long-bracket>
<long-bracket> ::= "[" <equals> "[" <long-body> "]" <equals> "]"
<equals> ::= { "=" }
<long-body> ::= { <long-char> }
```

## 7. Symbols and Punctuation

The following symbols are recognized as individual tokens, with greedy matching for multi-character forms:

- Multi-character: `...` `..` `==` `~=` `<=` `>=` `<<` `>>` `//`
- Single-character: `+` `-` `*` `/` `%` `^` `#` `&` `~` `|` `<` `>` `=` `(` `)` `{` `}` `[` `]` `;` `:` `,` `.`

BNF:
```
<symbol> ::= "..." | ".." | "==" | "~=" | "<=" | ">=" | "<<" | ">>" | "//"
           | "+" | "-" | "*" | "/" | "%" | "^" | "#" | "&" | "~" | "|"
           | "<" | ">" | "=" | "(" | ")" | "{" | "}" | "[" | "]" | ";" | ":" | "," | "."
```

## 8. Token Stream and Trivia Attachment

- Each token carries `leading` and `trailing` comment trivia lists.
- Comments are attached based on whether a newline has occurred since the previous token.
- All comments are later collected into `Chunk.comments` by the parser, preserving the order they appear in the token stream.
