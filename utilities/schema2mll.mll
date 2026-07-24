-- schema2mll — read a JSON Schema on stdin, emit a mata-ll data type that
-- derives FromJSON and ToJSON, on stdout.
--
-- USAGE
--   target/release/mll -r utilities/schema2mll.mll [RootTypeName] < schema.json
--
--   The optional program argument names the root type. Without it, the
--   schema's "title" is used, falling back to "Root".
--
-- WHAT IT UNDERSTANDS
--   * object with "properties"  -> a record data type (one field per property)
--   * "required"                -> non-required fields become  Maybe T
--   * array with "items"        -> [T]
--   * string/integer/number/boolean -> String / Int / Number / Bool
--   * nested objects            -> their own named record, referenced by field
--   * "$ref": "#/.../Name"      -> the type  Name
--   * "definitions" / "$defs"   -> a named type per entry (so $ref resolves)
--   * anything else (free-form object, union type, missing "type") -> Json
--     (the raw-value passthrough, kept verbatim by the JSON module)
--
-- LIMITATION
--   The derived FromJSON/ToJSON use each record field's label as the JSON key
--   verbatim, so the label MUST equal the key. Haskell field labels are
--   module-global, so two generated records that share a key name collide and
--   one must be renamed by hand. Enum constraints are not encoded (an enum of
--   strings stays `String`), because capitalising values to constructors would
--   break the case-sensitive round-trip.

import JSON
import LIO
import LString (strByte, strLen, strSub, strChar, strToInts)

-- === A rendered field: label and its mata-ll type expression ===============

data Field = Field String String

-- === Identifier hygiene ====================================================

-- Keep only bytes valid in a mata-ll identifier: 0-9, A-Z, a-z, underscore.
keepIdentByte :: Int -> Bool
keepIdentByte b =
    (b >= 48 && b <= 57) || (b >= 65 && b <= 90) ||
    (b >= 97 && b <= 122) || b == 95

keepChar :: Int -> String
keepChar b = if keepIdentByte b then strChar b else ""

sanitize :: String -> String
sanitize s = mconcat (map keepChar (strToInts s))

-- Uppercase / lowercase a single ASCII byte.
upByte :: Int -> String
upByte b = if b >= 97 && b <= 122 then strChar (b - 32) else strChar b

downByte :: Int -> String
downByte b = if b >= 65 && b <= 90 then strChar (b + 32) else strChar b

capitalize :: String -> String
capitalize s =
    if strLen s == 0 then s
    else upByte (strByte s 1) <> strSub s 2 (strLen s)

lowerFirst :: String -> String
lowerFirst s =
    if strLen s == 0 then s
    else downByte (strByte s 1) <> strSub s 2 (strLen s)

isLetterByte :: Int -> Bool
isLetterByte b = (b >= 65 && b <= 90) || (b >= 97 && b <= 122)

-- A type/constructor name: sanitized, capitalized, guaranteed to start with
-- an uppercase letter (empty or digit-leading names get a "T" prefix).
typeIdent :: String -> String
typeIdent s =
    let c = capitalize (sanitize s)
    in if strLen c == 0 then "T"
       else if strByte c 1 >= 65 && strByte c 1 <= 90 then c
       else "T" <> c

-- A record field label: sanitized, lowercase-first, guaranteed to start with a
-- lowercase letter or underscore (empty -> "field", digit-leading -> "f_...").
fieldIdent :: String -> String
fieldIdent s =
    let c = lowerFirst (sanitize s)
    in if strLen c == 0 then "field"
       else let b = strByte c 1
            in if (b >= 97 && b <= 122) || b == 95 then c else "f_" <> c

-- === Schema inspection helpers =============================================

schemaTypeTag :: Json -> Maybe String
schemaTypeTag node = case jLookup "type" node of
    Just (JStr t) -> Just t
    _ -> Nothing

hasProps :: Json -> Bool
hasProps node = case jLookup "properties" node of
    Just (JObj (_ : _)) -> True
    _ -> False

objProps :: Json -> [(String, Json)]
objProps node = case jLookup "properties" node of
    Just (JObj kvs) -> kvs
    _ -> []

collectStrs :: [Json] -> [String]
collectStrs [] = []
collectStrs (JStr s : rest) = s : collectStrs rest
collectStrs (_ : rest) = collectStrs rest

requiredFields :: Json -> [String]
requiredFields node = case jLookup "required" node of
    Just (JArr xs) -> collectStrs xs
    _ -> []

-- The type name a "$ref" points at: the last path segment, e.g.
-- "#/definitions/Address" -> "Address".
lastSlash :: String -> Int
lastSlash s = go 1 0
  where
    n = strLen s
    go i best =
        if i > n then best
        else go (i + 1) (if strByte s i == 47 then i else best)

afterLastSlash :: String -> String
afterLastSlash s = strSub s (lastSlash s + 1) (strLen s)

refName :: String -> String
refName r = typeIdent (afterLastSlash r)

-- === Type generation =======================================================
-- genType returns (auxiliary declarations that must also be emitted, the
-- mata-ll type expression that refers to this schema).

genType :: String -> Json -> ([String], String)
genType hint node =
    case jLookup "$ref" node of
        Just (JStr r) -> ([], refName r)
        _ ->
            if hasProps node
                then objectType hint node
                else case schemaTypeTag node of
                    Just "object"  -> ([], "Json")
                    Just "array"   -> arrayType hint node
                    Just "integer" -> ([], "Int")
                    Just "number"  -> ([], "Number")
                    Just "boolean" -> ([], "Bool")
                    Just "string"  -> ([], "String")
                    Just "null"    -> ([], "Json")
                    _              -> ([], "Json")

arrayType :: String -> Json -> ([String], String)
arrayType hint node = case jLookup "items" node of
    Just items ->
        let (aux, t) = genType (hint <> "Item") items
        in (aux, "[" <> t <> "]")
    Nothing -> ([], "[Json]")

objectType :: String -> Json -> ([String], String)
objectType hint node =
    let name = typeIdent hint
        req = requiredFields node
        (aux, fields) = genFields name req (objProps node)
        decl = renderRecord name fields
    in (aux ++ [decl], name)

-- Each property becomes a field; nested object/array types are named after the
-- owner and key (e.g. field "address" of Person -> type PersonAddress).
genFields :: String -> [String] -> [(String, Json)] -> ([String], [Field])
genFields _ _ [] = ([], [])
genFields owner req ((k, sub) : rest) =
    let (aux1, t0) = genType (owner <> typeIdent k) sub
        t = if elem k req then t0 else "Maybe (" <> t0 <> ")"
        (aux2, fs) = genFields owner req rest
    in (aux1 ++ aux2, Field (fieldIdent k) t : fs)

-- === Rendering =============================================================

renderField :: Field -> String
renderField (Field n t) = n <> " :: " <> t

deriv :: String
deriv = "    deriving (Eq, Show, FromJSON, ToJSON)\n"

renderRecord :: String -> [Field] -> String
renderRecord name [] =
    "data " <> name <> " = " <> name <> "\n" <> deriv
renderRecord name (f : fs) =
    "data " <> name <> " = " <> name <> "\n"
      <> "    { " <> renderField f <> "\n"
      <> mconcat (map (\x -> "    , " <> renderField x <> "\n") fs)
      <> "    }\n"
      <> deriv

-- A named schema that is NOT an object record becomes a type synonym.
nameDecl :: String -> ([String], String) -> [String]
nameDecl nm (aux, t) =
    if t == nm then aux
    else aux ++ ["type " <> nm <> " = " <> t <> "\n"]

-- === Driver ================================================================

rootTypeName :: [String] -> Json -> String
rootTypeName args schema = case args of
    (n : _) -> typeIdent n
    [] -> case jLookup "title" schema of
        Just (JStr t) -> typeIdent t
        _ -> "Root"

genDefs :: Json -> [String]
genDefs schema =
    let d1 = defsUnder "definitions" schema
        d2 = defsUnder "$defs" schema
    in concatMap genOneDef (d1 ++ d2)

defsUnder :: String -> Json -> [(String, Json)]
defsUnder key schema = case jLookup key schema of
    Just (JObj kvs) -> kvs
    _ -> []

genOneDef :: (String, Json) -> [String]
genOneDef (name, sub) =
    let nm = typeIdent name
    in nameDecl nm (genType nm sub)

joinDecls :: [String] -> String
joinDecls ds = mconcat (map (\d -> d <> "\n") ds)

banner :: String
banner =
    "-- Generated by utilities/schema2mll.mll from a JSON Schema.\n"
      <> "-- Field labels are the JSON keys verbatim so the derived FromJSON/ToJSON\n"
      <> "-- instances round-trip; two generated records that share a key name will\n"
      <> "-- clash (Haskell field labels are module-global) and must be renamed.\n\n"
      <> "import JSON\n\n"

generate :: [String] -> Json -> String
generate args schema =
    let rootName = rootTypeName args schema
        defs = genDefs schema
        rootPart = nameDecl rootName (genType rootName schema)
    in banner <> joinDecls (defs ++ rootPart)

main :: IO ()
main = do
    args <- getArgs
    schemaText <- readStdin "a"
    case parseJSON schemaText of
        Left err -> putStrLn ("-- error: could not parse JSON schema: " <> err)
        Right schema -> putStr (generate args schema)
