namespace Nova;

public partial class Engine
{
    private static readonly unsafe Type BoolType = new()
    {
        Get = &NullGet,
        Set = &NullSet,
        Unary = &BoolUnary,
        Binary = &BoolBinary,
        Call = &NullCall,
        Calli = &NullCalli
    };

    private static Value BoolTrue => Make(in BoolType, 1);

    private static Value BoolFalse => Make(in BoolType, 0);

    private static bool BoolUnary(ref Stack s, ref Frame f, Value v, OpUnary o, out Value r)
    {
        if (o is not OpUnary.LNot)
        {
            r = default;
            return false;
        }

        r = v.V == 0 ? BoolTrue : BoolFalse;
        return true;
    }

    private static unsafe bool BoolBinary(ref Stack s, ref Frame f, Value l, Value r, OpBinary o, out Value v)
    {
        if (r.T != BoolTrue.T) goto Fail;

        switch (o)
        {
            case OpBinary.BAnd:
            case OpBinary.LAnd:
                v = Make(in BoolType, (int)l.V & (int)r.V);
                break;
            case OpBinary.LOr:
            case OpBinary.BOr:
                v = Make(in BoolType, (int)l.V | (int)r.V);
                break;
            case OpBinary.BXor:
                v = Make(in BoolType, (int)l.V ^ (int)r.V);
                break;
            default:
                goto Fail;
        }

        return true;
        Fail:
        v = default;
        return false;
    }
}