namespace Nova;

public partial class Engine
{
    private static readonly unsafe Type SProcType = new()
    {
        Get = &NullGet,
        Set = &NullSet,
        Unary = &NullUnary,
        Binary = &NullBinary,
        Call = &SFuncCall,
        Calli = &NullCalli
    };

    private struct SProcData
    {
        /// <summary> count of arguments </summary>
        public int Ac;

        /// <summary> count of return values </summary>
        public int Rc;
        
        /// <summary> program </summary>
        public nuint Pp;
    }

    private static unsafe void SFuncCall(ref Stack s, ref Frame f, Value o, byte args, bool isTailCall)
    {
        ref var data = ref *(SProcData*)o.V;
        // TODO: handle params package
        if (isTailCall)
        {
            FTail(ref s, ref f, args, data.Pp);
        }
        else
        {
            FPush(ref s, args, data.Pp);
        }
    }
}