using System.Diagnostics;
using Nova;

const int FibN = 40;

var (pp, pe) = MakeProgram();
{
    var sw = Stopwatch.StartNew();
    Fib(FibN);
    Console.WriteLine(sw.ElapsedMilliseconds);
    sw.Stop();
}

{
    var sw = Stopwatch.StartNew();
    RunProgram(pp, pe);
    Console.WriteLine(sw.ElapsedMilliseconds);
    sw.Stop();
}
return;

static long Fib(int n)
{
    if (n <= 2) return 1;
    return Fib(n - 1) +  Fib(n - 2);
}

static void RunProgram(Engine.IProgram p, Engine.Value e)
{
    using var _ = p;
    var v = p.Call(e);
    //Console.WriteLine(v.V);
}

static (Engine.IProgram, Engine.Value) MakeProgram()
{
    using var b = Engine.CreateProgramBuilder();
    b.Tell(out var fibS);
    var fib = b.Procedure(fibS, 1, 1);
    b
        .AddLdA(1)
        .AddLi(2)
        .AddOpBi(Engine.OpBinary.Le)
        .AddJn(0)
        .AddLi(1)
        .AddRt(1)
        .Label(0)
        .AddLdA(1)
        .AddLi(1)
        .AddOpBi(Engine.OpBinary.Sub)
        .AddLiC(fib)
        .AddCall(1, false)
        .AddLdA(1)
        .AddLi(2)
        .AddOpBi(Engine.OpBinary.Sub)
        .AddLiC(fib)
        .AddCall(1, false)
        .AddOpBi(Engine.OpBinary.Add)
        .AddRt(1);
    b.Tell(out var mainS);
    var main = b.Procedure(mainS, 0, 1);
    b
        .AddLi(FibN)
        .AddLiC(fib)
        .AddCall(1, true);
    return (b.Build(), main);
}

// 1 1 2 3 5 8 13 21 34