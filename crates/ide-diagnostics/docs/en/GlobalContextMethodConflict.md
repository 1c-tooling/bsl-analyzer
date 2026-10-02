# Form method name conflicts with a global function (GlobalContextMethodConflict)

A form module cannot declare a method with the name of a platform global function: the platform rejects the form during compilation. This diagnostic checks the current platform catalog and points at the method name.

```bsl
Function BriefErrorDescription(Start, End)
    Return Start;
EndFunction
```

Rename the form method. The check is limited to form modules; one matching name does not by itself prohibit declarations in other module types.
