using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Nova;

public partial class Engine
{
    private enum Op
    {
        LiT, // load immediate true
        LiF, // load immediate false
        LiZ, // load immediate zero
        LiI1, // load immediate i1b
        LiI2, // load immediate i2b
        LiI4, // load immediate i4b
        LiI8, // load immediate i8b
        LiF4, // load immediate f4b
        LiF8, // load immediate f8b
        LcPv, // load constant from pointer to value
        LdR, // load from register 
        LdA, // load from argument
        StR, // save to register
        LdT, // load value from object
        LdTr, // load value from object raw
        StT, // store value to object
        StTr, // store value to object raw
        OpUn, // call unary operator
        OpBi, // call binary operator
        Call, // object call
        CallT, // object tail call
        Calli, // named object index call
        CalliT, // named object index tail call
        J, // jump
        Jn, // jump if false
        Rt, // return
        Pop // pop from stack
    }

    private interface IOp<T> where T : unmanaged
    {
        static abstract void Do(ref Stack s, ref Frame f, ref T i);
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct OpLiT() : IOp<OpLiT>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLiT i)
        {
            SrPush(ref s, ref f, BoolTrue);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct OpLiF() : IOp<OpLiF>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLiF i)
        {
            SrPush(ref s, ref f, BoolFalse);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct OpLiZ() : IOp<OpLiZ>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLiZ i)
        {
            SrPush(ref s, ref f, Make(in IntType, 0));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLiI<T>() : IOp<OpLiI<T>> where T : unmanaged, IConvertible
    {
        public byte O = 0;

        public T V = default;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLiI<T> i)
        {
            SrPush(ref s, ref f, Make(in IntType, i.V.ToInt64(null)));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLiF<T>() : IOp<OpLiF<T>> where T : unmanaged, IConvertible
    {
        public byte O = 0;

        public T V = default;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLiF<T> i)
        {
            SrPush(ref s, ref f, Make(in RealType, BitConverter.DoubleToInt64Bits(i.V.ToDouble(null))));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLcPv() : IOp<OpLcPv>
    {
        public byte O = 0;

        public Value V = default;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLcPv i)
        {
            SrPush(ref s, ref f, i.V);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLdR() : IOp<OpLdR>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLdR i)
        {
            SrPush(ref s, ref f, SrGet(ref s, ref f, i.I));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLdA() : IOp<OpLdA>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpLdA i)
        {
            SrPush(ref s, ref f, SaGet(ref s, ref f, i.I));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpStR() : IOp<OpStR>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpStR i)
        {
            SrSet(ref s, ref f, i.I, SrPop(ref s, ref f));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLdT() : IOp<OpLdT>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpLdT i)
        {
            var n = SrPop(ref s, ref f);
            var t = SrPop(ref s, ref f);
            SrPush(ref s, ref f, t.T->Get(ref s, ref f, t, n, false));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpLdTr() : IOp<OpLdTr>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpLdTr i)
        {
            var n = SrPop(ref s, ref f);
            var t = SrPop(ref s, ref f);
            SrPush(ref s, ref f, t.T->Get(ref s, ref f, t, n, true));
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpStT() : IOp<OpStT>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpStT i)
        {
            var v = SrPop(ref s, ref f);
            var n = SrPop(ref s, ref f);
            var t = SrPop(ref s, ref f);
            t.T->Set(ref s, ref f, t, n, false, v);
        }
    }


    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpStTr() : IOp<OpStTr>
    {
        public byte O = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpStTr i)
        {
            var v = SrPop(ref s, ref f);
            var n = SrPop(ref s, ref f);
            var t = SrPop(ref s, ref f);
            t.T->Set(ref s, ref f, t, n, true, v);
        }
    }

    private sealed class ErrExecNoOp : Exception
    {
        public override string Message => "Operation not found operands";
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpUn() : IOp<OpUn>
    {
        public byte O = 0;

        public OpUnary S = OpUnary.Neg;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpUn i)
        {
            var v = SrPop(ref s, ref f);
            if (!v.T->Unary(ref s, ref f, v, i.S, out var r))
                throw new ErrExecNoOp();
            SrPush(ref s, ref f, r);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpBi() : IOp<OpBi>
    {
        public byte O = 0;

        public OpBinary S = OpBinary.Add;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpBi i)
        {
            var r = SrPop(ref s, ref f);
            var l = SrPop(ref s, ref f);
            if (!l.T->Binary(ref s, ref f, l, r, i.S, out var v))
                if (!r.T->Binary(ref s, ref f, r, l, i.S, out v))
                    throw new ErrExecNoOp();
            SrPush(ref s, ref f, v);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpCall() : IOp<OpCall>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpCall i)
        {
            var iv = SrPop(ref s, ref f);
            iv.T->Call(ref s, ref f, iv, i.I, false);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpCallT() : IOp<OpCallT>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpCallT i)
        {
            var iv = SrPop(ref s, ref f);
            iv.T->Call(ref s, ref f, iv, i.I, true);
        }
    }


    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpCalli() : IOp<OpCalli>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpCalli i)
        {
            var id = SrPop(ref s, ref f);
            var iv = SrPop(ref s, ref f);
            iv.T->Calli(ref s, ref f, iv, id, i.I, false);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpCalliT() : IOp<OpCalliT>
    {
        public byte O = 0;

        public byte I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpCalliT i)
        {
            var id = SrPop(ref s, ref f);
            var iv = SrPop(ref s, ref f);
            iv.T->Calli(ref s, ref f, iv, id, i.I, true);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpRt() : IOp<OpRt>
    {
        public byte O = 0;

        public byte I = 0;

        public static void Do(ref Stack s, ref Frame f, ref OpRt i)
        {
            FPop(ref s, ref f, i.I);
        }
    }


    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpPop() : IOp<OpPop>
    {
        public byte O = 0;

        public byte I = 0;

        public static void Do(ref Stack s, ref Frame f, ref OpPop i)
        {
            for (var x = 0; x < i.I; ++x) SrPop(ref s, ref f);
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpJ() : IOp<OpJ>
    {
        public byte O = 0;

        public int I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static void Do(ref Stack s, ref Frame f, ref OpJ i)
        {
            s.Pp += (nuint)i.I;
        }
    }

    [StructLayout(LayoutKind.Sequential, Pack = 1)]
    private struct OpJn() : IOp<OpJn>
    {
        public byte O = 0;

        public int I = 0;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public static unsafe void Do(ref Stack s, ref Frame f, ref OpJn i)
        {
            var v = SrPop(ref s, ref f);
            if (v.T == BoolFalse.T && v.V == 0) s.Pp += (nuint)i.I;
        }
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static unsafe void DoOp<T>(ref Stack s, ref Frame f, nuint pp) where T : unmanaged, IOp<T>
    {
        s.Pp = pp + (nuint)sizeof(T);
        T.Do(ref s, ref f, ref Unsafe.AsRef<T>((void*)pp));
    }

    private static unsafe void Dispatch(ref Stack s, ref Frame f, nuint pp)
    {
        var op = (Op)(*(byte*)pp);
        switch (op)
        {
            case Op.LiT:
                DoOp<OpLiT>(ref s, ref f, pp);
                break;
            case Op.LiF:
                DoOp<OpLiF>(ref s, ref f, pp);
                break;
            case Op.LiZ:
                DoOp<OpLiZ>(ref s, ref f, pp);
                break;
            case Op.LiI1:
                DoOp<OpLiI<sbyte>>(ref s, ref f, pp);
                break;
            case Op.LiI2:
                DoOp<OpLiI<short>>(ref s, ref f, pp);
                break;
            case Op.LiI4:
                DoOp<OpLiI<int>>(ref s, ref f, pp);
                break;
            case Op.LiI8:
                DoOp<OpLiI<long>>(ref s, ref f, pp);
                break;
            case Op.LiF4:
                DoOp<OpLiF<float>>(ref s, ref f, pp);
                break;
            case Op.LiF8:
                DoOp<OpLiF<double>>(ref s, ref f, pp);
                break;
            case Op.LcPv:
                DoOp<OpLcPv>(ref s, ref f, pp);
                break;
            case Op.LdR:
                DoOp<OpLdR>(ref s, ref f, pp);
                break;
            case Op.LdA:
                DoOp<OpLdA>(ref s, ref f, pp);
                break;
            case Op.StR:
                DoOp<OpStR>(ref s, ref f, pp);
                break;
            case Op.LdT:
                DoOp<OpLdT>(ref s, ref f, pp);
                break;
            case Op.LdTr:
                DoOp<OpLdTr>(ref s, ref f, pp);
                break;
            case Op.StT:
                DoOp<OpStT>(ref s, ref f, pp);
                break;
            case Op.StTr:
                DoOp<OpStTr>(ref s, ref f, pp);
                break;
            case Op.OpUn:
                DoOp<OpUn>(ref s, ref f, pp);
                break;
            case Op.OpBi:
                DoOp<OpBi>(ref s, ref f, pp);
                break;
            case Op.Call:
                DoOp<OpCall>(ref s, ref f, pp);
                break;
            case Op.CallT:
                DoOp<OpCallT>(ref s, ref f, pp);
                break;
            case Op.Calli:
                DoOp<OpCalli>(ref s, ref f, pp);
                break;
            case Op.CalliT:
                DoOp<OpCalliT>(ref s, ref f, pp);
                break;
            case Op.Rt:
                DoOp<OpRt>(ref s, ref f, pp);
                break;
            case Op.J:
                DoOp<OpJ>(ref s, ref f, pp);
                break;
            case Op.Jn:
                DoOp<OpJn>(ref s, ref f, pp);
                break;
            case Op.Pop:
                DoOp<OpPop>(ref s, ref f, pp);
                break;
            default:
                throw new ErrExecNoOp();
        }
    }

    public interface IProgram : IDisposable
    {
        Value Call(Value p);
    }

    public interface IProgramBuilder : IDisposable
    {
        Value Constant(in Type type, int size);
        Value Procedure(int start, int args, int rets);
        IProgramBuilder Label(int index);
        IProgramBuilder AddLi(bool value);
        IProgramBuilder AddLi(long value);
        IProgramBuilder AddLi(double value);
        IProgramBuilder AddLiC(Value value);
        IProgramBuilder AddLdR(int index);
        IProgramBuilder AddStR(int index);
        IProgramBuilder AddLdA(int index);
        IProgramBuilder AddLdT(bool raw);
        IProgramBuilder AddStR(bool raw);
        IProgramBuilder AddOpUn(OpUnary op);
        IProgramBuilder AddOpBi(OpBinary op);
        IProgramBuilder Tell(out int offset);
        IProgramBuilder AddCall(int args, bool tail);
        IProgramBuilder AddCalli(int args, bool tail);
        IProgramBuilder AddRt(int args);
        IProgramBuilder AddJ(int label);
        IProgramBuilder AddJn(int label);
        IProgramBuilder AddPop(int count);
        IProgram Build();
    }

    private sealed class Program(nuint text, nuint data) : IProgram
    {
        private nuint _text = text, _data = data;

        public unsafe void Dispose()
        {
            NativeMemory.Free((void*)_text);
            for (var c = _data; c != 0;)
            {
                (c, var r) = ((nuint)(*(long*)c), c);
                NativeMemory.Free((void*)r);
            }

            _text = 0;
            _data = 0;
        }

        public unsafe Value Call(Value p)
        {
            var vs = NativeMemory.Alloc(32000, (nuint)sizeof(Value));
            var fs = NativeMemory.Alloc(1024, (nuint)sizeof(Frame));
            try
            {
                var s = new Stack
                {
                    Vs = (Value*)vs,
                    Fs = (Frame*)fs,
                    Sa = 0,
                    Sc = 32000,
                    Fa = 0,
                    Fc = 1024,
                    Pp = 0
                };
                ref var pv = ref *(SProcData*)p.V;
                FPush(ref s, pv.Ac, pv.Pp);
                while (s.Pp != 0)
                {
                    Dispatch(ref s, ref s.Fs[s.Fa - 1], s.Pp);
                    //PrintStack(ref s);
                }

                return s.Vs[0];
            }
            finally
            {
                NativeMemory.Free(fs);
                NativeMemory.Free(vs);
            }
        }
    }

    private sealed class ProgramBuilder : IProgramBuilder
    {
        private readonly List<byte> _code = []; // list of instructions
        private readonly List<Value> _sps = []; // list of functions. need to rewrite Pp on build
        private readonly List<int> _labels = []; // list of actual labels
        private readonly List<int> _resolve = []; //list of offsets in code to resolve to labels
        private nuint _head, _alloc;
        private long _space;

        private static int Align(int size)
        {
            var (q, r) = Math.DivRem(size, 8);
            if (r != 0) ++q;
            return q * 8;
        }

        public unsafe void Dispose()
        {
            for (var c = _head; c != 0;)
            {
                (c, var r) = ((nuint)(*(long*)c), c);
                NativeMemory.Free((void*)r);
            }

            _head = 0;
        }

        private void PushOp<T>(T v) where T : unmanaged, IOp<T>
        {
            _code.AddRange(MemoryMarshal.Cast<T, byte>(MemoryMarshal.CreateSpan(ref v, 1)));
        }

        unsafe Value IProgramBuilder.Constant(in Type type, int size)
        {
            size = Align(size);
            if (_space < size)
            {
                var alloc = Math.Max(65536, size + 8);
                var mem = NativeMemory.Alloc((nuint)alloc);
                *(long*)mem = (long)_head;
                _head = (nuint)mem;
                _alloc = _head + 8;
                _space = alloc - 8;
            }

            var res = Make(in type, (long)_alloc);
            _alloc += (nuint)size;
            _space -= size;
            return res;
        }

        unsafe Value IProgramBuilder.Procedure(int start, int args, int rets)
        {
            var v = ((IProgramBuilder)this).Constant(in SProcType, sizeof(SProcData));
            _sps.Add(v);
            *(SProcData*)v.V = new SProcData { Ac = args, Rc = rets, Pp = (nuint)start };
            return v;
        }

        IProgramBuilder IProgramBuilder.Label(int label)
        {
            while (_labels.Count <= label) _labels.Add(0);
            _labels[label] = _code.Count;
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLi(bool value)
        {
            if (value)
                PushOp(new OpLiT { O = (byte)Op.LiT });
            else
                PushOp(new OpLiF { O = (byte)Op.LiF });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLi(long value)
        {
            if (value == 0)
                PushOp(new OpLiZ());
            else if (value is >= sbyte.MinValue and <= sbyte.MaxValue)
                PushOp(new OpLiI<sbyte> { O = (byte)Op.LiI1, V = (sbyte)value });
            else if (value is >= short.MinValue and <= short.MaxValue)
                PushOp(new OpLiI<short> { O = (byte)Op.LiI2, V = (short)value });
            else if (value is >= int.MinValue and <= int.MaxValue)
                PushOp(new OpLiI<int> { O = (byte)Op.LiI4, V = (int)value });
            else
                PushOp(new OpLiI<long> { O = (byte)Op.LiI8, V = value });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLi(double value)
        {
            if (value is >= float.MinValue and <= float.MaxValue)
                PushOp(new OpLiF<float> { O = (byte)Op.LiI1, V = (float)value });
            else
                PushOp(new OpLiF<double> { O = (byte)Op.LiI2, V = value });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLiC(Value value)
        {
            PushOp(new OpLcPv { O = (byte)Op.LcPv, V = value });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLdR(int index)
        {
            PushOp(new OpLdR { O = (byte)Op.LdR, I = (byte)index });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddStR(int index)
        {
            PushOp(new OpStR { O = (byte)Op.StR, I = (byte)index });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLdA(int index)
        {
            PushOp(new OpLdA { O = (byte)Op.LdA, I = (byte)index });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddLdT(bool raw)
        {
            if (raw)
                PushOp(new OpLdT { O = (byte)Op.LdT });
            else
                PushOp(new OpLdTr { O = (byte)Op.LdTr });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddStR(bool raw)
        {
            if (raw)
                PushOp(new OpStT { O = (byte)Op.StT });
            else
                PushOp(new OpStTr { O = (byte)Op.StTr });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddOpUn(OpUnary op)
        {
            PushOp(new OpUn { O = (byte)Op.OpUn, S = op });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddOpBi(OpBinary op)
        {
            PushOp(new OpBi { O = (byte)Op.OpBi, S = op });
            return this;
        }

        IProgramBuilder IProgramBuilder.Tell(out int offset)
        {
            offset = _code.Count;
            return this;
        }

        IProgramBuilder IProgramBuilder.AddCall(int args, bool tail)
        {
            if (!tail)
                PushOp(new OpCall { O = (byte)Op.Call, I = (byte)args });
            else
                PushOp(new OpCallT { O = (byte)Op.CallT, I = (byte)args });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddCalli(int args, bool tail)
        {
            if (!tail)
                PushOp(new OpCalli { O = (byte)Op.Calli, I = (byte)args });
            else
                PushOp(new OpCalliT { O = (byte)Op.CalliT, I = (byte)args });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddRt(int args)
        {
            PushOp(new OpRt { O = (byte)Op.Rt, I = (byte)args });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddJ(int label)
        {
            _resolve.Add(_code.Count);
            PushOp(new OpJ { O = (byte)Op.J, I = label });
            return this;
        }

        IProgramBuilder IProgramBuilder.AddJn(int label)
        {
            _resolve.Add(_code.Count);
            PushOp(new OpJn { O = (byte)Op.Jn, I = label });
            return this;
        }

        public IProgramBuilder AddPop(int count)
        {
            PushOp(new OpPop { O = (byte)Op.Pop, I = (byte)count });
            return this;
        }

        unsafe IProgram IProgramBuilder.Build()
        {
            var text = (byte*)NativeMemory.Alloc((nuint)_code.Count);
            _code.CopyTo(new Span<byte>(text, _code.Count));
            // rewrite all procedure references
            foreach (var sp in _sps) ((SProcData*)sp.V)->Pp += (nuint)text;
            (var data, _head) = (_head, 0);
            // rewrite all labels
            foreach (var res in _resolve)
            {
                var pos = (nuint)text + (nuint)res;
                ref var j = ref *(OpJ*)pos;
                j.I = (int)(text + (nuint)_labels[j.I] - (pos + (nuint)sizeof(OpJ)));
            }

            return new Program((nuint)text, data);
        }
    }

    public static IProgramBuilder CreateProgramBuilder() => new ProgramBuilder();
}