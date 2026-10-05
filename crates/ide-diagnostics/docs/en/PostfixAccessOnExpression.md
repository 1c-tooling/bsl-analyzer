# Member access on a parenthesised expression, `Новый` or a literal (PostfixAccessOnExpression)

## Description

The platform does not compile a dot, an index or a call written directly after
an expression that is not a variable, a property or a method call:

- after a parenthesised expression: `(Ф).Размер()`, `(Новый Файл(Имя)).Размер()`,
  `(М)[0]`, `(Ф)(1)`, `(С).Свойство = 1;`;
- after the `Новый` operator in both of its forms: `Новый Массив(2)[0]`,
  `Новый("Массив", Параметры)[0]`, `Новый Массив[0]`, `Новый Массив(2)(1)`;
- after a literal: `"абв".Длина`, `"абв"[0]`, `Неопределено.Х`, `Истина[0]`,
  `'20200101'.Х`, `1[0]`;
- an index or a call after `?(…)`: `?(Условие, А, Б)[0]`, `?(Условие, А, Б)(1)`.

The compiler answers "Неопознанный оператор" (unrecognised operator) when the
construct starts a statement or follows `Возврат`, and "Ожидается символ ')'"
(`)` expected) when it is passed as an argument; the error points at the dot or
the bracket, or at the opening parenthesis when the construct starts a
statement. This is a compilation error, not a warning: the module does not
compile as a whole, and every method of it fails on first use, not only the
line with the mistake.

These constructs are legal in almost every other language, which is why the
mistake slips past a reader easily.

A dot after `?(…)` is accepted by the platform: `?(Условие, А, Б).Свойство` and
`?(Условие, А, Б).Метод()` compile, and the diagnostic leaves them alone.

A dot right after `Новый` (`Новый Файл(Имя).Размер()`,
`Новый("Массив").Количество()`) is reported by the parser itself
(`ParseError`), so this diagnostic does not repeat it.

## Examples

Incorrect:

```bsl
Возврат (Новый Файл(ИмяФайла)).Размер();
```

```bsl
Первый = Новый Массив(2)[0];
```

Correct:

```bsl
Файл = Новый Файл(ИмяФайла);
Возврат Файл.Размер();
```

## Sources

- Verified by compiling external data processors on platforms 8.3.17.1549 and
  8.3.27.2214 (compatibility mode 8.3.17): every form listed above is refused
  with the same text at the same position on both versions, while the same
  operation on a value held in a variable compiles.
