# Method size (MethodSize)

<!-- Блоки выше заполняются автоматически, не трогать -->
## Description

A large method is harder to understand, test, and maintain.

Methods usually become too large when new logic keeps being added directly to
the same procedure or function instead of being extracted into smaller parts.

The diagnostic compares the method size with a configurable threshold:

- `maxMethodSize`, `200` by default;
- a method is reported when its size is strictly greater than the threshold
  (`size > maxMethodSize`); a size equal to the threshold is not reported.

The method size is the difference between the line numbers of the end and the
start of the method's syntax node: `S = line(end) − line(start)`. The node
starts at the first annotation, if any, and ends after `EndProcedure` /
`EndFunction`. It is neither an inclusive line count nor a statement count: a
one-line method has size 0, and annotations, comments, and blank lines inside
the node count. Lines outside the method node do not count.

For example, the method

```bsl
Procedure Test()
    A = 1;
EndProcedure
```

has size 2: with `maxMethodSize = 1` it gets one report with size 2, while a
two-line method (size 1) is not reported at the same threshold.

Earlier versions subtracted 4 from the size and skipped methods whose result
was zero, so methods shorter than five lines were never reported at any
threshold. The subtraction is gone: for methods of size 4 and above the computed
size is 4 greater than before. Methods that were reported before now show a
number 4 greater; methods of size `maxMethodSize + 1` to `maxMethodSize + 4`, and
short methods whose size exceeds the threshold, are now reported for the first
time.

Practical refactoring heuristics:

- if you want to add a comment explaining a code block, that block may deserve a
  separate method with a meaningful name;
- if one method performs several subtasks, split them into focused helper
  methods.

## Examples

The examples illustrate the refactoring; the comment about 200 lines in the
first one does not by itself make it a large method.

Invalid:

```bsl
Procedure ProcessDocument(Document)
    // 200 lines of validation, calculations, persistence, notifications
EndProcedure
```

Better:

```bsl
Procedure ProcessDocument(Document)
    ValidateDocument(Document);
    CalculateTotals(Document);
    SaveDocument(Document);
    NotifyUser(Document);
EndProcedure
```

## Sources

There is no normative 1C standard with a method size threshold; the default of
200 is a setting of this analyzer.

Related public context, not normative:

- [Martin Fowler: Refactoring](https://www.refactoring.com/)
- [Refactoring tools in 1C (RU)](https://v8.1c.ru/o7/201312ref/index.htm)
