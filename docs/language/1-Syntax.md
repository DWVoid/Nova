# Nova Grammar 

Notes:
- Terminals are quoted (e.g., "if").
- Non-terminals use angle brackets (e.g., <stat>).
- `{ ... }` means repetition (zero or more).
- `[ ... ]` means optional.

## 1. Compilation Unit

BNF:
```
<compilation_unit> ::= { <use_decl> } <namespace_decl> { <top_item> } <eof>

<top_item> ::= <definition> | <implementation>
```

## 2. Use and Namespace Declarations

BNF:
```
<use_decl> ::= "use" <namespace_path> [ <use_selector> | <use_alias> ] ";"

<use_selector> ::= "." "{" <use_list> "}"
<use_list> ::= <use_item> { "," <use_item> }
<use_item> ::= <name> [ "as" <name> ]

<use_alias> ::= "as" <name>

<namespace_decl> ::= "namespace" <namespace_path> ";"

<namespace_path> ::= <name> { "." <name> }
```

## 3. Visibility Modifiers

BNF:
```
<visibility> ::= "export" [ "(" <scope_list> ")" ]
<scope_list> ::= <name> { "," <name> }
```

## 4. Definitions and Implementations

BNF:
```
<definition> ::= { <decorator> } [ <visibility> ] "define" <name> [ <type_spec> ] <def_expr> [ ";" ]

<def_expr> ::= <struct_def>
             | <enum_def>
             | <variant_def>
             | <trait_def>
             | <exp>

<implementation> ::= "implement" [ <type> ] "for" <type> <impl_body> "end"
<impl_body> ::= { <definition> }
```

### 4.1 Structs

BNF:
```
<struct_def> ::= "struct" <struct_body> "end"
<struct_body> ::= { <field_decl> }
<field_decl> ::= <name> <type_spec> [ ";" ]
```

### 4.2 Enums

BNF:
```
<enum_def> ::= "enum" <type_spec> <enum_body> "end"
<enum_body> ::= { <enum_member> }
<enum_member> ::= <name> "=" <exp> [ ";" ]
```

### 4.3 Variants

BNF:
```
<variant_def> ::= "variant" <variant_body> "end"
<variant_body> ::= { <variant_member> }
<variant_member> ::= <name> <type_spec> [ ";" ]
```

### 4.4 Traits

BNF:
```
<trait_def> ::= "trait" <trait_body> "end"
<trait_body> ::= { <trait_sig> }
<trait_sig> ::= <name> <param_list> <type_spec> [ ";" ]
```

## 5. Decorators

BNF:
```
<decorator> ::= "@" <name> [ "(" [ <explist> ] ")" ]
```

## 6. Types

BNF:
```
<type> ::= <namespace_path>
<type_spec> ::= ":" <type>
```

## 7. Blocks and Statements

BNF:
```
<block> ::= { <stat> }

<stat> ::= ";"
        | <exp> { "," <exp> } "=" <explist>
        | <exp> <args>
        | "do" <block> "end"
        | "while" <exp> "do" <block> "end"
        | "repeat" <block> "until" <exp>
        | "if" <exp> "then" <block> { "elseif" <exp> "then" <block> } [ "else" <block> ] "end"
        | "for" <name> "=" <exp> "," <exp> [ "," <exp> ] "do" <block> "end"
        | "for" <namelist> "in" <explist> "do" <block> "end"
        | "break"
        | "continue"
        | "goto" <name>
        | "::" <name> "::"
        | "return" [ <explist> ] [ ";" ]

<namelist> ::= <name> { "," <name> }
<explist>  ::= <exp> { "," <exp> }
```

A `return` statement may appear anywhere in a block, but once parsed it
terminates the block — no further statements may follow it (the optional
trailing `;` is consumed as part of the return, not as a separate empty
statement).

The left-hand side of an assignment and the target of a call statement are
written as plain expressions.  The semantic stage enforces that assignment
targets are valid l-values (`<name>`, field access, index access, or a
`var`/`val` declaration) and that call statements resolve to an actual call.

## 8. Expressions

### 8.1 Expression Forms

All expression forms, including names, field/index access, calls, and local
variable declarations, are unified into a single `<exp>` rule.  The parser
makes no distinction between l-values and r-values — that analysis is deferred
to the semantic stage.

BNF:
```
<exp> ::= "nil"
        | "false"
        | "true"
        | <number>
        | <string>
        | <name>
        | <lambda_expr>
        | "var" <name> [ <type_spec> ]
        | "val" <name> [ <type_spec> ]
        | "(" <exp> ")"
        | <exp> "." <name>
        | <exp> "[" <exp> "]"
        | <exp> <args>
        | <unop> <exp>
        | <exp> <binop> <exp>
```

Postfix operators (`.`, `[]`, and call arguments) bind tighter than any prefix
or binary operator and are left-associative.  Method calls are expressed as
`obj.method(args)` — field access followed by a call.

The `var`/`val` forms introduce a local binding site.  They are syntactically
valid in any expression position but are semantically restricted to the
left-hand side of an assignment statement.

### 8.2 Lambda Expressions

BNF:
```
<lambda_expr> ::= <param_list> <type_spec> [ "const" ] <block> "end"

<param_list> ::= "(" [ <param_items> ] ")"
<param_items> ::= <param> { "," <param> }
<param> ::= <name> [ <type_spec> ]
```

`const` placed after the return-type annotation qualifies the *body* as
const-evaluable.  This position keeps the signature `(params): type` visually
intact and avoids `const` appearing in the middle of a `define` line between
the bound name and its parameter list.

### 8.3 Unary Operators

BNF:
```
<unop> ::= "not" | "-" | "#" | "~"
```

### 8.4 Binary Operators and Precedence

Precedence (lowest to highest), with associativity:
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

## 9. Initializers

BNF:
```
<initializer> ::= "{" [ <fieldlist> ] "}"
<fieldlist> ::= <field> { "," <field> } [ "," ]

<field> ::= "[" <exp> "]" "=" <exp>
          | <name> "=" <exp>
          | <exp>
```

## 10. Call Arguments

BNF:
```
<args> ::= "(" [ <explist> ] ")"
        | <initializer>
```

## 11. AST Cross-Reference

This section cross-references every grammar rule with the Rust struct or enum
that represents it in `src/syntax/ast/`.  It also lists named AST types that
are not explicitly called out in the BNF rules above, and pure helper rules
that have no named struct of their own.

### 11.1 Grammar Rule → AST Type

| Grammar rule | AST type | Notes |
|---|---|---|
| `<compilation_unit>` | `Chunk` | |
| `<top_item>` | `TopItem` | enum |
| `<use_decl>` | `UseDecl` | |
| `<use_item>` | `UseItem` | |
| `<use_alias>` / `<use_selector>` | `UseTail` | enum with `Alias` and `Selector` variants |
| `<namespace_decl>` | `NamespaceDecl` | |
| `<visibility>` | `Visibility` | |
| `<definition>` | `Definition` | |
| `<def_expr>` | `DefExpr` | enum |
| `<implementation>` | `Implementation` | |
| `<struct_def>` | `StructDef` | |
| `<field_decl>` | `FieldDecl` | |
| `<enum_def>` | `EnumDef` | |
| `<enum_member>` | `EnumMember` | |
| `<variant_def>` | `VariantDef` | |
| `<variant_member>` | `VariantMember` | |
| `<trait_def>` | `TraitDef` | |
| `<trait_sig>` | `TraitSig` | |
| `<decorator>` | `Decorator` | |
| `<type>` | `TypeName` | |
| `<type_spec>` | `TypeSpec` | |
| `<block>` | `Block` | |
| `<stat>` | `Stat` | enum; each variant is a dedicated struct (see §11.2) |
| `<exp>` | `Exp` | enum; each variant is a dedicated struct (see §11.3) |
| `<lambda_expr>` | `ExpLambda` | |
| `<param>` | `Param` | |
| `<unop>` | `UnOp` | enum |
| `<binop>` | `BinOp` | enum |
| `<initializer>` | `Initializer` | |
| `<field>` | `Field` | struct; `FieldKey` enum for the key variant |
| `<args>` | `Args` | `ArgsKind` enum for `ExpList` vs `Initializer` |
| `<name>` | `Name` | |

### 11.2 Statement Structs (`Stat` variants)

Each variant of the `Stat` enum has a dedicated struct:

| `Stat` variant | Struct |
|---|---|
| `Stat::Empty` | `StatEmpty` |
| `Stat::Assign` | `StatAssign` |
| `Stat::Call` | `StatCall` |
| `Stat::Do` | `StatDo` |
| `Stat::While` | `StatWhile` |
| `Stat::Repeat` | `StatRepeat` |
| `Stat::If` | `StatIf` (with `IfClause` for each `if`/`elseif` arm) |
| `Stat::ForNumeric` | `StatForNumeric` |
| `Stat::ForGeneric` | `StatForGeneric` |
| `Stat::Break` | `StatBreak` |
| `Stat::Continue` | `StatContinue` |
| `Stat::Goto` | `StatGoto` |
| `Stat::Label` | `StatLabel` |
| `Stat::Return` | `StatReturn` |

### 11.3 Expression Structs (`Exp` variants)

Each variant of the `Exp` enum has a dedicated struct:

| `Exp` variant | Struct |
|---|---|
| `Exp::Nil` | `ExpNil` |
| `Exp::Bool` | `ExpBool` |
| `Exp::Number` | `ExpNumber` |
| `Exp::String` | `ExpString` |
| `Exp::Name` | `ExpName` |
| `Exp::Lambda` | `ExpLambda` |
| `Exp::VarDecl` | `ExpVarDecl` (`VarDeclKind` enum for `var`/`val`) |
| `Exp::Paren` | `ExpParen` |
| `Exp::Field` | `ExpField` |
| `Exp::Index` | `ExpIndex` |
| `Exp::Call` | `ExpCall` |
| `Exp::Unary` | `ExpUnary` |
| `Exp::Binary` | `ExpBinary` |

### 11.4 Grammar Rules Without a Dedicated Named Struct

The following grammar rules are purely structural groupings that are
represented as `Vec<T>` or inline fields rather than named structs:

| Grammar rule | Representation |
|---|---|
| `<use_selector>` | inline in `UseDecl::parse`; result stored as `UseTail::Selector(Vec<UseItem>)` |
| `<use_list>` | `Vec<UseItem>` |
| `<use_alias>` | `UseTail::Alias(Name)` |
| `<namespace_path>` | `Vec<Name>` |
| `<scope_list>` | `Vec<Name>` (inside `Visibility`) |
| `<impl_body>` | `Vec<Definition>` (inside `Implementation`) |
| `<struct_body>` | `Vec<FieldDecl>` (inside `StructDef`) |
| `<enum_body>` | `Vec<EnumMember>` (inside `EnumDef`) |
| `<variant_body>` | `Vec<VariantMember>` (inside `VariantDef`) |
| `<trait_body>` | `Vec<TraitSig>` (inside `TraitDef`) |
| `<namelist>` | `Vec<Name>` (inside `StatForGeneric`) |
| `<explist>` | `Vec<Exp>` — `impl Parsable for Vec<Exp>` in `exp.rs` |
| `<param_list>` | `Vec<Param>` — `Parser::parse_param_list` helper |
| `<param_items>` | inline inside `parse_param_list` |
| `<fieldlist>` | `Vec<Field>` (inside `Initializer`) |
