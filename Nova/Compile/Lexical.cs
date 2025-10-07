using System.Collections;
using System.Globalization;
using System.Runtime.InteropServices;
using System.Text;

namespace Nova;

public static partial class Compile
{
    public record SourceText(string Text, (int, int) Head, (int, int) Tail);

    private readonly ref struct Grapheme(ReadOnlySpan<char> chars) : IEquatable<Grapheme>
    {
        private readonly ReadOnlySpan<char> _chars = chars;

        public bool Equals(Grapheme other) => _chars == other._chars || _chars.SequenceEqual(other._chars);

        public ReadOnlySpan<char> Chars => _chars;

        public SpanRuneEnumerator Runes => _chars.EnumerateRunes();
    }

    private readonly ref struct Graphemes(
        ReadOnlySpan<char> chars,
        ReadOnlySpan<int> elements,
        int offset
    ) : IEquatable<Graphemes>
    {
        public ref struct Enumerator : IEnumerator<Grapheme>
        {
            private readonly ReadOnlySpan<char> _chars;

            private ReadOnlySpan<int> _elements;

            internal Enumerator(Graphemes text)
            {
                _chars = text._chars;
                _elements = text._elements;
            }

            public Grapheme Current => _elements.Length >= 2
                ? new Grapheme(_chars[_elements[0].._elements[1]])
                : default;

            public bool MoveNext()
            {
                if (_elements.Length == 2) return false;
                _elements = _elements[1..];
                return true;
            }

            object IEnumerator.Current => throw new NotSupportedException();

            void IEnumerator.Reset() => throw new NotSupportedException();

            void IDisposable.Dispose()
            {
            }
        }

        private readonly ReadOnlySpan<char> _chars = chars;

        private readonly ReadOnlySpan<int> _elements = elements;

        public bool Equals(Graphemes other)
        {
            var lSlice = Chars;
            var rSlice = other.Chars;
            return lSlice == rSlice || lSlice.SequenceEqual(rSlice);
        }

        public int Offset => offset;

        public int Length => _elements.IsEmpty ? 0 : _elements.Length - 1;

        public Grapheme this[int index] => new(_chars[_elements[index].._elements[index + 1]]);

        public Graphemes this[Range range] => new(
            _chars, _elements[range.Start..(range.End.Value + 1)], offset + range.Start.Value
        );

        public ReadOnlySpan<char> Chars => _chars[_elements[0].._elements[^1]];

        public SpanRuneEnumerator Runes => _chars.EnumerateRunes();

        public Enumerator GetEnumerator() => new(this);

        public SourceText ToSourceText(List<int> lines)
        {
            var lc = lines.Count;
            var ls = Offset - lines[^1];
            return new SourceText(Chars.ToString(), (lc, ls), (lc, ls + Length));
        }
    }

    private static Graphemes BuildGraphemeLength(string text)
    {
        var span = text.AsSpan();
        var list = new List<int> { 0 };
        while (!span.IsEmpty)
        {
            var len = StringInfo.GetNextTextElementLength(span);
            list.Add(list[^1] + len);
            span = span[len..];
        }

        return new Graphemes(text, CollectionsMarshal.AsSpan(list), 0);
    }

    // 26 pairs
    private static readonly int[] IdCTb =
    [
        0x0030, 0x0039, 0x0041, 0x005A, 0x005F, 0x005F, 0x0061, 0x007A,
        0x00A8, 0x00A8, 0x00AA, 0x00AA, 0x00AD, 0x00AD, 0x00AF, 0x00AF, 0x00B2, 0x00B5,
        0x00B7, 0x00BA, 0x00BC, 0x00BE, 0x00C0, 0x00D6, 0x00D8, 0x00F6, 0x00F8, 0x00FF,
        0x0100, 0x167F, 0x1681, 0x180D, 0x180F, 0x1FFF,
        0x200B, 0x200D, 0x202A, 0x202E, 0x203F, 0x2040, 0x2054, 0x2054, 0x2060, 0x206F,
        0x2070, 0x218F, 0x2460, 0x24FF, 0x2776, 0x2793, 0x2C00, 0x2DFF, 0x2E80, 0x2FFF,
        0x3004, 0x3007, 0x3021, 0x302F, 0x3031, 0xD7FF,
        0xF900, 0xFD3D, 0xFD40, 0xFDCF, 0xFDF0, 0xFE44, 0xFE47, 0xFFFD
    ];

    // 29 pairs
    private static readonly int[] IdITb =
    [
        0x0041, 0x005A, 0x005F, 0x005F, 0x0061, 0x007A,
        0x00A8, 0x00A8, 0x00AA, 0x00AA, 0x00AD, 0x00AD, 0x00AF, 0x00AF, 0x00B2, 0x00B5,
        0x00B7, 0x00BA, 0x00BC, 0x00BE, 0x00C0, 0x00D6, 0x00D8, 0x00F6, 0x00F8, 0x00FF,
        0x0100, 0x0300, 0x036F, 0x167F, 0x1681, 0x180D, 0x180F, 0x1DC0, 0x1DFF, 0x1FFF,
        0x200B, 0x200D, 0x202A, 0x202E, 0x203F, 0x2040, 0x2054, 0x2054, 0x2060, 0x206F,
        0x2070, 0x20D0, 0x20FF, 0x218F, 0x2460, 0x24FF, 0x2776, 0x2793, 0x2C00, 0x2DFF,
        0x2E80, 0x2FFF, 0x3004, 0x3007, 0x3021, 0x302F, 0x3031, 0xD7FF,
        0xF900, 0xFD3D, 0xFD40, 0xFDCF, 0xFDF0, 0xFE20, 0xFE2F, 0xFE44, 0xFE47, 0xFFFD
    ];

    private static bool IsIdC(Rune r)
    {
        var ui = r.Value;
        if (ui > 0xEFFFD || (ui & 0xFFFF) > 0xFFFE) return false;
        var (b, e) = (0, 26);
        do
        {
            var m = (b + e) / 2;
            (IdCTb[m << 1] > ui ? ref e : ref b) = m;
        } while (e - b != 1);

        return IdCTb[(b << 1) | 1] >= ui;
    }

    private static bool IsIdC(Grapheme g)
    {
        foreach (var r in g.Runes)
            if (!IsIdC(r))
                return false;
        return true;
    }

    private static bool IsIdI(Rune r)
    {
        var ui = r.Value;
        if (ui > 0xEFFFD || (ui & 0xFFFF) > 0xFFFE) return false;
        var (b, e) = (0, 29);
        do
        {
            var m = (b + e) / 2;
            (IdITb[m << 1] > ui ? ref e : ref b) = m;
        } while (e - b != 1);

        return IdITb[(b << 1) | 1] >= ui;
    }

    private static bool IsIdI(Grapheme r)
    {
        using var en = r.Runes;
        en.MoveNext();
        if (!IsIdI(en.Current)) return false;
        while (en.MoveNext())
            if (!IsIdC(en.Current))
                return false;
        return true;
    }

    private static bool TryGetId(ref Graphemes text, out Graphemes token)
    {
        var count = 0;

        foreach (var g in text)
        {
            if (count == 0)
            {
                if (!IsIdI(g))
                {
                    token = default;
                    return false;
                }
            }
            else if (!IsIdC(g)) break;

            ++count;
        }

        token = text[..count];
        text = text[count..];
        return true;
    }

    private static bool TryGetSym(ref Graphemes text, out Graphemes token)
    {
        var count = 0;

        foreach (var g in text)
            if (g.Chars[0] is >= '!' and <= '/' or >= ':' and <= '@' or >= '[' and <= '`' or >= '{' and <= '~')
                ++count;
            else
                break;

        if (count > 0)
        {
            token = text[..count];
            text = text[count..];
            return true;
        }

        token = default;
        return false;
    }

    private static bool TryGetNum(ref Graphemes text, out Graphemes token)
    {
        var count = 0;

        foreach (var g in text)
            if (g.Chars[0] is >= '0' and <= '9')
                ++count;
            else
                break;

        if (count > 0)
        {
            token = text[..count];
            text = text[count..];
            return true;
        }

        token = default;
        return false;
    }

    private ref struct SourceContext(Graphemes text)
    {
        public Graphemes Text = text;
        public readonly List<int> Lines = [0];

        public Exception UnexpectedToken(string type) => throw new Exception(
            $"unexpected token [{Lines.Count}:{Text.Offset - Lines[^1]}]: expecting {type} got {(Text.Length > 0 ? Text.Chars[0] : "nil")}"
        );
    }

    private static Graphemes GetId(ref SourceContext source)
    {
        SkipWs(ref source);
        if (!TryGetId(ref source.Text, out var token))
            throw source.UnexpectedToken("<identifier>");
        return token;
    }

    private static void SkipWs(ref SourceContext source)
    {
        Next:
        if (source.Text.Length >= 2)
        {
            var chars = source.Text.Chars;
            if ((chars[0] == '\r' && chars[1] == '\n') || (chars[0] == '\n' && chars[1] == '\r'))
            {
                source.Text = source.Text[2..];
                source.Lines.Add(source.Text.Offset);
                goto Next;
            }
            // TODO: should also pick out comment here
        }

        if (source.Text.Length >= 1)
        {
            var chars = source.Text.Chars;
            if (chars[0] == '\r' || chars[0] == '\n')
            {
                source.Text = source.Text[1..];
                source.Lines.Add(source.Text.Offset);
                goto Next;
            }

            for (var i = 0; i < chars.Length; ++i)
                if (chars[i] is not (' ' or '\t' or '\v' or '\f'))
                {
                    if (i > 0) source.Text = source.Text[i..];
                    goto Next;
                }
        }
    }
}