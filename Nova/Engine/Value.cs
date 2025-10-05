using System.Runtime.CompilerServices;

namespace Nova;

public partial class Engine
{
    public enum OpUnary
    {
        Neg,
        BNot,
        LNot,
        Len
    }

    public enum OpBinary
    {
        Add,
        Sub,
        Mul,
        Div,
        Mod,
        Pow,
        BAnd,
        BOr,
        BXor,
        Shr,
        Shl,
        Eq,
        Lt,
        Le,
        LAnd,
        LOr
    }

    public struct Type
    {
        public unsafe delegate* managed <ref Stack, ref Frame, Value, Value, bool, Value> Get;

        public unsafe delegate* managed <ref Stack, ref Frame, Value, Value, bool, Value, void> Set;

        public unsafe delegate* managed <ref Stack, ref Frame, Value, OpUnary, out Value, bool> Unary;

        public unsafe delegate* managed <ref Stack, ref Frame, Value, Value, OpBinary, out Value, bool> Binary;

        public unsafe delegate* managed <ref Stack, ref Frame, Value, byte, bool, void> Call;

        public unsafe delegate* managed <ref Stack, ref Frame, Value, Value, byte, bool, void> Calli;
        
        public unsafe delegate* managed <ref Stack, ref Frame, Value, void> Final;
        
        public unsafe delegate* managed <ref Stack, ref Frame, Value, void> Close;

        public unsafe delegate* managed <Value, bool> IsHeap;
    }

    private static Value NullGet(ref Stack s, ref Frame f, Value t, Value n, bool r) => new();

    private static void NullSet(ref Stack s, ref Frame f, Value t, Value n, bool r, Value v)
    {
    }

    private static bool NullUnary(ref Stack s, ref Frame f, Value v, OpUnary o, out Value r)
    {
        r = default;
        return false;
    }

    private static bool NullBinary(ref Stack s, ref Frame f, Value l, Value r, OpBinary o, out Value v)
    {
        v = default;
        return false;
    }

    private sealed class ErrBadCall : Exception
    {
        public override string Message => "Object is not callable";
    }

    private static void NullCall(ref Stack s, ref Frame f, Value o, byte args, bool isTailCall)
    {
        throw new ErrBadCall();
    }

    private static void NullCalli(ref Stack s, ref Frame f, Value o, Value k, byte args, bool isTailCall)
    {
        throw new ErrBadCall();
    }

    public struct Value()
    {
        public unsafe Type* T = null;
        public long V = 0;
    }

    private static unsafe Value Make(in Type t, long v) => new() { T = (Type*)Unsafe.AsPointer(in t), V = v };
}