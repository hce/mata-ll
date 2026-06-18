# Important differences to haskell to be aware of.

## Strings and ByteStrings

Haskell has:

    type String = [Char]

Haskell discourages the use of String for most real world uses.
mata-ll doesn't use a type alias to [Char] but uses Lua's string type.
This is a good tradeoff as haskell discourages the use of String
anyway. By using Lua's string type internally, we can get speedups
compared to [Char] while also staying compatible with the Lua host
language.

Since mata-ll Strings are not lists, you cannot use the concatenate
operator (++); instead, use the Semigroup's (<>) operator, like so:

    "Hello" <> " " <> "world"

mata-ll also has a ByteString type. Only strict ByteStrings are
supported; haskell is slowly deprecating lazy ByteStrings anyway.
ByteString and String shares the same type in Lua, string. string in
Lua is really a ByteString with no encoding awareness.

A proper Text type in mata-ll is planned for later.
