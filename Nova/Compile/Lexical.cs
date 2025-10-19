using System.Diagnostics;
using System.Text;

namespace Nova;

public static partial class Compile
{
    private struct IdScan() : Parse.IScan
    {
        private bool _est = false;

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

        private static bool IsIdC(Parse.Grapheme g)
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

        private static bool IsIdI(Parse.Grapheme r)
        {
            using var en = r.Runes;
            en.MoveNext();
            if (!IsIdI(en.Current)) return false;
            while (en.MoveNext())
                if (!IsIdC(en.Current))
                    return false;
            return true;
        }

        public bool Visit(in Parse.Grapheme g)
        {
            if (_est) return IsIdC(g);
            if (!IsIdI(g)) return false;
            return _est = true;
        }

        public bool Complete() => _est;
    }

    private struct WsScan : Parse.IScan
    {
        public bool Visit(in Parse.Grapheme g) =>
            g.Chars.Length == 1 && g.Chars[0] is not (' ' or '\t' or '\v' or '\f' or '\r' or '\n');

        public bool Complete() => true;
    }

    private struct NumScan() : Parse.IScan
    {
        private bool _est = false, _mod = false, _hex = false, _cpl = true, _exp = false;

        public bool Visit(in Parse.Grapheme g)
        {
            if (_est) return IsC(g);
            if (!IsI(g)) return false;
            return _est = true;
        }

        private bool IsC(in Parse.Grapheme g)
        {
            if (g.Chars.Length != 1) return false;
            var ch = g.Chars[0];
            if (ch is < '0' or > '9') return false;
            if (ch == 0) (_mod, _cpl) = (true, false);
            return true;
        }

        private bool IsI(in Parse.Grapheme g)
        {
            if (g.Chars.Length != 1) return false;
            var ch = g.Chars[0];
            if (_mod)
            {
                if (ch is 'x' or 'X') _hex = true;
                _mod = false;
                return true;
            }

            if (_hex)
            {
                if (ch is >= '0' and <= '9' or >= 'a' and <= 'f' or >= 'A' and <= 'F')
                {
                    _cpl = true;
                    return true;
                }

                if (_cpl && ch is '.')
                {
                    _cpl = false;
                    return true;
                }

                if (_cpl && ch is 'P' or 'p')
                {
                    (_cpl, _exp) = (false, true);
                    return true;
                }

                if (_exp && ch is '+' or '-')
                {
                    return true;
                }

                if (_exp && ch is >= '0' and <= '9')
                {
                    _cpl = true;
                    return true;
                }
            }
            else
            {
                if (ch is >= '0' and <= '9')
                {
                    _cpl = true;
                    return true;
                }

                if (_cpl && ch is '.')
                {
                    _cpl = false;
                    return true;
                }

                if (_cpl && ch is 'E' or 'e')
                {
                    (_cpl, _exp) = (false, true);
                    return true;
                }

                if (_exp && ch is '+' or '-')
                {
                    return true;
                }

                if (_exp && ch is >= '0' and <= '9')
                {
                    _cpl = true;
                    return true;
                }
            }

            return false;
        }

        public bool Complete() => _est && _cpl;
    }

    private struct StrScan() : Parse.IScan
    {
        private char _init = '\0';
        private bool _est = false, _esc = false, _closed = false;
        private int _rS = 0, _rE = 0;

        public bool Visit(in Parse.Grapheme g)
        {
            if (_init == '\0') return IsC(g);
            if (!(_init switch
                {
                    '\'' => SsChar(g),
                    '"' => DsChar(g),
                    '[' => GsChar(g),
                    _ => throw new UnreachableException()
                })) return false;
            return _est = true;
        }

        private bool IsC(Parse.Grapheme g)
        {
            if (g.Chars.Length != 1) return false;
            var ch = g.Chars[0];
            if (ch is not ('\'' or '"' or '[')) return false;
            if (ch is not '[') _est = true;
            _init = ch;
            return true;
        }

        private bool SsChar(in Parse.Grapheme g)
        {
            if (_closed) return false;
            if (!_esc && g.Chars[0] == '\'') _closed = true;
            if (!_esc && g.Chars[0] == '\\') _esc = true;
            if (_esc) _esc = false;
            return true;
        }

        private bool DsChar(in Parse.Grapheme g)
        {
            if (_closed) return false;
            if (!_esc && g.Chars[0] == '"') _closed = true;
            if (!_esc && g.Chars[0] == '\\') _esc = true;
            if (_esc) _esc = false;
            return true;
        }

        private bool GsChar(in Parse.Grapheme g)
        {
            if (_closed) return false;
            if (!_est)
            {
                switch (g.Chars[0])
                {
                    case '=':
                        ++_rS;
                        return true;
                    case ']':
                        ++_rS;
                        _est = true;
                        return true;
                    default:
                        return false;
                }
            }

            if (_rE == 0)
            {
                if (g.Chars[0] is ']') _rE = 1;
                return true;
            }

            if (g.Chars[0] is '=')
            {
                ++_rE;
                return true;
            }

            if (g.Chars[0] is ']')
            {
                if (_rS == _rE) _closed = true;
                return true;
            }

            return false;
        }

        public bool Complete() => _est && _closed;
    }
}