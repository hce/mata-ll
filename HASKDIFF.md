# Important differences to haskell to be aware of.

## Strings and ByteStrings

Haskell has

    type String = [Char]

while mata-ll uses Lua's built in string type to implement its own
String type. Just like haskell's String, mata-ll's String is with its
limitations. Unlike haskell's, though, mata-ll's String type will
remain the base type for string handling because it's cheap to do FFI
with Lua in that way.

Because of this property, you cannot use the concat operator (++) with
mata-ll strings; instead, use the Semigroup's (<>) operator like so:

    "Hello" <> " " <> "world"
