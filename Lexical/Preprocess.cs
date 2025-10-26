using System.Collections.Immutable;

namespace Lexical;

public sealed class PpToken : IEquatable<PpToken>
{
    private readonly string _type;
    private readonly Source.Part _part;

    internal PpToken(string type, Source.Part part) => (_type, _part) = (type, part);

    /// <summary>
    /// The type of this token as specified when building the rule
    /// </summary>
    public string Type => _type;

    /// <summary>
    /// The span of characters representing this token
    /// </summary>
    public ReadOnlySpan<char> Chars => _part.Chars;

    /// <summary>
    /// The location of the first character in the source for diagnostics
    /// </summary>
    public SourceLocation Head => ToLocation(_part.Head);

    /// <summary>
    /// The location of the last character in the source for diagnostics
    /// </summary>
    public SourceLocation Tail => ToLocation(_part.Tail);

    private static SourceLocation ToLocation(Source.SCluster c) => new(c.Line, c.Column);

    public bool Equals(PpToken? other) =>
        other is not null && (ReferenceEquals(this, other) || _part.Equals(other._part));

    public override bool Equals(object? obj) =>
        ReferenceEquals(this, obj) || obj is PpToken other && Equals(other);

    public override int GetHashCode() => _part.GetHashCode();
}

public interface IPpScanCtx
{
    /// <summary>
    /// Advance the scan location by one
    /// </summary>
    /// <returns>
    /// true if the operation succeeds, false if no more character is available
    /// </returns>
    bool MoveNext();

    /// <summary>
    /// The current grapheme cluster being scanned
    /// </summary>
    Grapheme Current { get; }

    /// <summary>
    /// Mark the beginning of the scan to an offset to the current scan as a token candidate.
    /// </summary>
    /// <param name="offset">
    /// Offset to the current scan
    /// </param>
    void MarkCandidate(int offset);
}

public sealed class PpRules
{
    public interface IBuilder
    {
        /// <summary>
        /// Define a PpToken type with scanner.
        /// </summary>
        /// <param name="name"> Name of type </param>
        /// <param name="scanner"> Scanner delegate </param>
        void Type(string name, Action<IPpScanCtx> scanner);

        /// <summary>
        /// Defines the delimiting token type between two tokens.
        /// The delimiting token is automatically ignored when processing token sequence,
        /// and is usually categorized as white-space.
        /// </summary>
        /// <param name="left"> Token type on the left </param>
        /// <param name="right"> Token type on the right </param>
        /// <param name="delim"> Token type in the middle </param>
        /// <param name="optional"> Is the delimiter optional </param>
        void Delim(string left, string right, string delim, bool optional);

        /// <summary>
        /// Moves the specified rule set into a new instance of PpRules.
        /// </summary>
        /// <returns>
        /// The created rule set
        /// </returns>
        PpRules Build();
    }

    private sealed class Builder : IBuilder
    {
        private readonly List<(string, Action<IPpScanCtx>)> _s = [];
        private readonly List<(string, string, string, bool)> _d = [];

        public void Type(string name, Action<IPpScanCtx> scanner) =>
            _s.Add((name, scanner));

        public void Delim(string left, string right, string delim, bool optional) =>
            _d.Add((left, right, delim, optional));

        public PpRules Build()
        {
            // assign id to scanner and build lookup/table
            var tb = ImmutableArray.CreateBuilder<Action<IPpScanCtx>>();
            var lb = ImmutableDictionary.CreateBuilder<string, int>();
            foreach (var (n, f) in _s)
            {
                lb.Add(n, tb.Count);
                tb.Add(f);
            }

            var t = tb.MoveToImmutable();
            var l = lb.ToImmutable();
            // build delim rules
            var b = new List<ImmutableArray<(int, int, bool)>.Builder>();
            for (var i = 0; i < t.Length; ++i) b.Add(ImmutableArray.CreateBuilder<(int, int, bool)>());
            foreach (var (tl, tm, tr, o) in _d) b[l[tl]].Add((l[tr], l[tm], o));

            var db = ImmutableArray.CreateBuilder<ImmutableArray<(int, int, bool)>>();
            foreach (var o in b) db.Add(o.MoveToImmutable());
            var d = db.MoveToImmutable();

            // create the rule set
            return new PpRules(t, d, l);
        }
    }

    private readonly ImmutableArray<Action<IPpScanCtx>> _scan;
    private readonly ImmutableArray<ImmutableArray<(int, int, bool)>> _delim;
    private readonly ImmutableDictionary<string, int> _name;

    internal PpRules(ImmutableArray<Action<IPpScanCtx>> scans,
        ImmutableArray<ImmutableArray<(int, int, bool)>> delim,
        ImmutableDictionary<string, int> names) => (_scan, _delim, _name) = (scans, delim, names);

    public static IBuilder CreateBuilder() => new Builder();

    internal int GetIndex(string name) => _name[name];
}