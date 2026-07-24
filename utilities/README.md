utilities
=========

Standalone tools written in pure mata-ll.

schema2mll.mll
--------------

Reads a [JSON Schema](https://json-schema.org/) on stdin and writes a mata-ll
data type — deriving `FromJSON` and `ToJSON` — on stdout.

```sh
target/release/mll -r utilities/schema2mll.mll [RootTypeName] < schema.json
```

The optional argument names the root type; without it the schema's `title` is
used, falling back to `Root`.

Mapping:

| JSON Schema                         | mata-ll                          |
|-------------------------------------|----------------------------------|
| object with `properties`            | a record data type               |
| field not in `required`             | `Maybe T`                        |
| `array` with `items`                | `[T]`                            |
| `string` / `integer` / `number` / `boolean` | `String` / `Int` / `Number` / `Bool` |
| nested object                       | its own named record             |
| `$ref: "#/.../Name"`                | the type `Name`                  |
| `definitions` / `$defs`             | one named type per entry         |
| free-form object / union / no `type`| `Json` (raw passthrough)         |

The derived instances use each record field's label as the JSON key verbatim,
so the label must equal the key. Haskell field labels are module-global, so two
generated records that share a key name collide and one must be renamed by hand.
Enum constraints are not encoded (an enum of strings stays `String`): mapping
values to constructors would need capitalisation and break the case-sensitive
round-trip.

Example:

```sh
$ echo '{"title":"Person","type":"object","required":["name"],
         "properties":{"name":{"type":"string"},"age":{"type":"integer"}}}' \
    | target/release/mll -r utilities/schema2mll.mll
```

```haskell
import JSON

data Person = Person
    { name :: String
    , age :: Maybe (Int)
    }
    deriving (Eq, Show, FromJSON, ToJSON)
```
