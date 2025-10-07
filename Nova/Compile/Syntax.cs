using System.Collections.Immutable;
using System.Runtime.CompilerServices;

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

    // ‘;’ | 
    // varlist ‘=’ explist | 
    // functioncall | 
    // label | 
    // break | 
    // goto Name | 
    // do block end | 
    // while exp do block end | 
    // repeat block until exp | 
    // if exp then block {elseif exp then block} [else block] end | 
    // for Name ‘=’ exp ‘,’ exp [‘,’ exp] do block end | 
    // for namelist in explist do block end | 
    // function funcname funcbody | 
    // local function Name funcbody | 
    // local attnamelist [‘=’ explist] 
    private static bool TryStmt(ref SourceContext source, out IStmt syn)
    {
        SkipWs(ref source);
        if (TryGetId(ref source.Text, out var tok))
        {
            switch (tok.Chars.ToString())
            {
                case "break":
                    syn = new BreakStmt();
                    break;
                case "goto":
                    syn = StmtGoto(ref source);
                    break;
                case "do":
                    syn = StmtDo(ref source);
                    break;
                case "while":
                    syn = StmtWhile(ref source);
                    break;
                case "repeat":
                    syn = StmtRepeat(ref source);
                    break;
                case "if":
                    syn = StmtIf(ref source);
                    break;
                case "for":
                    syn = StmtFor(ref source);
                    break;
                case "function":
                    break;
                case "local":
                    break;
            }
        }

        syn = null;
        return true;
    }

    private record BreakStmt : IStmt;

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

    private record ForNumStmt(SourceText Name, IExpr Init, IExpr Limit, IExpr Step, Block Block);

    private record ForIterStmt(ImmutableArray<SourceText> Name, ImmutableArray<IExpr> Expr, Block Block) : IStmt;

    private static IStmt StmtFor(ref SourceContext source)
    {
        var name = GetId(ref source);
        SkipWs(ref source);
        if (source.Text.Length == 0)
            throw source.UnexpectedToken("<,|=|in>");
        switch (source.Text.Chars[0])
        {
            case '=':
            {
                var init = SynExpr(ref source);
                ExpectSymbolic(ref source, ",");
                var limit = SynExpr(ref source);
                SkipWs(ref source);
                if (source.Text.Length == 0)
                    throw source.UnexpectedToken("<,|=|in>");
                IExpr step = ConstIntExpr.One;
                if (source.Text.Chars[0] == ',')
                {
                    ExpectSymbolic(ref source, ",");
                    step = SynExpr(ref source);
                }
                else
                {
                }

                break;
            }
            case ',':
                goto case 'i';
            case 'i':
                ExpectLiteral(ref source, "in");
                break;
        }

        throw new NotImplementedException();
    }

    private record ConstIntExpr(long Num) : IExpr
    {
        public static readonly ConstIntExpr One = new(1);
    }

    private record ConstRealExpr(double Num) : IExpr;

    private static IExpr SynExpr(ref SourceContext source)
    {
        return null!;
    }
}