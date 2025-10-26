using System.Collections.Immutable;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;

namespace Nova;

public static class Parse
{
    public interface ISyntax;

    public readonly ref struct Grapheme(ReadOnlySpan<char> chars) : IEquatable<Grapheme>
    {
        private readonly ReadOnlySpan<char> _chars = chars;

        public bool Equals(Grapheme other) => _chars == other._chars || _chars.SequenceEqual(other._chars);

        public ReadOnlySpan<char> Chars => _chars;

        public SpanRuneEnumerator Runes => _chars.EnumerateRunes();
    }

    public readonly record struct SourceLine(int Line, int Chars);

    public readonly record struct SourceCluster(int Line, int Column, int Chars, int Span);

    public sealed class Source
    {
        private readonly string _text;
        private readonly List<SourceLine> _lines = [];
        private readonly List<SourceCluster> _clusters = [];

        public struct Cursor(Source source, int offset)
        {
            private int _offset = offset;

            public bool MoveNext() => ++_offset < source._clusters.Count;

            public Grapheme Current
            {
                get
                {
                    var clu = source._clusters[_offset];
                    return new Grapheme(source._text.AsSpan(clu.Chars, clu.Span));
                }
            }

            public Text Slice(in Cursor other) => new(source, _offset, other._offset - 1);
        }

        public Source(string text)
        {
            _text = text;

            // scan and separate all clusters in the source.
            var acc = 0;
            var span = text.AsSpan();
            while (!span.IsEmpty)
            {
                var len = StringInfo.GetNextTextElementLength(span);
                _clusters.Add(new SourceCluster(0, 0, acc, len));
                acc += len;
                span = span[len..];
            }

            // scan and separate all lines in the source by line ends in { CR, LF, CRLF, LFCR }
            var (line, column, count) = (0, 0, _clusters.Count);
            for (var i = 0; i < count; ++i)
            {
                var l = _clusters[i].Chars;
                if (column == 0) _lines.Add(new SourceLine(line, l));
                _clusters[i] = _clusters[i] with { Line = line, Column = column++ };
                if (i + 1 < count && _text.AsSpan(l, 2) is "\r\n" or "\n\r")
                {
                    _clusters[i + 1] = _clusters[i + 1] with { Line = line, Column = column };
                    (i, line, column) = (i + 1, line + 1, 0);
                }
                else if (_text[l] is '\r' or '\n') (line, column) = (line + 1, 0);
            }
        }

        public sealed class Text(Source source, int head, int tail) : IEquatable<Text>, ISyntax
        {
            public ReadOnlySpan<char> Chars
            {
                get
                {
                    var l = source._clusters[head];
                    var r = source._clusters[tail];
                    return source._text.AsSpan()[l.Chars..(r.Chars + r.Span)];
                }
            }

            public SourceCluster Head => source._clusters[head];

            public SourceCluster Tail => source._clusters[tail];

            public bool Equals(Text? other) => other != null && Chars.SequenceEqual(other.Chars);

            public override bool Equals(object? obj) => obj is Text other && Equals(other);

            public override int GetHashCode() => string.GetHashCode(Chars);
        }
    }


    public interface IScan
    {
        bool Visit(in Grapheme g);
        bool Complete();
    }

    public interface ISelectionBuilder
    {
        /// <summary>
        /// Match the exact sequence given by text once and drop the token.
        /// </summary>
        ISelectionBuilder Branch(string text);

        /// <summary>
        /// Match the exact rule given once and drop the token.
        /// </summary>
        ISelectionBuilder BranchRule(string rule);
    }

    public interface ISequenceBuilder
    {
        /// <summary>
        /// If the current parsing is in a multi-branch choice,
        /// anchor this sequence as the branch chosen, and further mismatch will produce diagnostics error
        /// </summary>
        ISequenceBuilder Anchor();

        /// <summary>
        /// Match the exact sequence given by text once and drop the token.
        /// </summary>
        ISequenceBuilder Drop(string text);

        /// <summary>
        /// Match the exact rule given once and drop the token.
        /// </summary>
        ISequenceBuilder DropRule(string rule);

        /// <summary>
        /// Match the exact rule given at least min times and at most max times and drop the token.
        /// </summary>
        ISequenceBuilder DropRule(string rule, int min, int max);

        /// <summary>
        /// Match the exact sequence given by text once and store the token.
        /// </summary>
        ISequenceBuilder Keep(string text);

        /// <summary>
        /// Match the exact rule given once and store the token.
        /// </summary>
        ISequenceBuilder KeepRule(string rule);

        /// <summary>
        /// Match the exact rule given at least min times and at most max times and drop store the token(s) as array
        /// </summary>
        ISequenceBuilder KeepRule(string rule, int min, int max);

        /// <summary>
        /// Build the sequence given a mapper function
        /// </summary>
        /// <returns></returns>
        public void Build(Func<ReadOnlySpan<object>, object> map);
    }

    public interface ISyntaxBuilder
    {
        void FromScan<TS>(string name) where TS : struct, IScan;

        ISelectionBuilder Selection(string name);

        ISequenceBuilder Sequence(string name);

        Func<string, Source, object> Build(params ReadOnlySpan<string> symbol);
    }

    private sealed class RuleBuilder : ISyntaxBuilder
    {
        private static readonly object PlaceHolder = new();

        private delegate bool Scan(Context c, ref Source.Cursor s);

        private readonly List<Scan> _scan = [];
        private readonly List<object> _list = [];
        private readonly Dictionary<string, int> _named = [];
        private readonly Dictionary<string, int> _const = [];
        private readonly Dictionary<string, int> _scans = [];

        private sealed record CCst(string C);

        private sealed record CSel(List<int> M);

        private sealed record CSeq(
            List<(int R, int L, int U, bool A, bool K)> M,
            Func<ReadOnlySpan<object>, object> T
        );

        private static readonly object LazyMarker = new();

        private int GetConst(string v)
        {
            if (_const.TryGetValue(v, out var c)) return c;
            var r = _list.Count;
            _list.Add(new CCst(v));
            _const.Add(v, r);
            return r;
        }

        private int GetNamed(string n)
        {
            if (_named.TryGetValue(n, out var c)) return c;
            var r = _list.Count;
            _list.Add(LazyMarker);
            _named.Add(n, r);
            return r;
        }

        private sealed class CSeqB(RuleBuilder h, string n) : ISequenceBuilder
        {
            private bool _anchor;
            private readonly List<(int R, int L, int U, bool A, bool K)> _list = [];

            public ISequenceBuilder Anchor()
            {
                _anchor = true;
                return this;
            }

            private CSeqB Rule(int rule, int min, int max, bool keep)
            {
                _list.Add((rule, min, max, _anchor, keep));
                if (_anchor) _anchor = false;
                return this;
            }

            private CSeqB Drop(int rule, int min, int max) => Rule(rule, min, max, false);

            private CSeqB Keep(int rule, int min, int max) => Rule(rule, min, max, true);

            public ISequenceBuilder Drop(string text) => Drop(h.GetConst(text), 1, 1);

            public ISequenceBuilder DropRule(string rule) => Drop(h.GetNamed(rule), 1, 1);

            public ISequenceBuilder DropRule(string rule, int min, int max) => Drop(h.GetNamed(rule), min, max);

            public ISequenceBuilder Keep(string text) => Keep(h.GetConst(text), 1, 1);

            public ISequenceBuilder KeepRule(string rule) => Keep(h.GetNamed(rule), 1, 1);

            public ISequenceBuilder KeepRule(string rule, int min, int max) => Keep(h.GetNamed(rule), min, max);

            public void Build(Func<ReadOnlySpan<object>, object> map)
            {
                h._list[h.GetNamed(n)] = new CSeq(_list, map);
            }
        }

        private sealed class CSelB(RuleBuilder h, string n) : ISelectionBuilder
        {
            private readonly CSel _s = (CSel)(h._list[h.GetNamed(n)] = new CSel([]));

            public ISelectionBuilder Branch(string text)
            {
                _s.M.Add(h.GetConst(text));
                return this;
            }

            public ISelectionBuilder BranchRule(string rule)
            {
                _s.M.Add(h.GetNamed(rule));
                return this;
            }
        }

        private static bool ScanFinalize(Context c, Source.Cursor cc, ref Source.Cursor s)
        {
            switch (c.TopFrame().Keep)
            {
                case 0:
                    break;
                case 1:
                    c.Nodes.Add(s.Slice(in cc));
                    break;
                case 2:
                    c.Nodes.Add(PlaceHolder);
                    break;
            }

            s = cc;
            return true;
        }

        private void FromScan<TS>(TS init) where TS : struct, IScan
        {
            _scan.Add((c, ref s) =>
            {
                var cc = s;
                var scan = init;
                while (scan.Visit(cc.Current))
                    if (cc.MoveNext())
                        break;
                return scan.Complete() && ScanFinalize(c, cc, ref s);
            });
        }

        public void FromScan<TS>(string name) where TS : struct, IScan
        {
            var r = _list.Count;
            _scans.Add(name, r);
            FromScan(new TS());
        }

        public ISelectionBuilder Selection(string name) => new CSelB(this, name);

        public ISequenceBuilder Sequence(string name) => new CSeqB(this, name);

        private struct Frame
        {
            // 0 - selection
            // 1 - sequential non anchored
            // 2 - sequential anchored
            // 3 - array builder
            public int Mode;

            // index to the name of current rule
            public int Name;

            // PC + 1 of what called this rule
            public int Last;

            // How the current value should be handled
            // 0 - drop
            // 1 - keep
            // 2 - keep placeholder (mode array builder on drop)
            public int Keep;
        }

        private sealed class Context
        {
            public readonly List<Frame> Steps = [];

            public readonly List<object> Nodes = [];

            public void PopFrame() => Steps.RemoveAt(Steps.Count - 1);

            public ref Frame TopFrame() => ref CollectionsMarshal.AsSpan(Steps)[^1];
        }

        /*
         * Seq:
         * MODE 1
         * NAME 1
         * KEEP 0
         * CALL 1
         * FAIL
         * MODE 2
         * KEEP 1
         * CALL 2
         * FAIL
         * BUILD 1
         * DONE
         */

        /*
         * Sel:
         * MODE 0
         * NAME 1
         * CALL 1
         * DONE
         * CALL 2
         * DONE
         * CALL 3
         * DONE
         * FAIL
         */

        /*
         * Arr:
         * MODE 3
         * CALL 1 CONST 0 CONST 0x7FFFFFF
         * FAIL
         * PACK
         * DONE
         */

        /*
         * Scan, does not use mode as it does not need call
         * SCAN 1
         * FAIL
         * DONE
         */

        /*
         * Stub:
         * MODE 2
         * CALL 1
         * FAIL
         * HALT
         */

        private enum Op
        {
            // set mode of current rule
            Mode,

            // set keep of current rule
            Keep,

            // set index of name of current rule
            Name,

            // setup call to use a rule by using its instruction offset as argument
            // the following effects happens based on mode on Done or Fail is hit in the rule
            // mode 0: do no extra on success, skip one instruction on fail
            // mode 1 & 2: skip one instruction on success, do no extra on fail
            // mode 3:
            // on success, check second trailing constant and see if maximum is satisfied
            // if so, return and skip one instruction, else repeat the call
            // on failure, check first trailing constant and see if minimum is satisfied
            // if so, return and skip one instruction, else return and do no extra
            Call,

            // use a scan rule of using its rule offset as argument
            // on success, unconditionally skips an exception
            Scan,

            // pack the current frame into an array return
            Pack,

            // build the current frame into an object by calling a mapping rule by index
            Build,

            // mark the successful end of a sequence
            Done,

            // mark the failure end of a sequence
            Fail,

            // mark the end of evaluation and return the last element on stack
            Halt,

            // placeholder for inline extra const value
            Const
        }

        private readonly record struct Ins(Op Op, int Arg);

        private sealed class Parser(
            Ins[] ins,
            Scan[] scan,
            string[] name,
            Func<ReadOnlySpan<object>, object>[] build,
            Dictionary<string, int> symbols
        )
        {
            private readonly string[] _name = name;

            public object Run(string sym, Source src) => Run(symbols[sym], src);

            private object Run(int pc, Source src)
            {
                var ctx = new Context();
                var cur = new Source.Cursor(src, 0);
                ctx.Steps.Add(new Frame());
                ref var fra = ref ctx.TopFrame();
                for (;; ++pc)
                {
                    var (op, arg) = ins[pc];
                    switch (op)
                    {
                        case Op.Mode:
                            fra.Mode = arg;
                            if (arg == 3 && fra.Keep is 0 or 2) fra.Keep = 2;
                            break;
                        case Op.Keep:
                            if (fra.Keep != 2) fra.Keep = arg;
                            break;
                        case Op.Name:
                            fra.Name = arg;
                            break;
                        case Op.Call:
                            ctx.Steps.Add(new Frame { Last = pc + 1, Keep = fra.Keep });
                            fra = ref ctx.TopFrame();
                            pc = arg;
                            continue;
                        case Op.Scan:
                            if (scan[arg](ctx, ref cur)) ++pc;
                            break;
                        case Op.Pack:
                            // handle the value keep rule
                            switch (fra.Keep)
                            {
                                case 0:
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    break;
                                case 1:
                                {
                                    var o = ImmutableArray.Create(CollectionsMarshal.AsSpan(ctx.Nodes)[fra.Last..]);
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    ctx.Nodes.Add(o);
                                    break;
                                }
                                case 2:
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    ctx.Nodes.Add(PlaceHolder);
                                    break;
                            }

                            break;
                        case Op.Build:
                            // handle the value keep rule
                            switch (fra.Keep)
                            {
                                case 0:
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    break;
                                case 1:
                                {
                                    var o = build[arg](CollectionsMarshal.AsSpan(ctx.Nodes)[fra.Last..]);
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    ctx.Nodes.Add(o);
                                    break;
                                }
                                case 2:
                                    ctx.Nodes.RemoveRange(fra.Last, ctx.Nodes.Count - fra.Last);
                                    ctx.Nodes.Add(PlaceHolder);
                                    break;
                            }

                            break;
                        case Op.Done:
                        {
                            pc = fra.Last;
                            ctx.PopFrame();
                            fra = ref ctx.TopFrame();
                            switch (fra.Mode)
                            {
                                case 0:
                                    continue;
                                case 1:
                                case 2:
                                    ++pc;
                                    continue;
                                case 3:
                                {
                                    var max = ins[pc + 2].Arg;
                                    var cnt = ctx.Nodes.Count - fra.Last;
                                    if (cnt < max) --pc;
                                    else ++pc;
                                    continue;
                                }
                                default:
                                    throw new InvalidOperationException();
                            }
                        }
                        case Op.Fail:
                        {
                            if (fra.Mode == 2)
                            {
                                // TODO: emit diagnostics
                                throw new Exception("Bad Grammar");
                            }

                            pc = fra.Last;
                            ctx.PopFrame();
                            fra = ref ctx.TopFrame();
                            switch (fra.Mode)
                            {
                                case 0:
                                    ++pc;
                                    continue;
                                case 1:
                                case 2:
                                    continue;
                                case 3:
                                {
                                    var min = ins[pc + 1].Arg;
                                    var cnt = ctx.Nodes.Count - fra.Last;
                                    if (cnt >= min) ++pc;
                                    continue;
                                }
                                default:
                                    throw new InvalidOperationException();
                            }
                        }
                        case Op.Halt:
                            return ctx.Nodes[0];
                        case Op.Const:
                        default:
                            throw new ArgumentOutOfRangeException();
                    }
                }
            }
        }


        public Func<string, Source, object> Build(params ReadOnlySpan<string> symbols)
        {
            // find unresolved rule names
            foreach (var (n, i) in _named)
                if (_list[i] == LazyMarker)
                    Console.WriteLine($"Rule demanded but unresolved: {n}");
            Console.WriteLine($"Found {_named.Count} named rules");
            Console.WriteLine($"Found {_const.Count} const rules");
            return (_, _) => null;
        }
    }

    public static ISyntaxBuilder CreateSyntaxBuilder() => new RuleBuilder();
}