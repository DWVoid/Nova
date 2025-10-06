using System.Globalization;
using System.Runtime.InteropServices;

namespace Nova;

public partial class Compile
{
    // syntax representation of a translation (a source file, snippet, etc.)
    public class Unit
    {
    }

    // syntax representation of a block of statements
    public class Block
    {
    }

    // an abstract statement
    public interface IStmt
    {
    }

    // an abstract expression
    public interface IExpr
    {
    }

    /// <summary> Translate a given text unit into syntax representation </summary>
    /// <param name="text"> The text unit. Must be a valid sequence of Unicode grapheme clusters </param>
    /// <returns> The translated syntax </returns>
    public static Unit Translate(string text)
    {
        throw new NotImplementedException();
    }
}