# Nova Bytecode (NVBC) Specification

Version 0.1 — DRAFT

## Overview

NVBC is a stack-based bytecode intermediate representation for Nova.
It is the output of the `nova-run` compiler phase and the input to the
integrated VM.  The format is designed for simple, efficient execution
with minimal decoding overhead.

## Execution Model

- **Stack machine**: all operations consume values from a value stack
  and push results back onto it.
- **Call stack**: frames contain the function index, program counter,
  a base pointer into the value stack (locals + incoming args), and
  the return address.
- **Locals**: each function has a fixed number of local slots accessed
  by index.  Incoming arguments occupy the first N local slots.
- **Globals**: a module-level key–value store.  Host-provided globals
  are injected before execution begins.
- **Types**: types are referenced by index into the module's type list
  (imported from the NVIL bundle's type table).

## Serialised Format (binary)

```
┌─ Module Header ──────────────────────────────────────────┐
│ Magic         : b"NVBC" (4 bytes)                        │
│ Version major : u16 LE                                   │
│ Version minor : u16 LE                                   │
│ Flags         : u32 LE (reserved, 0 for now)             │
├─ Constant Pool ──────────────────────────────────────────┤
│ count         : u32 LE                                   │
│ entries[]     : see §Constant encoding                   │
├─ Global Table ───────────────────────────────────────────┤
│ count         : u32 LE                                   │
│ entries[]     : { name_len: u32, name: [u8] }            │
├─ Type Table ─────────────────────────────────────────────┤
│ count         : u32 LE                                   │
│ entries[]     : { bundle_id: u32, name_len: u32, ... }   │
├─ Function Table ─────────────────────────────────────────│
│ count         : u32 LE                                   │
│ entries[]     : { name_len, name, arg_c, ret_c,          │
│                  local_c, code_len, code[] }             │
└──────────────────────────────────────────────────────────┘
```

### Constant Encoding

Each constant pool entry starts with a tag byte:

| Tag  | Type     | Payload                         |
|------|----------|---------------------------------|
| 0x00 | Nil      | (empty)                         |
| 0x01 | Bool     | `value: u8` (0/1)               |
| 0x02 | Number   | `value: f64 LE`                 |
| 0x03 | String   | `len: u32` `bytes: [u8; len]`   |
| 0x04 | Type     | `type_index: u32`               |

### Code Section Encoding

Each function's `code` array is a flat byte sequence of instructions.
Opcode is a single byte.  Operands follow immediately, with sizes
determined by the opcode.  Multi-byte integers are LE.

## Instruction Set

### Stack Manipulation

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x00   | NIL           | —               | `→ nil`                        |
| 0x01   | TRUE          | —               | `→ true`                       |
| 0x02   | FALSE         | —               | `→ false`                      |
| 0x03   | CONST         | `idx: u32`      | `→ constants[idx]`             |
| 0x04   | I8            | `val: i8`       | `→ i8 as number`               |
| 0x05   | I32           | `val: i32`      | `→ i32 as number`              |
| 0x06   | F64           | `val: f64 LE`   | `→ f64`                        |
| 0x07   | STRING        | `len: u32` `bytes` | `→ String`                   |
| 0x08   | DUP           | —               | `a → a a`                      |
| 0x09   | POP           | —               | `a →`                          |

### Variable Access

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x10   | LOAD          | `idx: u32`      | `→ locals[idx]`                |
| 0x11   | STORE         | `idx: u32`      | `a →` (writes to `locals[idx]`)|
| 0x12   | GLOAD         | `name: str`     | `→ globals[name]`              |
| 0x13   | GSTORE        | `name: str`     | `a →` (writes to `globals[name]`) |

### Function Calls

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x20   | CALL          | `fi: u32` `n: u32` | `arg_n … arg_1 → ret_v …` |
| 0x21   | CALL_HOST     | `hi: u32` `n: u32` | `arg_n … arg_1 → ret_v …` |
| 0x22   | RETURN        | `n: u32`        | `ret_1 … ret_n →` (pops from caller view) |
| 0x23   | CLOSURE       | `fi: u32`       | `→ closure(function_idx)` |

### Tables (future)

| 0x30   | NEWTABLE      | —               | `→ table`                      |
| 0x31   | TGET          | —               | `table key → value`            |
| 0x32   | TSET           | —               | `table key value →`            |

### Arithmetic

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x40   | ADD           | —               | `a b → (a + b)`                |
| 0x41   | SUB           | —               | `a b → (a - b)`                |
| 0x42   | MUL           | —               | `a b → (a * b)`                |
| 0x43   | DIV           | —               | `a b → (a / b)`                |
| 0x44   | MOD           | —               | `a b → (a % b)`                |
| 0x45   | POW           | —               | `a b → (a ^ b)`                |
| 0x46   | NEG           | —               | `a → (-a)`                     |

### Comparison

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x48   | EQ            | —               | `a b → (a == b)`               |
| 0x49   | NEQ           | —               | `a b → (a != b)`               |
| 0x4A   | LT            | —               | `a b → (a < b)`                |
| 0x4B   | LE            | —               | `a b → (a <= b)`               |
| 0x4C   | GT            | —               | `a b → (a > b)`                |
| 0x4D   | GE            | —               | `a b → (a >= b)`               |

### Logical

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x50   | NOT           | —               | `a → not a`                    |
| 0x51   | AND           | —               | `a b → a and b`                |
| 0x52   | OR            | —               | `a b → a or b`                 |
| 0x53   | CONCAT        | —               | `a b → a .. b`                 |

### Bitwise

| Opcode | Mnemonic      | Operands        | Stack effect                   |
|--------|---------------|-----------------|--------------------------------|
| 0x58   | BAND          | —               | `a b → a & b`                  |
| 0x59   | BOR           | —               | `a b → a \| b`                 |
| 0x5A   | BXOR          | —               | `a b → a ^ b`                  |
| 0x5B   | BNOT          | —               | `a → ~a`                       |
| 0x5C   | SHL           | —               | `a b → a << b`                 |
| 0x5D   | SHR           | —               | `a b → a >> b`                 |

### Control Flow

| Opcode | Mnemonic      | Operands           | Effect                         |
|--------|---------------|--------------------|--------------------------------|
| 0x60   | JMP           | `offset: i32`      | pc += offset                   |
| 0x61   | JZ            | `offset: i32`      | pop a; if false pc += offset   |
| 0x62   | JNZ           | `offset: i32`      | pop a; if true pc += offset    |
| 0x63   | LOOP          | `offset: i32`      | decrement loop counter, jump   |

### Misc

| Opcode | Mnemonic      | Operands        | Effect                         |
|--------|---------------|-----------------|--------------------------------|
| 0xF0   | NOP           | —               | no-op                          |
| 0xF1   | BREAKPOINT    | —               | debugger trap                  |
| 0xFF   | HALT          | —               | stop execution                 |
