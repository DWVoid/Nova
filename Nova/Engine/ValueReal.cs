namespace Nova;

public partial class Engine
{
    private static readonly unsafe Type IntType = new()
    {
        Get = &NullGet,
        Set = &NullSet,
        Unary = &IntUnary,
        Binary = &IntBinary,
        Call = &NullCall,
        Calli = &NullCalli
    };

    private static readonly Value IntZero = Make(in IntType, 0);

    private static readonly unsafe Type RealType = new()
    {
        Get = &NullGet,
        Set = &NullSet,
        Unary = &RealUnary,
        Binary = &RealBinary,
        Call = &NullCall,
        Calli = &NullCalli
    };

    private static readonly Value RealZero = Make(in RealType, 0);

    private static bool IntUnary(ref Stack s, ref Frame f, Value v, OpUnary o, out Value r)
    {
        switch (o)
        {
            case OpUnary.Neg:
                r = Make(in IntType, -v.V);
                break;
            case OpUnary.BNot:
                r = Make(in IntType, ~v.V);
                break;
            default:
                goto Fail;
        }

        return true;
        Fail:
        r = default;
        return false;
    }

    private static bool RealUnary(ref Stack s, ref Frame f, Value v, OpUnary o, out Value r)
    {
        if (o is not OpUnary.Neg)
        {
            r = default;
            return false;
        }

        r = Make(in RealType, BitConverter.DoubleToInt64Bits(-BitConverter.Int64BitsToDouble(v.V)));
        return true;
    }

    private sealed class ErrBadShift : Exception
    {
        public override string Message => "Shift num cannot be negative";
    }

    private static bool DoIntBinary(long l, long r, OpBinary o, out Value v)
    {
        switch (o)
        {
            case OpBinary.Add:
                v = Make(in IntType, l + r);
                break;
            case OpBinary.Sub:
                v = Make(in IntType, l - r);
                break;
            case OpBinary.Mul:
                v = Make(in IntType, l * r);
                break;
            case OpBinary.Div:
                v = Make(in IntType, l / r);
                break;
            case OpBinary.Mod:
                v = Make(in IntType, l % r);
                break;
            case OpBinary.Pow:
                v = Make(in IntType, (long)Math.Pow(l, r));
                break;
            case OpBinary.BAnd:
                v = Make(in IntType, l & r);
                break;
            case OpBinary.BOr:
                v = Make(in IntType, l | r);
                break;
            case OpBinary.BXor:
                v = Make(in IntType, l ^ r);
                break;
            case OpBinary.Shr:
                if (r < 0) throw new ErrBadShift();
                v = Make(in IntType, l >> (int)r);
                break;
            case OpBinary.Shl:
                if (r < 0) throw new ErrBadShift();
                v = Make(in IntType, l << (int)r);
                break;
            case OpBinary.Eq:
                v = l == r ? BoolTrue : BoolFalse;
                break;
            case OpBinary.Lt:
                v = l < r ? BoolTrue : BoolFalse;
                break;
            case OpBinary.Le:
                v = l <= r ? BoolTrue : BoolFalse;
                break;
            default:
                goto Fail;
        }

        return true;
        Fail:
        v = default;
        return false;
    }

    private static bool DoRealBinary(double l, double r, OpBinary o, out Value v)
    {
        switch (o)
        {
            case OpBinary.Add:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(l + r));
                break;
            case OpBinary.Sub:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(l - r));
                break;
            case OpBinary.Mul:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(l * r));
                break;
            case OpBinary.Div:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(l / r));
                break;
            case OpBinary.Mod:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(l % r));
                break;
            case OpBinary.Pow:
                v = Make(in RealType, BitConverter.DoubleToInt64Bits(Math.Pow(l, r)));
                break;
            case OpBinary.Eq:
                v = l.Equals(r) ? BoolTrue : BoolFalse;
                break;
            case OpBinary.Lt:
                v = l < r ? BoolTrue : BoolFalse;
                break;
            case OpBinary.Le:
                v = l <= r ? BoolTrue : BoolFalse;
                break;
            default:
                goto Fail;
        }

        return true;
        Fail:
        v = default;
        return false;
    }

    private static unsafe bool IntBinary(ref Stack s, ref Frame f, Value l, Value r, OpBinary o, out Value v)
    {
        if (r.T == IntZero.T) return DoIntBinary(l.V, r.V, o, out v);
        if (r.T == RealZero.T) return DoRealBinary(l.V, BitConverter.Int64BitsToDouble(r.V), o, out v);
        v = default;
        return false;
    }

    private static unsafe bool RealBinary(ref Stack s, ref Frame f, Value l, Value r, OpBinary o, out Value v)
    {
        if (r.T == RealZero.T)
            return DoRealBinary(
                BitConverter.Int64BitsToDouble(l.V), BitConverter.Int64BitsToDouble(r.V), o, out v
            );
        v = default;
        return false;
    }
}