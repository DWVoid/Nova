using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Nova;

public partial class Engine
{
    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct GcHead
    {
        // next field for GC collection process 
        public GcHead* NextA;

        // next field for GC marking process
        public GcHead* NextB;

        // GcObject VTable. This should not be null
        public GcVtb* Vt;

        // auxiliary data. only 32 bit is used. declared as nuint for alignment purpose
        // this allows actual data to be accessed by ((nint)this)+sizeof(GcHead)
        // Allocation (from LSB):
        // [00-03]: GC progression marker. always default to 0 (white)
        // [04-04]: if the object currently belongs to the blue list
        // [05-05]: if the object has been added to the blue list this GC cycle.
        // [06-06]: if the object has been added to the black list this GC cycle.
        // [07-31]: reserved
        public nuint Aux;

        public GcState State
        {
            get => (GcState)(Aux & 0xF);
            set => Aux = (Aux & 0xFFFFFFF0) | (uint)value;
        }

        public const uint BlueBit = 1u << 4;
        public const uint BluedBit = 1u << 5;
        public const uint BlackedBit = 1u << 6;
        public const uint GcBits = 0xFF;

        public bool IsBlue => (Aux & BlueBit) != 0;

        public bool IsBlued => (Aux & BluedBit) != 0;

        public bool IsBlacked => (Aux & BlackedBit) != 0;

        public void SetBits(uint bits) => Aux |= bits;

        public void ClearBits(uint bits) => Aux &= ~bits;
    }

    [StructLayout(LayoutKind.Sequential)]
    public unsafe struct GcVtb
    {
        // function to walk all referred object of the given object
        public delegate* managed<GcHead*, Action<Value>, void> Scan;

        // function to implement custom move behavior of given object
        public delegate* managed<GcHead*, GcHead*, void> Move;

        // function to release all associated resources of given object
        public delegate* managed<GcHead*, void> Free;

        // reference to type to avoid storing full Value
        public Type* Next;
    }

    private struct GcListA()
    {
        private unsafe GcHead* _h = null, _t = null;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe void Push(GcHead* o)
        {
            if (_t == null)
                _h = o;
            else
                _t->NextA = o;
            _t = o;
            o->NextA = null;
        }


        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe GcHead* Pop()
        {
            var r = _h;
            if (r == null) return null;
            var n = r->NextA;
            _h = n;
            if (n == null) _t = null;
            return r;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe bool Empty() => _h == null;
    }

    private struct GcListB()
    {
        private unsafe GcHead* _h = null, _t = null;

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe void Push(GcHead* o)
        {
            if (_t == null)
                _h = o;
            else
                _t->NextB = o;
            _t = o;
            o->NextB = null;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe GcHead* Pop()
        {
            var r = _h;
            if (r == null) return null;
            var n = r->NextB;
            _h = n;
            if (n == null) _t = null;
            return r;
        }

        [MethodImpl(MethodImplOptions.AggressiveInlining)]
        public unsafe bool Empty() => _h == null;
    }

    public enum GcState
    {
        White = 0, // unmarked object
        Gray = 1, //  reachable from root, some child is neither Gray nor Black
        Black = 2 //  reachable from root, all children are Gray or Black
    }

    private GcListB _gcGray = new(), _gcBlack = new(), _gcFin = new();
    private GcListA _gcBlue = new(), _gcHold = new();
    private bool _bWrite;

    public unsafe nint New(nint len)
    {
        return (nint)NativeMemory.AllocZeroed((nuint)len);
    }

    public unsafe void Free(nint ptr)
    {
        NativeMemory.Free((void*)ptr);
    }

    // allocate a new object on heap
    public unsafe void* GcNew(nint len, GcVtb* vtb)
    {
        var mem = New(len + sizeof(GcHead));
        *(GcHead*)mem = new GcHead { Vt = vtb };
        return (void*)(mem + sizeof(GcHead));
    }

    // replace GcObject o with GcObject n
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public unsafe void GcAssign(GcHead* o, GcHead* n)
    {
        if (o == n) return;
        if (n != null) n->ClearBits(GcHead.BlueBit);
        if (o == null) return;
        if (o->IsBlue) return;
        if (!o->IsBlued) _gcBlue.Push(o);
        o->SetBits(GcHead.BlueBit | GcHead.BluedBit);
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private unsafe void GcAssignOnWriteBarrier(GcHead* t, GcHead* o, GcHead* n)
    {
        // do nothing if the old node is white. if it is either Gray or Black, further check is needed
        if (o != null && o->State is GcState.White) return;
        if (n == null) return;
        // white on new could break invariant, check if t is black. if it is, reset to grey for rescan
        if (n->State is not GcState.White) return;
        if (t->State is not GcState.Black) return;
        t->State = GcState.Gray;
        _gcGray.Push(t);
    }

    // replace GcObject o in GcObject t with GcObject n
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public unsafe void GcAssign(GcHead* t, GcHead* o, GcHead* n)
    {
        GcAssign(o, n);
        if (_bWrite) GcAssignOnWriteBarrier(t, o, n);
    }

    private unsafe void WhiteToGray(Value v)
    {
        // consult the type info to see if this object is on heap
        if (!v.T->IsHeap(v)) return;
        var p = (GcHead*)(nuint)(v.V - sizeof(GcHead));
        // transfer all white state objects to gray
        if (p->State is not GcState.White) return;
        p->State = GcState.Gray;
        _gcGray.Push(p);
    }

    private unsafe void GcProcessGrayList()
    {
        while (true)
        {
            var n = _gcGray.Pop();
            if (n == null) break;
            // call the scan function if any
            var vt = n->Vt;
            if (vt->Scan != null) vt->Scan(n, WhiteToGray);
            // all direct descendents enqueued, mark the current object black and also clear the blue flag if any
            // the reason why this list is kept is that we need it to be reset to white before next cycle
            n->State = GcState.Black;
            n->ClearBits(GcHead.BlueBit);
            if (n->IsBlacked) continue;
            n->SetBits(GcHead.BlackedBit);
            _gcBlack.Push(n);
        }
    }

    private unsafe void WhiteToBlue(Value v)
    {
        // consult the type info to see if this object is on heap
        if (!v.T->IsHeap(v)) return;
        var p = (GcHead*)(nuint)(v.V - sizeof(GcHead));
        // only mark white objects. gray and black are not to be released in this GC trigger
        if (p->State is not GcState.White) return;
        // only mark and add to list if blue flag is not set and blue in-list flag is not set
        if (p->IsBlue) return;
        if (!p->IsBlued) _gcBlue.Push(p);
        p->SetBits(GcHead.BlueBit | GcHead.BluedBit);
    }

    private unsafe void GcProcessBlueList()
    {
        while (true)
        {
            var n = _gcBlue.Pop();
            if (n == null) break;

            // check if the blue flag is currently set.
            // if it has been reset (not on currently) skip the item
            // whatever has the flag on should be unassigned this cycle and not marked during primary collect
            if (!n->IsBlue)
            {
                n->ClearBits(GcHead.BluedBit);
                continue;
            }

            var vt = n->Vt;
            // check if this object requires finalization. if so, add it to list and continue to next
            // the gray list Next is used here as the blue list is still in use here
            if (vt->Next->Final != null)
            {
                _gcFin.Push(n);
                // the blue list flags are retained to check for prevent re-adding to blue list
                // while retaining the ability to check if the object is revived during finalization
                // no need to do anything further here
                continue;
            }

            // we have no finalizer tasks here
            // mark all its descendents if needed
            if (vt->Scan != null) vt->Scan(n, WhiteToBlue);
            // free all internalized resource of this object
            if (vt->Free != null) vt->Free(n);

            // if it reaches here, we can be sure the object itself is safe to be released
            Free((nint)n);
        }
    }

    private unsafe void FreeObjectVt(GcHead* n)
    {
        var vt = n->Vt;
        // mark all its descendents if needed
        if (vt->Scan != null) vt->Scan(n, WhiteToBlue);
        // free all internalized resource of this object
        if (vt->Free != null) vt->Free(n);
        // kill the object storage
        Free((nint)n);
    }

    private unsafe void GcProcessFinalizeReviveRescan()
    {
        while (true)
        {
            var n = _gcHold.Pop();
            if (n == null) break;
            if (n->State is not GcState.Black)
                FreeObjectVt(n);
            else
                // clear the blue in-list flag
                n->Aux &= 0xFFFFFFFD;
        }
    }

    private unsafe void GcResetBlackList()
    {
        while (true)
        {
            var n = _gcBlack.Pop();
            if (n == null) break;
            n->ClearBits(GcHead.GcBits);
        }
    }

    // recursively scan roots and collect objects using BFS
    public unsafe void GcGc(Func<nuint, Task> runFin)
    {
        _bWrite = true;
        // process whatever is left in the gray list
        GcProcessGrayList();
        while (true)
        {
            // process the blue list
            GcProcessBlueList();
            // check if there is any finalization pending. if there is none then we are done here
            if (_gcFin.Empty()) break;
            // process the finalization list in order
            while (true)
            {
                var n = _gcFin.Pop();
                if (n == null) break;
                // TODO: actually fix this to be proper await
                // (or handled via state machine as GC cannot have two run instances)
                runFin((nuint)n).Wait();
                // check if the object has been possibly revived by checking if its blue bit is cleared.
                // if this happened, the object it has been assigned to would be marked for rescan (gray)
                // object is fin list cannot become gray since none of this is now Black, so no chain conflict
                // since the in-list flag is retained from blue processing, it cannot be on blue-list now
                // we can put it on a separate hold list using BlNext to check for live after next gray scan
                if (!n->IsBlue)
                    _gcHold.Push(n);
                else
                    FreeObjectVt(n);
            }

            // now run the gray list scan again to tie the ends of finalization
            GcProcessGrayList();
            // process the hold list to pick out fake revives and kill the objects (not black after gray scan)
            GcProcessFinalizeReviveRescan();
            // now we are back to square one with a packed blue list anc clear gray list
        }

        GcResetBlackList();
        _bWrite = false;
    }
}