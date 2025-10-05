using System.Runtime.CompilerServices;

namespace Nova;

public partial class Engine
{
    public struct Frame()
    {
        /// <summary> Stack pointer. Start of registers </summary>
        public int Sp = 0;

        /// <summary> Count of actual params of invocation </summary>
        public int Ac = 0;

        /// <summary> Program counter to return to </summary>
        public nuint Rp = 0;
    }

    public struct Stack
    {
        /// <summary> pointer to stack value memory </summary>
        public unsafe Value* Vs;

        /// <summary> pointer to stack frame memory </summary>
        public unsafe Frame* Fs;

        /// <summary> stack values allocated </summary>
        public int Sa;

        /// <summary> stack values capacity </summary>
        public int Sc;

        /// <summary> stack frames allocated </summary>
        public int Fa;

        /// <summary> stack frames capacity </summary>
        public int Fc;

        /// <summary> current program pointer </summary>
        public nuint Pp;
    }

    private sealed class ErrSaOor(int count, int index) : Exception
    {
        public int Count => count;

        public int Index => index;

        public override string Message => $"Param out of range: [{index + 1}] of [{count}]";
    }

    private sealed class ErrSrOor(int count, int index) : Exception
    {
        public int Count => count;

        public int Index => index;

        public override string Message => $"Register out of range: [{index}] of [{count}]";
    }

    private sealed class ErrSaOvf(int count, int index) : Exception
    {
        public int Count => count;

        public int Index => index;

        public override string Message => $"Stack overflow: [{index}] of [{count}]";
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe Value SaGet(ref Stack s, ref Frame f, int i) => i > f.Ac
        ? throw new ErrSaOor(f.Ac, i)
        : s.Vs[f.Sp - i];

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe Value SrGet(ref Stack s, ref Frame f, int i) => i + f.Sp > s.Sa
        ? throw new ErrSrOor(s.Sa - f.Sp, i)
        : s.Vs[f.Sp + i];

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe void SrSet(ref Stack s, ref Frame f, int i, Value v)
    {
        if (i + f.Sp > s.Sa) throw new ErrSrOor(s.Sa - f.Sp, i);
        // TODO: handle writer barrier
        s.Vs[i + f.Sp] = v;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe Value SrPop(ref Stack s, ref Frame f)
    {
        if (f.Sp < s.Sa) return s.Vs[--s.Sa];
        throw new ErrSrOor(0, 0);
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe void SrPush(ref Stack s, ref Frame f, Value v)
    {
        if (s.Sc <= s.Sa) throw new ErrSaOvf(s.Sc, s.Sa + 1);
        s.Vs[s.Sa++] = v;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe void FPush(ref Stack s, int args, nuint pp)
    {
        if (s.Fc == s.Fa) throw new StackOverflowException();
        // allocate the next frame
        s.Fs[s.Fa++] = new Frame
        {
            Sp = s.Sa,
            Ac = args,
            Rp = s.Pp
        };
        // args are already on stack so Sa does not need to be adjusted
        // set the current program pointer to target location
        s.Pp = pp;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe void FTail(ref Stack s, ref Frame f, int args, nuint pp)
    {
        // TODO: handle write barrier when clearing arguments
        // move call argument values to start of frame where arguments were placed
        var ab = f.Sp - f.Ac;
        var rb = f.Sp;

        // there must be more than one argument for the move to table place
        if (rb != ab)
        {
            for (var i = 0; i < args; ++i)
            {
                ref var r = ref s.Vs[rb + i];
                s.Vs[ab + i] = r;
            }
        }

        // adjust Sa
        s.Sa -= f.Ac;
        // allocate the next frame and get reference
        var rp = f.Rp;
        f = new Frame
        {
            Sp = s.Sa,
            Ac = args,
            Rp = rp
        };
        // args are already on stack so Sa does not need to be adjusted
        // set the current program pointer to target location
        s.Pp = pp;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private static unsafe void FPop(ref Stack s, ref Frame f, int args)
    {
        // TODO: handle write barrier when clearing arguments
        // move return values to start of frame where arguments were placed
        var ab = f.Sp - f.Ac;
        var rb = f.Sp;

        // there must be more than one argument for the move to table place
        if (rb != ab)
        {
            for (var i = 0; i < args; ++i)
            {
                ref var r = ref s.Vs[rb + i];
                s.Vs[ab + i] = r;
            }
        }

        // adjust Sa, restore Pp, clear and pop current frame
        s.Sa -= f.Ac;
        s.Pp = f.Rp;
        //f = new Frame();
        --s.Fa;
    }

    private static unsafe void PrintStack(ref Stack s)
    {
        for (var i = 0; i < s.Sa; ++i)
        {
            var v = s.Vs[i];
            if (v.T == BoolFalse.T) Console.Write($"B[{v.V == 1}] ");
            else if (v.T == IntZero.T) Console.Write($"I[{v.V}] ");
            else if (v.T == RealZero.T) Console.Write($"F[{BitConverter.Int64BitsToDouble(v.V)}] ");
            else Console.Write($"O[{v.V:x}] ");
        }

        Console.WriteLine();
    }
}