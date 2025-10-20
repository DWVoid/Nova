using System.Globalization;
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

    public interface IRule
    {
        object? Try(ref Source.Cursor c);
    }

    public interface IScan
    {
        bool Visit(in Grapheme g);
        bool Complete();
    }

    private sealed class ScanRule<TS>(TS init) : IRule where TS : struct, IScan
    {
        public object? Try(ref Source.Cursor c)
        {
            var cc = c;
            var scan = init;
            while (scan.Visit(cc.Current))
                if (cc.MoveNext())
                    break;
            if (!scan.Complete()) return null;
            var r = c.Slice(in cc);
            c = cc;
            return r;
        }
    }

    private struct ExactScan(string s) : IScan
    {
        private int _match = 0;

        public bool Visit(in Grapheme g)
        {
            var span = s.AsSpan(_match);
            // guard against partial grapheme match
            if (span.Length < g.Chars.Length) return false;
            span = span[..g.Chars.Length];
            // do sequence compare on full cluster length
            var r = span.SequenceEqual(g.Chars);
            if (r) _match += g.Chars.Length;
            return r;
        }

        public bool Complete() => _match == s.Length;
    }

    private sealed class RepeatRule : IRule
    {
        public required IRule Rule { get; init; }
        public required int Min { get; init; }
        public required int Max { get; init; }

        public object? Try(ref Source.Cursor c)
        {
            var cc = c;
            var rr = new List<object>();
            for (var i = 0; i < Max; i++)
            {
                var r = Rule.Try(ref cc);
                if (r == null)
                    if (i >= Min) break;
                    else return null;
                rr.Add(r);
            }

            return rr.ToArray();
        }
    }

    public sealed class ParseContext
    {
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

        void Build();
    }

    private sealed class RuleBuilder : ISyntaxBuilder
    {
        private readonly List<object> _list = [];
        private readonly Dictionary<string, int> _named = [];
        private readonly Dictionary<string, int> _const = [];

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

        public void FromScan<TS>(string name) where TS : struct, IScan
        {
            // throw new NotImplementedException();
        }

        public ISelectionBuilder Selection(string name) => new CSelB(this, name);

        public ISequenceBuilder Sequence(string name) => new CSeqB(this, name);
        
        public void Build()
        {
            // find unresolved rule names
            foreach (var (n, i) in _named)
                if (_list[i] == LazyMarker)
                    Console.WriteLine($"Rule demanded but unresolved: {n}");
            Console.WriteLine($"Found {_named.Count} named rules");
            Console.WriteLine($"Found {_const.Count} const rules");
        }
    }

    public static ISyntaxBuilder CreateSyntaxBuilder() => new RuleBuilder();
}