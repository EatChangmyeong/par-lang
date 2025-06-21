# The `let` Expression

Just one last stop before setting on a tour through Par's types and their expressions: the `let`
expression. It's for assigning a variable and using it in another expression.

Start with the keyword `let`, then a lower-case name of the variable, an `=` sign, a value
to assign to the variable, and finally the keyword `in` followed by an expression that may use
the variable.

That's a mouthful.

```par
def Six = let three = 3 in Nat.Add(three, three)
```

The left side of the `=` sign can actually be more than a variable!

For one, it can have an annotation:

```par
def Six = let three: Nat = 3 in Nat.Add(three, three)
```

And it can also be a pattern:

```par
def Twelve = let (a, b)! = (3, 4)! in Nat.Mul(a, b)
```

The above is a combination of a [pair](../types/pair.md) and a [unit](../types/unit.md) pattern.
We'll learn more about those soon.

> Type annotations always go after a variable name. So, this is invalid:
>
> ```par
> let (a, b)! : (Nat, Nat)! = (3, 4)! in ...  // Error!
> ```
>
> The annotation does not follow a variable. But this is good:
>
> ```par
> let (a: Nat, b: Nat)! = (3, 4) in ...      // Okay.
> ```

**Now, onto types and their expressions!**
