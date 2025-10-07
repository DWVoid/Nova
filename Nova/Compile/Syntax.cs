using System.Collections.Immutable;

namespace Nova;

public static partial class Compile
{
    // a syntax element
    public interface ISyntax;


    // an abstract expression
    public interface IExpr : ISyntax
    {
    }

    // an abstract statement
    public interface IStmt : ISyntax
    {
    }

    // syntax representation of a block of statements
    public class Block : ISyntax
    {
        public required ImmutableArray<IStmt> Stmts { get; init; }
    }

    // syntax representation of a translation (a source file, snippet, etc.)
    public class Unit : ISyntax
    {
    }

    private static Block SynBlock(ref SourceContext source, string delim = "")
    {
        var stmts = ImmutableArray.CreateBuilder<IStmt>();
        while (TryStmt(ref source, out var stmt)) stmts.Add(stmt);
        return new Block { Stmts = stmts.MoveToImmutable() };
    }

    private static bool TryStmt(ref SourceContext source, out IStmt syn)
    {
        SkipWs(ref source);
        // first handle all cases that start with reserved keyword
        var rewind = source.Text;
        if (TryGetId(ref source.Text, out var tok))
        {
            switch (tok.Chars.ToString())
            {
                case "break":
                    syn = new BreakStmt();
                    return true;
                case "goto":
                    syn = StmtGoto(ref source);
                    return true;
                case "do":
                    syn = StmtDo(ref source);
                    return true;
                case "while":
                    syn = StmtWhile(ref source);
                    return true;
                case "repeat":
                    syn = StmtRepeat(ref source);
                    return true;
                case "if":
                    syn = StmtIf(ref source);
                    return true;
                case "for":
                    syn = StmtFor(ref source);
                    return true;
                case "function":
                    syn = StmtFunc(ref source, false);
                    return true;
                case "local":
                    SkipWs(ref source);
                    if (source.Text.Length >= 8 && source.Text.Chars[..8] is "function")
                    {
                        ExpectLiteral(ref source, "function");
                        syn = StmtFunc(ref source, true);
                    }
                    else syn = StmtLocals(ref source);

                    return true;
                case "return":
                    syn = StmtReturn(ref source);
                    return true;
            }

            source.Text = rewind;
        }

        // then we look for ';' and labels
        if (source.Text.Length >= 1 && source.Text.Chars[0] == ';')
        {
            ExpectSymbolic(ref source, ";");
            syn = new EmptyStmt();
            return true;
        }

        if (source.Text.Length >= 2 && source.Text.Chars[..2] is "::")
        {
            source.Text = source.Text[2..];
            var text = GetId(ref source).ToSourceText(source.Lines);
            SkipWs(ref source);
            if (source.Text.Length < 2 || source.Text.Chars[..2] is not "::")
                throw source.UnexpectedToken("'::'");
            source.Text = source.Text[2..];
            syn = new LabelStmt(text);
            return true;
        }

        // look for discard stmt or assignment.
        // both starts with prefix expr, so if multiple expr is found seperated with comma then an assigment is found
        if (TrySynExpr(ref source, out var expr))
        {
            SkipWs(ref source);
            if (source.Text.Length >= 1)
            {
                var l = ImmutableArray.CreateBuilder<IExpr>();
                var r = ImmutableArray.CreateBuilder<IExpr>();
                Next:
                switch (source.Text.Chars[0])
                {
                    case ',':
                    {
                        l.Add(expr);
                        NextLeft:
                        ExpectSymbolic(ref source, ",");
                        l.Add(SynExpr(ref source));
                        SkipWs(ref source);
                        if (source.Text.Length == 0 || source.Text.Chars[0] != ',') goto Next;
                        goto NextLeft;
                    }
                    case '=':
                    {
                        ExpectSymbolic(ref source, "=");
                        NextRight:
                        r.Add(SynExpr(ref source));
                        SkipWs(ref source);
                        if (source.Text.Length == 0 || source.Text.Chars[0] != ',')
                        {
                            syn = new AssignStmt(l.MoveToImmutable(), r.MoveToImmutable());
                            return true;
                        }
                        ExpectSymbolic(ref source, ",");
                        goto NextRight;
                    }
                }
            }

            syn = new DiscardStmt(expr);
            return true;
        }

        syn = null!;
        return false;
    }

    private record EmptyStmt : IStmt;

    private record BreakStmt : IStmt;

    private record DiscardStmt(IExpr Expr) : IStmt;
    
    private record AssignStmt(ImmutableArray<IExpr> Left, ImmutableArray<IExpr> Right): IStmt;

    private record LabelStmt(SourceText Name) : IStmt;

    private record GotoStmt(SourceText Name) : IStmt;

    private static GotoStmt StmtGoto(scoped ref SourceContext source)
    {
        return new GotoStmt(GetId(ref source).ToSourceText(source.Lines));
    }

    private record DoStmt(Block Block) : IStmt;

    private static int ExpectLiteral(ref SourceContext source, params ReadOnlySpan<string> literal)
    {
        SkipWs(ref source);
        if (TryGetId(ref source.Text, out var tok))
            for (var i = 0; i < literal.Length; ++i)
                if (tok.Chars.SequenceEqual(literal[i]))
                    return i;
        throw source.UnexpectedToken($"'{string.Join('|', literal)}'");
    }

    private static int ExpectSymbolic(ref SourceContext source, params ReadOnlySpan<string> literal)
    {
        SkipWs(ref source);
        if (TryGetSym(ref source.Text, out var tok))
            for (var i = 0; i < literal.Length; ++i)
                if (tok.Chars.SequenceEqual(literal[i]))
                    return i;
        throw source.UnexpectedToken($"'{string.Join('|', literal)}'");
    }

    private static DoStmt StmtDo(ref SourceContext source)
    {
        var block = SynBlock(ref source);
        ExpectLiteral(ref source, "end");
        return new DoStmt(block);
    }

    private record WhileStmt(IExpr Expr, Block Block) : IStmt;

    private static WhileStmt StmtWhile(ref SourceContext source)
    {
        var expr = SynExpr(ref source);
        ExpectLiteral(ref source, "do");
        var block = SynBlock(ref source);
        ExpectLiteral(ref source, "end");
        return new WhileStmt(expr, block);
    }

    private record RepeatStmt(IExpr Expr, Block Block) : IStmt;

    private static RepeatStmt StmtRepeat(ref SourceContext source)
    {
        var block = SynBlock(ref source);
        ExpectLiteral(ref source, "until");
        return new RepeatStmt(SynExpr(ref source), block);
    }

    private record IfStmt(ImmutableArray<(IExpr, Block)> List, Block? Else) : IStmt;

    private static IfStmt StmtIf(ref SourceContext source)
    {
        var list = ImmutableArray.CreateBuilder<(IExpr, Block)>();
        Next:
        var expr = SynExpr(ref source);
        ExpectLiteral(ref source, "then");
        var block = SynBlock(ref source);
        list.Add((expr, block));
        switch (ExpectLiteral(ref source, "elseif", "else", "end"))
        {
            case 0:
                goto Next;
            case 1:
                block = SynBlock(ref source);
                ExpectLiteral(ref source, "end");
                return new IfStmt(list.MoveToImmutable(), block);
            case 2:
                return new IfStmt(list.MoveToImmutable(), null);
            default:
                throw new ArgumentOutOfRangeException(nameof(source));
        }
    }

    private record ForNumStmt(SourceText Name, IExpr Init, IExpr Limit, IExpr Step, Block Block) : IStmt;

    private record ForIterStmt(ImmutableArray<SourceText> Name, ImmutableArray<IExpr> Expr, Block Block) : IStmt;

    private static IStmt StmtFor(ref SourceContext source)
    {
        var name = GetId(ref source).ToSourceText(source.Lines);
        var names = ImmutableArray.CreateBuilder<SourceText>();
        SkipWs(ref source);
        // ReSharper disable once InvertIf
        if (source.Text.Length == 0)
        {
            switch (source.Text.Chars[0])
            {
                case '=':
                {
                    ExpectSymbolic(ref source, "=");
                    var init = SynExpr(ref source);
                    ExpectSymbolic(ref source, ",");
                    var limit = SynExpr(ref source);
                    SkipWs(ref source);
                    if (source.Text.Length == 0)
                        throw source.UnexpectedToken("',|=|in'");
                    IExpr step = ConstIntExpr.One;
                    if (source.Text.Chars[0] == ',')
                    {
                        ExpectSymbolic(ref source, ",");
                        step = SynExpr(ref source);
                    }

                    ExpectLiteral(ref source, "do");
                    var block = SynBlock(ref source);
                    ExpectLiteral(ref source, "end");
                    return new ForNumStmt(name, init, limit, step, block);
                }
                case ',':
                    ExpectSymbolic(ref source, ",");
                    names.Add(name);
                    name = GetId(ref source).ToSourceText(source.Lines);
                    SkipWs(ref source);
                    // ReSharper disable once InvertIf
                    if (source.Text.Length != 0)
                    {
                        if (source.Text.Chars[0] == ',') goto case ',';
                        if (source.Text.Chars[0] == 'i') goto case 'i';
                    }

                    throw source.UnexpectedToken("',|in'");
                case 'i':
                {
                    names.Add(name);
                    ExpectLiteral(ref source, "in");
                    var expr = ImmutableArray.CreateBuilder<IExpr>();
                    Next:
                    expr.Add(SynExpr(ref source));
                    SkipWs(ref source);
                    // ReSharper disable once InvertIf
                    if (source.Text.Length != 0)
                    {
                        if (source.Text.Chars[0] == ',')
                        {
                            ExpectSymbolic(ref source, ",");
                            goto Next;
                        }

                        if (source.Text.Chars[0] == 'd') goto Body;
                    }

                    throw source.UnexpectedToken("',|do'");
                    Body:
                    ExpectLiteral(ref source, "do");
                    var block = SynBlock(ref source);
                    ExpectLiteral(ref source, "end");
                    return new ForIterStmt(names.MoveToImmutable(), expr.MoveToImmutable(), block);
                }
            }
        }

        throw source.UnexpectedToken("',|=|in'");
    }

    private record FuncBody(ImmutableArray<SourceText> Par, Block Block);

    private record FuncStmt(bool Local, bool Self, ImmutableArray<SourceText> Path, FuncBody Body) : IStmt;

    private static FuncBody SynFuncBody(ref SourceContext source)
    {
        ExpectSymbolic(ref source, "(");
        var par = ImmutableArray.CreateBuilder<SourceText>();
        Next:
        SkipWs(ref source);
        // ReSharper disable once InvertIf
        if (source.Text.Length != 0)
        {
            switch (source.Text.Chars[0])
            {
                case ',':
                    ExpectSymbolic(ref source, ",");
                    goto default;
                case ')':
                    ExpectSymbolic(ref source, ")");
                    goto Body;
                case '.':
                    if (source.Text.Length >= 3 && source.Text.Chars[..3] is "...")
                    {
                        var sym = source.Text[..3];
                        source.Text = source.Text[3..];
                        par.Add(sym.ToSourceText(source.Lines));
                        goto case ')';
                    }

                    throw source.UnexpectedToken("'...'");
                default:
                    par.Add(GetId(ref source).ToSourceText(source.Lines));
                    goto Next;
            }
        }

        throw source.UnexpectedToken("',|)|...|<identifier>'");
        Body:
        var block = SynBlock(ref source);
        ExpectLiteral(ref source, "end");
        return new FuncBody(par.ToImmutable(), block);
    }

    private static FuncStmt StmtFunc(ref SourceContext source, bool local)
    {
        var self = false;
        var names = ImmutableArray.CreateBuilder<SourceText>();
        Next:
        names.Add(GetId(ref source).ToSourceText(source.Lines));
        SkipWs(ref source);
        // ReSharper disable once InvertIf
        if (source.Text.Length != 0)
        {
            switch (source.Text.Chars[0])
            {
                case '.':
                    ExpectSymbolic(ref source, ".");
                    goto Next;
                case ':':
                    self = true;
                    ExpectSymbolic(ref source, ":");
                    goto Next;
                case '(':
                    return new FuncStmt(local, self, names.MoveToImmutable(), SynFuncBody(ref source));
            }
        }

        throw source.UnexpectedToken("'.|:|('");
    }

    private record AttrName(SourceText Name, ImmutableArray<SourceText> Attrs);

    private record LocalsStmt(ImmutableArray<AttrName> Names, ImmutableArray<IExpr> Expr) : IStmt;

    private static LocalsStmt StmtLocals(ref SourceContext source)
    {
        var names = ImmutableArray.CreateBuilder<AttrName>();
        var expr = ImmutableArray.CreateBuilder<IExpr>();
        Name:
        var name = GetId(ref source).ToSourceText(source.Lines);
        // we cannot consume here, so peek from text
        SkipWs(ref source);
        Next:
        // ReSharper disable once InvertIf
        if (source.Text.Length != 0)
        {
            switch (source.Text.Chars[0])
            {
                case '<':
                {
                    var attrs = ImmutableArray.CreateBuilder<SourceText>();
                    NextAttr:
                    ExpectSymbolic(ref source, "<");
                    attrs.Add(GetId(ref source).ToSourceText(source.Lines));
                    ExpectSymbolic(ref source, ">");
                    SkipWs(ref source);
                    if (source.Text.Length > 0 && source.Text.Chars[0] == '<') goto NextAttr;
                    names.Add(new AttrName(name, attrs.MoveToImmutable()));
                    goto Next;
                }
                case ',':
                    ExpectSymbolic(ref source, ",");
                    goto Name;
                case '=':
                {
                    ExpectSymbolic(ref source, "=");
                    NextExpr:
                    expr.Add(SynExpr(ref source));
                    SkipWs(ref source);
                    if (source.Text.Length == 0 || source.Text.Chars[0] != ',') break;
                    ExpectSymbolic(ref source, ",");
                    goto NextExpr;
                }
            }
        }

        return new LocalsStmt(names.ToImmutable(), expr.ToImmutable());
    }

    private record ReturnStmt(ImmutableArray<IExpr> Expr) : IStmt;

    private static ReturnStmt StmtReturn(ref SourceContext source)
    {
        // this is one of two cases where expr is optional
        var list = ImmutableArray.CreateBuilder<IExpr>();
        if (TrySynExpr(ref source, out var expr)) goto Push;
        Next:
        expr = SynExpr(ref source);
        Push:
        list.Add(expr);
        SkipWs(ref source);
        if (source.Text.Length == 0 || source.Text.Chars[0] != ',')
            return new ReturnStmt(list.ToImmutable());
        ExpectSymbolic(ref source, ",");
        goto Next;
    }

    private record ConstIntExpr(long Num) : IExpr
    {
        public static readonly ConstIntExpr One = new(1);
    }

    private record ConstRealExpr(double Num) : IExpr;

    private static IExpr SynExpr(ref SourceContext source)
    {
        return TrySynExpr(ref source, out var expr) ? expr : throw source.UnexpectedToken("<expr>");
    }

    private static bool TrySynExpr(ref SourceContext source, out IExpr expr)
    {
        expr = null!;
        return false;
    }
}