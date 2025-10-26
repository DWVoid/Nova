namespace Lexical;

public class Syntax
{
    public interface ISelectionBuilder
    {
        /// <summary>
        /// Match the exact sequence given by text once and drop the token.
        /// </summary>
        ISelectionBuilder Branch(string text);

        /// <summary>
        /// Match the exact rule given once and drop the token.
        /// </summary>
        ISelectionBuilder BranchRule(string rule);
    }

    public interface ISequenceBuilder
    {
        /// <summary>
        /// If the current parsing is in a multi-branch choice,
        /// anchor this sequence as the branch chosen, and further mismatch will produce diagnostics error
        /// </summary>
        ISequenceBuilder Anchor();

        /// <summary>
        /// Match the exact sequence given by text once and drop the token.
        /// </summary>
        ISequenceBuilder Drop(string text);

        /// <summary>
        /// Match the exact rule given once and drop the token.
        /// </summary>
        ISequenceBuilder DropRule(string rule);

        /// <summary>
        /// Match the exact rule given at least min times and at most max times and drop the token.
        /// </summary>
        ISequenceBuilder DropRule(string rule, int min, int max);

        /// <summary>
        /// Match the exact sequence given by text once and store the token.
        /// </summary>
        ISequenceBuilder Keep(string text);

        /// <summary>
        /// Match the exact rule given once and store the token.
        /// </summary>
        ISequenceBuilder KeepRule(string rule);

        /// <summary>
        /// Match the exact rule given at least min times and at most max times and drop store the token(s) as array
        /// </summary>
        ISequenceBuilder KeepRule(string rule, int min, int max);

        /// <summary>
        /// Build the sequence given a mapper function
        /// </summary>
        /// <returns></returns>
        public void Build(Func<ReadOnlySpan<object>, object> map);
    }

    public interface IBuilder
    {
        ISelectionBuilder Selection(string name);

        ISequenceBuilder Sequence(string name);

        Func<string, Source, object> Build(params ReadOnlySpan<string> symbol);
    }
}