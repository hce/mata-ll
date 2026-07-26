-- FFI marshalling of the dynamic `Any` type in BOTH directions, with values
-- that cross the Lua boundary rather than literals. `Any` is opaque inside
-- mata-ll (a tagged ADT), but at the FFI boundary mata-ll tags a host scalar on
-- the way in and untags it to a plain scalar on the way out — the host only
-- ever sees Lua strings/numbers/booleans/nil, never the `{tag, payload}` table.
--
-- Hosts are Lua stdlib functions chosen for their observable behaviour:
--   tonumber : String -> number|nil       — a value CONSTRUCTED by the host,
--                                            decoded into the right constructor
--                                            (integer vs float vs nil split).
--   type     : Any    -> "string"|...     — reports the plain Lua type it was
--                                            actually handed, proving the untag.
--   table.concat : [Any] -> String        — rejects a table element, so a list
--                                            of Any that failed to untag would
--                                            fail loudly.

-- Host produces the value; the declared `Any` result tags it.
toNum   :: String -> LuaPure "tonumber" Any
-- Host inspects the value; the `Any` argument is untagged to a bare scalar.
luaType :: Any -> LuaPure "type" String
-- A list of Any marshalled out element-by-element to a native-string-only host.
joinAny :: [Any] -> String -> LuaPure "table.concat" String

describeAny :: Any -> String
describeAny (AnyString s)  = "string:" <> s
describeAny (AnyInt n) = "integer:" <> show n
describeAny (AnyNumber n)  = "number:" <> show n
describeAny (AnyBool b)    = "bool:" <> show b
describeAny AnyNull        = "null"

main :: IO ()
main = do
    -- Host -> mata-ll DECODE. tonumber builds the number/nil, so the scalar is
    -- genuinely produced across the boundary, not a literal wrapped after the
    -- fact. The integer/float split is driven by the host's own value.
    assert (describeAny (toNum "42") == "integer:42")
        "a whole number crossing as Any decodes to AnyInt"
    assert (describeAny (toNum "3.5") == "number:3.5")
        "a fractional number crossing as Any decodes to AnyNumber"
    assert (describeAny (toNum "not a number") == "null")
        "a nil crossing as Any decodes to AnyNull"

    -- mata-ll -> host MARSHAL. The host `type` reports the plain Lua type it was
    -- handed, so it observes the untagged scalar, never the ADT table (which
    -- would report "table" and fail every assert below).
    assert (luaType (AnyString "hi") == "string")
        "AnyString reaches the host as a plain Lua string"
    assert (luaType (AnyInt 7) == "number")
        "AnyInt reaches the host as a plain Lua number"
    assert (luaType (AnyNumber 1.5) == "number")
        "AnyNumber reaches the host as a plain Lua number"
    assert (luaType (AnyBool False) == "boolean")
        "AnyBool reaches the host as a plain Lua boolean (false, not nil)"
    assert (luaType AnyNull == "nil")
        "AnyNull reaches the host as plain nil"

    -- Nesting: a [Any] built from mixed constructors marshalled out. Each
    -- element is untagged, so table.concat (which raises on a table element or
    -- an unforced thunk) sees only native strings and numbers.
    let xs = [AnyString "a", AnyInt 1, AnyNumber 2.5]
    assert (joinAny xs "-" == "a-1-2.5")
        "a [Any] marshals element-by-element to plain scalars"

    -- Round-trip identity: marshal each Any out to `type`, and its constructor
    -- name matches the plain Lua type the host reports.
    assert (luaType (toNum "100") == "number")
        "decode then re-marshal (Int): host sees a plain number again"
    assert (luaType (toNum "x") == "nil")
        "decode then re-marshal (Null): host sees plain nil again"

    putStrLn "any ffi marshalling ok"
