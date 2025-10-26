using System.Globalization;
using System.Text;

namespace Lexical;

public readonly ref struct Grapheme(ReadOnlySpan<char> chars) : IEquatable<Grapheme>
{
    private readonly ReadOnlySpan<char> _chars = chars;

    public bool Equals(Grapheme other) => _chars == other._chars || _chars.SequenceEqual(other._chars);

    public ReadOnlySpan<char> Chars => _chars;

    public SpanRuneEnumerator Runes => _chars.EnumerateRunes();
}

/// <summary>
/// Representing a source input unit.
/// The origin of this source (e.g. file name) is non concern here,
/// as it should not participate in the rule engine.
/// Such data could be spliced into the source via intrinsic syntax or preprocessor.
/// </summary>
public sealed class Source
{
    internal readonly record struct SLine(int Line, int Chars);

    internal readonly record struct SCluster(int Line, int Column, int Chars, int Span);

    private readonly string _text;
    private readonly List<SLine> _lines = [];
    private readonly List<SCluster> _clusters = [];

    internal struct Cursor(Source source, int offset)
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

        public Part Slice(in Cursor other) => new(source, _offset, other._offset - 1);
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
            _clusters.Add(new SCluster(0, 0, acc, len));
            acc += len;
            span = span[len..];
        }

        // scan and separate all lines in the source by line ends in { CR, LF, CRLF, LFCR }
        var (line, column, count) = (0, 0, _clusters.Count);
        for (var i = 0; i < count; ++i)
        {
            var l = _clusters[i].Chars;
            if (column == 0) _lines.Add(new SLine(line, l));
            _clusters[i] = _clusters[i] with { Line = line, Column = column++ };
            if (i + 1 < count && _text.AsSpan(l, 2) is "\r\n" or "\n\r")
            {
                _clusters[i + 1] = _clusters[i + 1] with { Line = line, Column = column };
                (i, line, column) = (i + 1, line + 1, 0);
            }
            else if (_text[l] is '\r' or '\n') (line, column) = (line + 1, 0);
        }
    }

    internal sealed class Part(Source source, int head, int tail) : IEquatable<Part>
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

        public SCluster Head => source._clusters[head];

        public SCluster Tail => source._clusters[tail];

        public bool Equals(Part? other) => other != null && Chars.SequenceEqual(other.Chars);

        public override bool Equals(object? obj) => obj is Part other && Equals(other);

        public override int GetHashCode() => string.GetHashCode(Chars);
    }
}

public readonly record struct SourceLocation(int Line, int Column);
