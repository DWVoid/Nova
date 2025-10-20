using System.Collections.Immutable;
using System.Globalization;
using System.Text;

namespace Nova;

public static partial class Compile
{
    // an abstract expression
    public interface IExpr : Parse.ISyntax;

    // an abstract statement
    public interface IStmt : Parse.ISyntax;

    // syntax representation of a block of statements
    public record Block(ImmutableArray<IStmt> Stmts) : Parse.ISyntax;

    private record EmptyStmt : IStmt;

    private record LabelStmt(Parse.Source.Text Name) : IStmt;

    private record BreakStmt : IStmt;

    private record GotoStmt(Parse.Source.Text Name) : IStmt;

    private record DoStmt(Block Block) : IStmt;

    private record WhileStmt(IExpr Expr, Block Block) : IStmt;

    private record RepeatStmt(IExpr Expr, Block Block) : IStmt;

    private record IfStmt(ImmutableArray<(IExpr, Block)> List, Block? Else) : IStmt;

    private record ForNumStmt(Parse.Source.Text Name, IExpr Init, IExpr Limit, IExpr Step, Block Block) : IStmt;

    private record ForIterStmt(ImmutableArray<Parse.Source.Text> Name, ImmutableArray<IExpr> Expr, Block Block) : IStmt;

    private record FuncBody(ImmutableArray<Parse.Source.Text> Par, Block Block);

    private record FuncStmt(bool Local, bool Self, ImmutableArray<Parse.Source.Text> Path, FuncBody Body) : IStmt;

    private record AttrName(Parse.Source.Text Name, ImmutableArray<Parse.Source.Text> Attrs);

    private record LocalsStmt(ImmutableArray<AttrName> Names, ImmutableArray<IExpr> Expr) : IStmt;

    private record ReturnStmt(ImmutableArray<IExpr> Expr) : IStmt;

    private record AssignStmt(ImmutableArray<IExpr> Left, ImmutableArray<IExpr> Right) : IStmt;

    private record ExprStmt(IExpr Expr) : IStmt;

    private static void BuildSyntaxStmt(Parse.ISyntaxBuilder sbd)
    {
        sbd.Sequence("StmtEmpty")
            .Drop(";")
            .Build(_ => new EmptyStmt());

        sbd.Sequence("StmtLabel")
            .Drop("::")
            .Anchor()
            .KeepRule("PpId")
            .Drop("::")
            .Build(it => new LabelStmt((Parse.Source.Text)it[0]));

        sbd.Sequence("StmtBreak")
            .Drop("break")
            .Build(_ => new BreakStmt());

        sbd.Sequence("StmtGoto")
            .Drop("goto")
            .Anchor()
            .KeepRule("PpId")
            .Build(it => new GotoStmt((Parse.Source.Text)it[0]));

        sbd.Sequence("StmtDo")
            .Drop("do")
            .Anchor()
            .KeepRule("Block")
            .Drop("end")
            .Build(it => new DoStmt((Block)it[0]));

        sbd.Sequence("StmtWhile")
            .Drop("while")
            .Anchor()
            .KeepRule("Expr")
            .Drop("do")
            .KeepRule("Block")
            .Drop("end")
            .Build(it => new WhileStmt((IExpr)it[0], (Block)it[1]));

        sbd.Sequence("StmtRepeat")
            .Drop("repeat")
            .Anchor()
            .KeepRule("Block")
            .Drop("until")
            .KeepRule("Expr")
            .Build(it => new RepeatStmt((IExpr)it[1], (Block)it[0]));

        sbd.Sequence("StmtIf")
            .Drop("if")
            .Anchor()
            .KeepRule("Expr")
            .Drop("then")
            .KeepRule("Block")
            .KeepRule("StmtIfElseIfClause", 0, int.MaxValue)
            .KeepRule("StmtIfElseClause", 0, 1)
            .Drop("end")
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<(IExpr, Block)>();
                b.Add(((IExpr)it[0], (Block)it[1]));
                foreach (var eif in (ImmutableArray<object>)it[2]) b.Add(((IExpr, Block))eif);
                return new IfStmt(b.MoveToImmutable(), (Block?)it[3]);
            });

        sbd.Sequence("StmtIfElseIfClause")
            .Drop("elseif")
            .Anchor()
            .KeepRule("Expr")
            .Drop("then")
            .KeepRule("Block")
            .Build(it => ((IExpr)it[0], (Block)it[1]));

        sbd.Sequence("StmtIfElseClause")
            .Drop("else")
            .Anchor()
            .KeepRule("Block")
            .Build(it => (Block)it[1]);

        sbd.Sequence("StmtForNum")
            .Drop("for")
            .KeepRule("PpId")
            .Drop("=")
            .Anchor()
            .KeepRule("Expr")
            .KeepRule("StmtForNumExp", 1, 2)
            .Drop("do")
            .KeepRule("Block")
            .Drop("end")
            .Build(it =>
            {
                var e2 = (ImmutableArray<IExpr>)it[2];
                return new ForNumStmt(
                    (Parse.Source.Text)it[0],
                    (IExpr)it[1],
                    e2[0],
                    e2.Length > 1 ? e2[1] : ConstIntExpr.One,
                    (Block)it[3]
                );
            });

        sbd.Sequence("StmtForNumExp")
            .Drop(",")
            .Anchor()
            .KeepRule("Expr")
            .Build(it => it[0]);

        sbd.Sequence("NameListNext")
            .Drop(",")
            .KeepRule("PpId")
            .Build(it => it[0]);

        sbd.Sequence("NameList")
            .KeepRule("PpId")
            .Anchor()
            .KeepRule("NameListNext", 0, int.MaxValue)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Parse.Source.Text>();
                b.Add((Parse.Source.Text)it[0]);
                b.AddRange((ImmutableArray<Parse.Source.Text>)it[1]);
                return b.MoveToImmutable();
            });

        sbd.Sequence("ExprListNext")
            .Drop(",")
            .KeepRule("Expr")
            .Build(it => it[0]);

        sbd.Sequence("ExprList")
            .KeepRule("Expr")
            .Anchor()
            .KeepRule("ExprListNext", 0, int.MaxValue)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<IExpr>();
                b.Add((IExpr)it[0]);
                b.AddRange((ImmutableArray<IExpr>)it[1]);
                return b.MoveToImmutable();
            });

        sbd.Sequence("StmtForIter")
            .Drop("for")
            .KeepRule("NameList")
            .Drop("in")
            .Anchor()
            .KeepRule("ExprList")
            .Drop("do")
            .KeepRule("Block")
            .Drop("end")
            .Build(it => new ForIterStmt(
                (ImmutableArray<Parse.Source.Text>)it[0],
                (ImmutableArray<IExpr>)it[1],
                (Block)it[2]
            ));

        sbd.Sequence("ParListVarArgA")
            .Drop(",")
            .Keep("...")
            .Build(it => it[0]);

        sbd.Sequence("ParListVarArgB")
            .Keep("...")
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Parse.Source.Text>();
                b.Add((Parse.Source.Text)it[0]);
                return b.MoveToImmutable();
            });

        sbd.Sequence("ParListNamed")
            .KeepRule("NameList")
            .KeepRule("ParListVarArgA", 0, 1)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Parse.Source.Text>();
                b.AddRange((ImmutableArray<Parse.Source.Text>)it[0]);
                var o = (ImmutableArray<ImmutableArray<Parse.Source.Text>>)it[1];
                if (!o.IsEmpty) b.AddRange(o[0]);
                return b;
            });

        sbd.Selection("ParList")
            .BranchRule("ParListNamed")
            .BranchRule("ParListVarArgB");

        sbd.Sequence("FuncBody")
            .Drop("(")
            .Anchor()
            .KeepRule("ParList")
            .Drop(")")
            .KeepRule("Block")
            .Drop("end")
            .Build(it => new FuncBody((ImmutableArray<Parse.Source.Text>)it[0], (Block)it[1]));

        sbd.Sequence("FuncNameDotElement").Drop(".").Keep("PpId").Build(it => it[0]);

        sbd.Sequence("FuncNameSelfElement").Drop(":").Keep("PpId").Build(it => it[0]);

        sbd.Sequence("StmtFunction")
            .Drop("function")
            .KeepRule("PpId")
            .Anchor() // cannot anchor before name for ExprFunctionDef
            .KeepRule("FuncNameDotElement", 0, int.MaxValue)
            .KeepRule("FuncNameSelfElement", 0, 1)
            .KeepRule("FuncBody")
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Parse.Source.Text>();
                b.AddRange((ImmutableArray<Parse.Source.Text>)it[1]);
                var o = (ImmutableArray<Parse.Source.Text>)it[2];
                b.AddRange(o);
                return new FuncStmt(false, !o.IsEmpty, b.MoveToImmutable(), (FuncBody)it[3]);
            });

        sbd.Sequence("StmtLocalFunction")
            .Drop("local")
            .Drop("function")
            .Anchor()
            .KeepRule("PpId")
            .KeepRule("FuncBody")
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Parse.Source.Text>();
                b.Add((Parse.Source.Text)it[0]);
                return new FuncStmt(true, false, b.MoveToImmutable(), (FuncBody)it[1]);
            });

        sbd.Sequence("LocalsInit")
            .Drop("=")
            .Keep("ExprList")
            .Build(it => it[0]);

        sbd.Sequence("Attr")
            .Drop("<")
            .Anchor()
            .KeepRule("PpId")
            .Drop(">")
            .Build(it => it[0]);

        sbd.Sequence("AttrName")
            .KeepRule("PpId")
            .KeepRule("Attr", 0, int.MaxValue)
            .Build(it => new AttrName(
                (Parse.Source.Text)it[0],
                (ImmutableArray<Parse.Source.Text>)it[1]
            ));

        sbd.Sequence("AttrNameListNext")
            .Drop(",")
            .KeepRule("AttrName")
            .Build(it => it[0]);

        sbd.Sequence("AttrNameList")
            .KeepRule("AttrName")
            .Anchor()
            .KeepRule("AttrNameListNext", 0, int.MaxValue)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<AttrName>();
                b.Add((AttrName)it[0]);
                b.AddRange((ImmutableArray<AttrName>)it[1]);
                return b.MoveToImmutable();
            });

        sbd.Sequence("StmtLocals")
            .Drop("local")
            .KeepRule("AttrNameList")
            .KeepRule("LocalsInit", 0, int.MinValue)
            .Build(it =>
            {
                var o = (ImmutableArray<ImmutableArray<IExpr>>)it[1];
                return new LocalsStmt(
                    (ImmutableArray<AttrName>)it[0],
                    o.IsEmpty ? ImmutableArray<IExpr>.Empty : o[0]
                );
            });

        sbd.Sequence("StmtReturn")
            .Drop("return")
            .Anchor()
            .KeepRule("ExprList", 0, 1)
            .Build(it =>
            {
                var o = (ImmutableArray<ImmutableArray<IExpr>>)it[0];
                return new ReturnStmt(o.IsEmpty ? ImmutableArray<IExpr>.Empty : o[0]);
            });

        sbd.Sequence("StmtAssign")
            .KeepRule("ExprList")
            .Anchor()
            .Drop("=")
            .KeepRule("ExprList")
            .Build(it => new AssignStmt(
                (ImmutableArray<IExpr>)it[0],
                (ImmutableArray<IExpr>)it[1]
            ));

        sbd.Sequence("StmtExpr")
            .KeepRule("Expr")
            .Build(it => new ExprStmt((IExpr)it[0]));

        sbd.Selection("Stmt")
            .BranchRule("StmtEmpty")
            .BranchRule("StmtLabel")
            .BranchRule("StmtBreak")
            .BranchRule("StmtGoto")
            .BranchRule("StmtDo")
            .BranchRule("StmtWhile")
            .BranchRule("StmtRepeat")
            .BranchRule("StmtForNum")
            .BranchRule("StmtForIter")
            .BranchRule("StmtFunction")
            .BranchRule("StmtLocalFunction")
            .BranchRule("StmtLocals")
            .BranchRule("StmtReturn")
            .BranchRule("StmtAssign")
            .BranchRule("StmtExpr");

        sbd.Sequence("Block")
            .KeepRule("Stmt", 0, int.MaxValue)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<IStmt>();
                foreach (var o in it) b.Add((IStmt)o);
                return new Block(b.MoveToImmutable());
            });
    }

    private record UnaryOpExpr(IExpr Right, Parse.Source.Text Op) : IExpr;

    private record BinaryOpExpr(IExpr Left, IExpr Right, Parse.Source.Text Op) : IExpr;

    private static void BuildExprLvl(Parse.ISyntaxBuilder sbd, int lvl)
    {
        sbd.Selection($"ExprL{lvl}")
            .BranchRule($"ExprDoL{lvl}")
            .BranchRule($"ExprL{lvl + 1}");
    }

    private static void BuildExprOpLvl(Parse.ISyntaxBuilder sbd, int lvl, params ReadOnlySpan<string> ops)
    {
        var opb = sbd.Selection($"ExprOpL{lvl}");
        foreach (var op in ops) opb.Branch(op);
        BuildExprLvl(sbd, lvl);
    }

    private static void BuildUnaryExprLvl(Parse.ISyntaxBuilder sbd, int lvl, params ReadOnlySpan<string> ops)
    {
        sbd.Sequence($"ExprDoL{lvl}")
            .KeepRule($"ExprOpL{lvl}")
            .Anchor()
            .KeepRule($"ExprL{lvl}")
            .Build(it => new UnaryOpExpr((IExpr)it[1], (Parse.Source.Text)it[0]));
        BuildExprOpLvl(sbd, lvl, ops);
    }

    private static void BuildBinaryExprLvl(Parse.ISyntaxBuilder sbd, int lvl, params ReadOnlySpan<string> ops)
    {
        sbd.Sequence($"ExprDoL{lvl}")
            .KeepRule($"ExprL{lvl + 1}")
            .KeepRule($"ExprOpL{lvl}")
            .Anchor()
            .KeepRule($"ExprL{lvl}")
            .Build(it => new BinaryOpExpr((IExpr)it[0], (IExpr)it[2], (Parse.Source.Text)it[1]));
        BuildExprOpLvl(sbd, lvl, ops);
    }

    private record IdExpr(Parse.Source.Text Id) : IExpr;

    private record ConstNilExpr : IExpr
    {
        public static readonly ConstNilExpr Nil = new();
    }

    private record ConstBoolExpr(bool Bool) : IExpr
    {
        public static readonly ConstBoolExpr True = new(true);
        public static readonly ConstBoolExpr False = new(false);
    }

    private record ConstIntExpr(long Num) : IExpr
    {
        public static readonly ConstIntExpr One = new(1);
    }

    private record ConstRealExpr(double Num) : IExpr;

    private record ConstStrExpr(string Str) : IExpr;

    private record FunctionDefExpr(FuncBody Body) : IExpr;

    private record Field(IExpr Left, IExpr? Right);

    private record TableConstructExpr(ImmutableArray<Field> Fields) : IExpr;

    private interface IPrefixOperation;

    private record PrefixIndexOperation(IExpr Index) : IPrefixOperation;

    private record PrefixAccessOperation(Parse.Source.Text Access) : IPrefixOperation;

    private record PrefixInvokeOperation(ImmutableArray<IExpr> Args) : IPrefixOperation;

    private record PrefixExpr(IExpr Source, ImmutableArray<IPrefixOperation> Ops) : IExpr;

    private record VarExpandExpr : IExpr
    {
        public static readonly VarExpandExpr Expr = new();
    }

    private static void BuildSyntaxExpr(Parse.ISyntaxBuilder sbd)
    {
        // prefix expr has the highest priority
        sbd.Selection("Expr")
            .BranchRule("PrefixExpr")
            .BranchRule("ExprL1");

        BuildBinaryExprLvl(sbd, 1, "or");
        BuildBinaryExprLvl(sbd, 2, "and");
        BuildBinaryExprLvl(sbd, 3, "<", ">", "<=", ">=", "~=", "==");
        BuildBinaryExprLvl(sbd, 4, "|");
        BuildBinaryExprLvl(sbd, 5, "~");
        BuildBinaryExprLvl(sbd, 6, "&");
        BuildBinaryExprLvl(sbd, 7, "<<", ">>");
        BuildBinaryExprLvl(sbd, 8, "..");
        BuildBinaryExprLvl(sbd, 9, "+", "-");
        BuildBinaryExprLvl(sbd, 10, "*", "/", "//", "%");
        BuildUnaryExprLvl(sbd, 11, "not", "#", "-", "~");
        BuildBinaryExprLvl(sbd, 12, "^");

        // this special rule does not produce a single value
        sbd.Selection("ExprL13").BranchRule("ExprVarExpand");

        sbd.Sequence("ExprVarExpand").Drop("...").Build(_ => VarExpandExpr.Expr);

        sbd.Selection("PrefixChainElement")
            .BranchRule("PrefixChainElementIndex")
            .BranchRule("PrefixChainElementAccess")
            .BranchRule("PrefixChainElementInvoke");

        sbd.Sequence("PrefixChainElementIndex")
            .Drop("[")
            .Anchor()
            .KeepRule("Expr")
            .Drop("]")
            .Build(it => new PrefixIndexOperation((IExpr)it[0]));

        sbd.Sequence("PrefixChainElementAccess")
            .Drop(".")
            .KeepRule("PpId")
            .Build(it => new PrefixAccessOperation((Parse.Source.Text)it[0]));

        sbd.Selection("PrefixChainElementInvoke")
            .BranchRule("PrefixChainElementInvokeByTable")
            .BranchRule("PrefixChainElementInvokeByLiteral")
            .BranchRule("PrefixChainElementInvokeByBracket");

        sbd.Sequence("PrefixChainElementInvokeByTable")
            .KeepRule("ExprTableConstruct")
            .Build(it => new PrefixInvokeOperation([(IExpr)it[0]]));

        sbd.Sequence("PrefixChainElementInvokeByLiteral")
            .KeepRule("ExprLiteral")
            .Build(it => new PrefixInvokeOperation([(IExpr)it[0]]));

        sbd.Sequence("PrefixChainElementInvokeByBracket")
            .Drop("(")
            .Anchor()
            .KeepRule("ExprList", 0, 1)
            .Drop(")")
            .Build(it =>
            {
                var o = (ImmutableArray<ImmutableArray<IExpr>>)it[0];
                return new PrefixInvokeOperation(o.IsEmpty ? ImmutableArray<IExpr>.Empty : o[0]);
            });

        // these produces a single value
        sbd.Selection("PrefixChainInitiator")
            .BranchRule("ExprId")
            .BranchRule("ExprNil")
            .BranchRule("ExprTrue")
            .BranchRule("ExprFalse")
            .BranchRule("ExprNumeral")
            .BranchRule("ExprLiteral")
            .BranchRule("ExprFunctionDef")
            .BranchRule("ExprTableConstruct");

        sbd.Sequence("PrefixExpr")
            .KeepRule("PrefixChainInitiator")
            .KeepRule("PrefixChainElement", 0, int.MaxValue)
            .Build(it => new PrefixExpr((IExpr)it[0], (ImmutableArray<IPrefixOperation>)it[1]));

        sbd.Sequence("ExprId")
            .KeepRule("PpId")
            .Build(it => new IdExpr((Parse.Source.Text)it[0]));

        sbd.Sequence("ExprNil").Drop("nil").Build(_ => ConstNilExpr.Nil);

        sbd.Sequence("ExprTrue").Drop("true").Build(_ => ConstBoolExpr.True);

        sbd.Sequence("ExprFalse").Drop("false").Build(_ => ConstBoolExpr.False);

        sbd.Sequence("ExprNumeral")
            .KeepRule("PpNum")
            .Build(it => PpNumToConst((Parse.Source.Text)it[0]));

        sbd.Sequence("ExprLiteral")
            .KeepRule("PpStr")
            .Build(it => PpStrToConst((Parse.Source.Text)it[0]));

        sbd.Sequence("ExprFunctionDef")
            .Drop("function")
            .KeepRule("FuncBody")
            .Build(it => new FunctionDefExpr((FuncBody)it[0]));

        sbd.Sequence("ExprTableConstruct")
            .Drop("{")
            .Anchor()
            .KeepRule("FieldList", 0, 1)
            .Drop("}")
            .Build(it =>
            {
                var o = (ImmutableArray<ImmutableArray<Field>>)it[0];
                return new TableConstructExpr(o.IsEmpty ? ImmutableArray<Field>.Empty : o[0]);
            });

        sbd.Sequence("FieldList")
            .KeepRule("Field")
            .KeepRule("FieldWithSep", 0, int.MaxValue)
            .DropRule("FieldSep", 0, 1)
            .Build(it =>
            {
                var b = ImmutableArray.CreateBuilder<Field>();
                b.Add((Field)it[0]);
                b.AddRange((ImmutableArray<Field>)it[1]);
                return new TableConstructExpr(b.MoveToImmutable());
            });

        sbd.Sequence("FieldWithSep")
            .DropRule("FieldSep")
            .KeepRule("Field")
            .Build(it => it[0]);

        sbd.Selection("Field")
            .BranchRule("FieldIndex")
            .BranchRule("FieldAssign")
            .BranchRule("FieldExpr");

        sbd.Sequence("FieldIndex")
            .Drop("[")
            .Anchor()
            .KeepRule("Expr")
            .Drop("]")
            .Drop("=")
            .KeepRule("Expr")
            .Build(it => new Field((IExpr)it[0], (IExpr)it[1]));

        sbd.Sequence("FieldAssign")
            .KeepRule("ExprId")
            .Drop("=")
            .Anchor()
            .KeepRule("Expr")
            .Build(it => new Field((IExpr)it[0], (IExpr)it[1]));

        sbd.Sequence("FieldExpr")
            .KeepRule("Expr")
            .Build(it => new Field((IExpr)it[0], null));

        sbd.Selection("FieldSep").Branch(",").Branch(";");
    }

    private static IExpr PpNumToConst(Parse.Source.Text text)
    {
        var s = text.Chars;
        var r = 0L;
        var m = 0;
        var real = false;
        var frac = false;
        var flip = false;
        if (s.StartsWith("0x") || s.StartsWith("0X"))
        {
            var span = s[2..];
            while (!span.IsEmpty)
            {
                switch (span[0])
                {
                    case >= '0' and <= '9':
                        r = (r << 4) | (r - '0');
                        break;
                    case >= 'a' and <= 'f':
                        r = (r << 4) | (r - 'a' + 10);
                        break;
                    case >= 'A' and <= 'F':
                        r = (r << 4) | (r - 'A' + 10);
                        break;
                    case '.':
                        real = true;
                        continue;
                    case 'P' or 'p':
                        frac = true;
                        break;
                    default:
                        throw new Exception("BAD_BINARY_NUMBER");
                }

                span = span[1..];
                if (frac) break;
                if (real) m -= 4;
            }

            if (frac)
            {
                if (span[0] is '-')
                {
                    flip = true;
                    span = span[1..];
                }

                if (span[0] is '+') span = span[1..];
                if (span.IsEmpty || !int.TryParse(span, NumberStyles.None, null, out var dec))
                    throw new Exception("BAD_BINARY_FRACTIONAL");
                m += flip ? -dec : dec;
            }

            return m != 0
                ? new ConstRealExpr(r * Math.Pow(2, m))
                : new ConstIntExpr(r);
        }
        else
        {
            var span = s;
            while (!span.IsEmpty)
            {
                switch (span[0])
                {
                    case >= '0' and <= '9':
                        r = (r << 4) | (r - '0');
                        break;
                    case '.':
                        real = true;
                        continue;
                    case 'E' or 'e':
                        frac = true;
                        break;
                    default:
                        throw new Exception("BAD_DECIMAL_NUMBER");
                }

                span = span[1..];
                if (frac) break;
                if (real) --m;
            }

            if (frac)
            {
                if (span[0] is '-')
                {
                    flip = true;
                    span = span[1..];
                }

                if (span[0] is '+') span = span[1..];
                if (span.IsEmpty || !int.TryParse(span, NumberStyles.None, null, out var dec))
                    throw new Exception("BAD_DECIMAL_FRACTIONAL");
                m += flip ? -dec : dec;
            }

            return m != 0
                ? new ConstRealExpr(r * Math.Pow(10, m))
                : new ConstIntExpr(r);
        }
    }

    private static ConstStrExpr PpStrToConst(Parse.Source.Text text)
    {
        var s = text.Chars;
        if (s[0] is '\'' or '"') return new ConstStrExpr(PpStrHandleEscapes(s[1..^1]));
        var trim = s[1..].IndexOf('[') + 2;
        s = s[trim..^trim];
        return new ConstStrExpr(PpStrStandardizeLn(s));
    }

    private static string PpStrHandleEscapes(ReadOnlySpan<char> s)
    {
        var sb = new StringBuilder(s.Length);
        Span<char> rb = stackalloc char[3];
        while (!s.IsEmpty)
        {
            if (s[0] is '\\' && s.Length >= 2)
            {
                switch (s[1])
                {
                    case 'a':
                        sb.Append('\a');
                        goto case '\0';
                    case 'b':
                        sb.Append('\b');
                        goto case '\0';
                    case 'f':
                        sb.Append('\f');
                        goto case '\0';
                    case 'n':
                        sb.Append('\n');
                        goto case '\0';
                    case 'r':
                        sb.Append('\r');
                        goto case '\0';
                    case 't':
                        sb.Append('\t');
                        goto case '\0';
                    case 'v':
                        sb.Append('\v');
                        goto case '\0';
                    case '\\':
                        sb.Append('\\');
                        goto case '\0';
                    case '"':
                        sb.Append('"');
                        goto case '\0';
                    case '\'':
                        sb.Append('\'');
                        goto case '\0';
                    case '\r' or '\n':
                        if (s[1..].StartsWith("\r\n") || s[1..].StartsWith("\n\r"))
                            s = s[3..];
                        else
                            s = s[2..];
                        sb.Append('\n');
                        break;
                    case 'z':
                        s = s[2..];
                        while (!s.IsEmpty && s[0] is ' ' or '\t' or '\v' or '\f' or '\r' or '\n') s = s[1..];
                        break;
                    case 'x':
                    {
                        if (s.Length < 4 || !int.TryParse(s[2..4], NumberStyles.AllowHexSpecifier, null, out var i))
                            throw new Exception("INVALID_STR_ESC_X");
                        sb.Append((char)i);
                        s = s[4..];
                        break;
                    }
                    case 'u':
                    {
                        if (s.Length < 3 || s[2] != '{')
                            throw new Exception("INVALID_STR_ESC_U_H");

                        for (var i = 3; i <= 11 && s.Length > i; ++i)
                        {
                            if (s[i] != '}') continue;
                            if (long.TryParse(s[3..i], NumberStyles.AllowHexSpecifier, null, out var v))
                                if (v < int.MaxValue)
                                {
                                    s = s[(i + 1)..];
                                    new Rune((int)v).TryEncodeToUtf16(rb, out var l);
                                    sb.Append(rb[..l]);
                                }

                            throw new Exception("INVALID_STR_ESC_U_M");
                        }

                        throw new Exception("INVALID_STR_ESC_U_T");
                        break;
                    }
                    case >= '0' and <= '9':
                    {
                        var l = s.Length >= 3 && s[2] is >= '0' and <= '9'
                            ? s.Length >= 4 && s[3] is >= '0' and <= '9'
                                ? 4
                                : 3
                            : 2;
                        sb.Append((char)int.Parse(s[1..l], NumberStyles.None));
                        s = s[l..];
                        break;
                    }
                    case '\0':
                        s = s[2..];
                        break;
                }
            }
            else sb.Append(s[0]);
        }

        return sb.ToString();
    }

    private static string PpStrStandardizeLn(ReadOnlySpan<char> s)
    {
        var init = false;
        var sb = new StringBuilder(s.Length);
        while (!s.IsEmpty)
        {
            if (s.StartsWith("\r\n") || s.StartsWith("\n\r"))
            {
                s = s[2..];
                if (init) sb.Append('\n');
            }
            else if (s[0] is '\n' or '\r')
            {
                s = s[1..];
                if (init) sb.Append('\n');
            }
            else
            {
                s = s[1..];
                sb.Append(s[0]);
            }

            if (!init) init = true;
        }

        return sb.ToString();
    }

    public static void BuildSyntax()
    {
        var sbd = Parse.CreateSyntaxBuilder();
        sbd.FromScan<IdScan>("PpId");
        sbd.FromScan<WsScan>("PpWs");
        sbd.FromScan<NumScan>("PpNum");
        sbd.FromScan<StrScan>("PpStr");
        BuildSyntaxStmt(sbd);
        BuildSyntaxExpr(sbd);
        sbd.Build();
    }
}