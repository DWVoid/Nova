# Nova Grammar (Current Parser)

This document describes the grammar implemented by the current parser. It follows the same style as the lexical document: standardized English plus BNF-style productions. The intent is descriptive, not prescriptive, and it mirrors the behavior in `src/parser/`.

Notes:
- Terminals are quoted (e.g., "if", "+").
- Non-terminals use angle brackets (e.g., `<exp>`).
- `{ ... }` means repetition (zero or more).
- `[ ... ]` means optional.

## 1. Entry Point and Blocks

- A compilation unit is a `<chunk>`, which is a `<block>` followed by end-of-file.
- A `<block>` is a sequence of statements, optionally ending with a return statement.
- Block termination depends on context:
  - Top-level blocks end at EOF.
  - Nested blocks end at one of: "end", "else", "elseif", or "until".

BNF:
```
<chunk> ::= <block> <eof>
<block> ::= { <stat> } [ <retstat> [";"] ]
```

## 2. Statements

### 2.1 Empty Statement

BNF:
```
<stat> ::= ";"
```

### 2.2 Assignment and Call Statements

- A call statement is a prefix expression that resolves to a function call.
- An assignment requires a comma-separated variable list and an expression list.
- A parenthesized expression cannot start a statement.
- A function call cannot be used as an assignment target.

BNF:
```
<stat> ::= <varlist> "=" <explist>
        | <functioncall>

<varlist> ::= <var> { "," <var> }
<explist> ::= <exp> { "," <exp> }
```

### 2.3 Local Declarations

- Local assignment: "local" name list, optional initializer list.
- Local function: "local function" name funcbody.
- Local attributes: `<const>` or `<close>` after a local name, using `<` and `>` tokens.

BNF:
```
<stat> ::= "local" <localnames> [ "=" <explist> ]
        | "local" "function" <name> <funcbody>

<localnames> ::= <localname> { "," <localname> }
<localname> ::= <name> [ "<" ("const" | "close") ">" ]
```

### 2.4 Function Statements

BNF:
```
<stat> ::= "function" <funcname> <funcbody>

<funcname> ::= <name> { "." <name> } [ ":" <name> ]
```

### 2.5 Control Flow Statements

BNF:
```
<stat> ::= "do" <block> "end"
        | "while" <exp> "do" <block> "end"
        | "repeat" <block> "until" <exp>
        | "if" <exp> "then" <block> { "elseif" <exp> "then" <block> } [ "else" <block> ] "end"
        | "for" <name> "=" <exp> "," <exp> [ "," <exp> ] "do" <block> "end"
        | "for" <namelist> "in" <explist> "do" <block> "end"
        | "break"
        | "goto" <name>
        | "::" <name> "::"

<namelist> ::= <name> { "," <name> }
```

### 2.6 Return Statement

- Return is allowed only as the last statement in a block.
- A return statement may be followed by an optional semicolon.

BNF:
```
<retstat> ::= "return" [ <explist> ]
```

## 3. Expressions

### 3.1 Expression Forms

BNF:
```
<exp> ::= "nil"
        | "false"
        | "true"
        | <number>
        | <string>
        | "..."
        | <functiondef>
        | <tableconstructor>
        | <prefixexp>
        | <unop> <exp>
        | <exp> <binop> <exp>
```

Notes:
- Vararg ("...") is only valid inside a vararg function body.

### 3.2 Unary Operators

BNF:
```
<unop> ::= "not" | "-" | "#" | "~"
```

### 3.3 Binary Operators and Precedence

The parser uses a precedence-climbing algorithm with the following precedence and associativity:

1. `or` (left)
2. `and` (left)
3. Comparisons: `<` `<=` `>` `>=` `==` `~=` (left)
4. Bitwise OR: `|` (left)
5. Bitwise XOR: `~` (left)
6. Bitwise AND: `&` (left)
7. Shifts: `<<` `>>` (left)
8. Concatenation: `..` (right)
9. Additive: `+` `-` (left)
10. Multiplicative: `*` `/` `//` `%` (left)
11. Exponentiation: `^` (right)

BNF (operator set):
```
<binop> ::= "or" | "and"
         | "<" | "<=" | ">" | ">=" | "==" | "~="
         | "|" | "~" | "&"
         | "<<" | ">>"
         | ".."
         | "+" | "-"
         | "*" | "/" | "//" | "%"
         | "^"
```

## 4. Prefix Expressions, Variables, and Calls

### 4.1 Prefix Expressions

- A prefix expression starts with a name or a parenthesized expression.
- It can be followed by any number of suffixes (field access, index, or call forms).

BNF:
```
<prefixexp> ::= <name> { <suffix> }
              | "(" <exp> ")" { <suffix> }

<suffix> ::= "." <name>
          | "[" <exp> "]"
          | ":" <name> <args>
          | <args>
```

### 4.2 Variables

- A variable is a name, or a field/index access on a prefix expression.

BNF:
```
<var> ::= <name>
        | <prefixexp> "[" <exp> "]"
        | <prefixexp> "." <name>
```

### 4.3 Function Calls

- A function call is a prefix expression followed by a call suffix.

BNF:
```
<functioncall> ::= <prefixexp> <args>
                 | <prefixexp> ":" <name> <args>
```

## 5. Function Definitions

### 5.1 Function Body

BNF:
```
<functiondef> ::= "function" <funcbody>
<funcbody> ::= "(" [ <parlist> ] ")" <block> "end"
```

### 5.2 Parameter List

- A parameter list is either just vararg, or a comma-separated name list with optional trailing vararg.

BNF:
```
<parlist> ::= "..."
            | <namelist> [ "," "..." ]
```

## 6. Table Constructors

- Fields can be explicit key/value pairs or implicit array entries.
- Field separators can be `,` or `;`.

BNF:
```
<tableconstructor> ::= "{" [ <fieldlist> ] "}"
<fieldlist> ::= <field> { <fieldsep> <field> } [ <fieldsep> ]
<fieldsep> ::= "," | ";"

<field> ::= "[" <exp> "]" "=" <exp>
          | <name> "=" <exp>
          | <exp>
```

## 7. Call Arguments

BNF:
```
<args> ::= "(" [ <explist> ] ")"
        | <tableconstructor>
        | <string>
```

## 8. Terminals and Non-terminals

- `<name>` is an identifier token from the lexer.
- `<number>` and `<string>` are numeric and string literal tokens from the lexer.
- `<eof>` is the end-of-file token emitted by the lexer.

## 9. Known Deviations or Constraints

- Identifiers are ASCII-only (per lexer).
- Vararg ("...") is only permitted within function bodies that declare vararg parameters.
- Parenthesized expressions cannot start a statement.
- Call expressions cannot be used as assignment targets.
