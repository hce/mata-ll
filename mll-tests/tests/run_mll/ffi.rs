//! The Lua FFI boundary: exports, imports, marshalling/decoding in both
//! directions, callbacks, LuaDict, and FFI-signature validation.

use super::*;

// Regression (long-standing FFI bug): a list-typed argument crossing OUT to a
// Lua host — on its own, nested inside a LuaDict record, or nested inside
// another list — must be marshalled into a plain 1-based Lua array with its
// elements forced, not handed over as a raw mata-ll cons cell. Before the fix
// the argument-direction marshaller only descended into tuples/Maybe and
// deliberately skipped lists, so the host received a cons cell (head at [1],
// lazy tail thunk at [2]); `operands[2]` was a function and any arithmetic on
// it crashed. The host functions below assert they receive real plain arrays
// (no metatable) of numbers, so a regression to the raw cons cell fails loudly.
#[test]
fn ffi_list_argument_marshalled_to_array() {
    let source = r#"
data Bag = Bag { bagItems as "items" :: [Int], bagName as "name" :: String }
    deriving (Show, LuaDict)

-- top-level list argument
hostSum :: [Int] -> LuaPure "host_sum" Int
-- list nested inside a LuaDict record field
hostBagSum :: Bag -> LuaPure "host_bagsum" Int
-- list of lists (nested list element needs its own conversion)
hostSum2 :: [[Int]] -> LuaPure "host_sum2" Int

main :: IO ()
main = do
  -- literal list
  assert (hostSum [10, 20, 30] == 60) "top-level [Int] argument"
  -- computed elements (thunks): forcing at the boundary is exercised
  assert (hostSum (map (\x -> x * 2) [5, 10, 15]) == 60) "list argument with thunked elements"
  -- list nested in a record, alongside a scalar field
  assert (hostBagSum (Bag [1, 2, 3, 4] "xs") == 10) "list nested in a record field"
  -- list of lists
  assert (hostSum2 [[1, 2], [3, 4], [5]] == 15) "list-of-lists argument"
  putStrLn "ffi list argument marshalling ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi list-argument program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host functions that REQUIRE a real Lua array of forced numbers. A raw cons
    // cell (metatable-tagged, `[2]` = tail function) trips every guard.
    let host = r#"
        local function checkArray(a, who)
            if type(a) ~= "table" then error(who .. ": not a table, got " .. type(a)) end
            if getmetatable(a) ~= nil then error(who .. ": got a metatable-tagged value (raw cons cell), not a plain array") end
        end
        local function sumArray(a, who)
            checkArray(a, who)
            local s = 0
            for i = 1, #a do
                if type(a[i]) ~= "number" then error(who .. ": element " .. i .. " is " .. type(a[i]) .. ", not a number") end
                s = s + a[i]
            end
            return s
        end
        function host_sum(a) return sumArray(a, "host_sum") end
        function host_bagsum(bag)
            if type(bag) ~= "table" then error("host_bagsum: bag not a table") end
            if type(bag.name) ~= "string" then error("host_bagsum: bag.name is " .. type(bag.name) .. ", not a string") end
            return sumArray(bag.items, "host_bagsum items")
        end
        function host_sum2(a)
            checkArray(a, "host_sum2")
            local s = 0
            for i = 1, #a do s = s + sumArray(a[i], "host_sum2 inner " .. i) end
            return s
        end
    "#;
    lua.load(host).set_name("ffi_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_list_argument_marshalled_to_array").exec()
        .expect("ffi list-argument program should run and pass its assertions");
}

// Regression (broke at c3cf855 "make cons heads lazy", worked in 0.1.2, fixed
// by the FFI argument marshaller): a String that is BUILT rather than written
// as a literal — e.g. decoded from JSON — is a `[Char]` structure, not a native
// Lua string. When cons heads became lazy, such a String began crossing the FFI
// argument boundary as a raw cons table instead of a native string, so a host
// reading it (e.g. `params.hostname`) received a table and failed with
// "converting Lua table to String". A String *literal* never reproduced this
// (it is already native), which is exactly why it slipped past the literal-only
// tests — the trigger has to be a constructed String. Here a `[String]` is
// decoded from JSON and each element is passed to a host that requires a native
// Lua string, so a regression to the raw cons table fails loudly.
#[test]
fn ffi_json_decoded_string_argument_is_native_string() {
    let source = r#"
import JSON

data Cfg = Cfg { cfgHosts as "hostnames" :: [String] }
    deriving (FromJSON, Show)

data HostParam = HostParam { hpName as "name" :: String }
    deriving (LuaDict)

sendHost :: HostParam -> LuaPure "send_host" String

cfg :: Cfg
cfg = case decodeJSON "{\"hostnames\": [\"hce.li\", \"example.com\"]}" of
        Right r -> r
        Left e  -> error e

main :: IO ()
main = do
  mapM_ (\h -> assert (sendHost (HostParam h) == h) "json-decoded string arg reaches host as a native string") (cfgHosts cfg)
  putStrLn "ffi json-decoded string argument ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi json-string program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host REQUIRES a native Lua string. A raw cons cell (a table) fails the guard.
    let host = r#"
        function send_host(p)
            if type(p) ~= "table" then error("send_host: params not a table") end
            if type(p.name) ~= "string" then
                error("send_host: name is " .. type(p.name) .. ", not a string "
                      .. "(regression: JSON-decoded String crossed as a raw cons table)")
            end
            return p.name
        end
    "#;
    lua.load(host).set_name("ffi_json_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_json_decoded_string_argument_is_native_string").exec()
        .expect("ffi json-string program should run and pass its assertions");
}

// Regression (long-standing FFI bug): a `Maybe` field inside a LuaDict record
// crossing OUT to a host must be UNWRAPPED — `Just x` becomes the bare `x`
// (recursively marshalled by x's type), `Nothing` becomes nil — matching
// __mll_to_lua and inverting the result decoder. Before the fix the argument
// marshaller descended into the `Just` wrapper without stripping it, so the
// host received the raw `{x}` __just_mt table and `p.port + 1` crashed with
// "arithmetic on a table value". This exercises the OUT direction (host sees a
// bare number / a real array / nil) AND the round-trip: the host echoes the
#[test]
fn ffi_maybe_list_argument_preserves_positions() {
    // A `[Maybe a]` FFI argument marshals `Nothing` -> nil AT ITS POSITION with
    // no compaction: `[Just 1, Nothing, Just 3]` reaches the host with 3 at
    // index 3, not shifted to index 2. Was: silently compacted to {1, 3}.
    let src = r#"
at :: Int -> [Maybe Int] -> LuaPure "at" Int
main :: IO ()
main = do
    let xs = [Just 1, Nothing, Just 3]
    assert (at 3 xs == 3) "Just 3 stays at index 3 (no compaction)"
    assert (at 1 xs == 1) "Just 1 stays at index 1"
    putStrLn "ok"
"#;
    let lua_code = compile(src, Path::new("."), &[])
        .expect("compile should succeed").lua_code;
    let lua = mlua::Lua::new();
    lua.load("function at(i, arr) return arr[i] or -1 end")
        .exec().expect("define host at");
    lua.load(&lua_code).set_name("ml_pos").exec()
        .expect("[Maybe a] argument must preserve element positions, not compact");
}

// port back into a `Maybe Int` result field, which the decoder must
// reconstruct as Just/Nothing — encode-then-decode identity.
#[test]
fn ffi_maybe_field_marshalled_and_roundtrips() {
    let source = r#"
data In = In
        { iName as "name" :: String
        , iPort as "port" :: Maybe Int
        , iTags as "tags" :: Maybe [Int] }
    deriving (Show, LuaDict)

data Out = Out { oBack as "back" :: Maybe Int, oSum as "sum" :: Int }
    deriving (Show, LuaDict)

probe :: In -> LuaPure "probe" Out

main :: IO ()
main = do
  -- Just: host sees a bare number and a real array; echoes the port back.
  case probe (In "h" (Just 443) (Just [1, 2, 3])) of
    Out back s -> do
      case back of
        Just n  -> assert (n == 443) "Just Maybe field round-trips to Just (present)"
        Nothing -> error "expected Just 443 back, got Nothing"
      assert (s == 6) "Just [Int] field unwrapped and marshalled to an array (1+2+3)"
  -- Nothing: host sees nil for both optional fields; echoes Nothing back.
  case probe (In "h" Nothing Nothing) of
    Out back s -> do
      case back of
        Nothing -> putStrLn "Nothing Maybe field round-trips to Nothing (absent)"
        Just _  -> error "expected Nothing back, got Just"
      assert (s == 0) "Nothing [Int] field is nil (sum 0)"
  putStrLn "ffi maybe-field marshalling ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi maybe-field program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // The host REQUIRES unwrapped Maybe fields: a bare number (or nil) for
    // `port`, and a plain array (no metatable) or nil for `tags`. A raw
    // `{x}` __just_mt wrapper or a cons cell trips these guards.
    let host = r#"
        function probe(inp)
            if inp.port ~= nil and type(inp.port) ~= "number" then
                error("probe: port must be a bare number or nil, got " .. type(inp.port))
            end
            local s = 0
            if inp.tags ~= nil then
                if getmetatable(inp.tags) ~= nil then
                    error("probe: tags must be a plain array (Just unwrapped), got a metatable-tagged value")
                end
                for i = 1, #inp.tags do
                    if type(inp.tags[i]) ~= "number" then
                        error("probe: tags element " .. i .. " is " .. type(inp.tags[i]) .. ", not a number")
                    end
                    s = s + inp.tags[i]
                end
            end
            -- Round-trip: echo the (already-unwrapped) port back; nil stays nil
            -- so the decoder reconstructs Nothing.
            return { back = inp.port, sum = s }
        end
    "#;
    lua.load(host).set_name("ffi_maybe_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_maybe_field_marshalled_and_roundtrips").exec()
        .expect("ffi maybe-field program should run and pass its assertions");
}

// Regression (long-standing FFI bug): a `HashMap` argument crossing OUT to a
// host must marshal its VALUES by the value type — `HashMap String [Int]`
// reaches the host as a dict of plain arrays, `HashMap String (Maybe X)` as a
// dict of bare values, `HashMap String Record` as a dict of dicts — recursively
// at any nesting. The argument marshaller descended into lists/tuples/records/
// Maybe but not HashMap, so each value arrived as a raw cons cell / wrapper.
// Keys are scalars already usable as Lua keys and are kept (like the decoder).
#[test]
fn ffi_hashmap_structured_values_marshalled() {
    let source = r#"
import qualified Data.Map as Map

data V = V { vName as "name" :: String, vNums as "nums" :: [Int] }
    deriving (Show, LuaDict)

mapLists  :: HashMap String [Int]      -> LuaPure "mp_lists"  Int
mapMaybes :: HashMap String (Maybe Int) -> LuaPure "mp_maybes" Int
mapRecs   :: HashMap String V              -> LuaPure "mp_recs"   Int

main :: IO ()
main = do
  assert (mapLists  (Map.fromList [("a", [1, 2]), ("b", [3, 4, 5])]) == 15) "hashmap of lists -> arrays"
  assert (mapMaybes (Map.fromList [("x", Just 7), ("z", Just 3)]) == 10)   "hashmap of Maybe -> bare values"
  assert (mapRecs   (Map.fromList [("r", V "n" [10, 20])]) == 30)          "hashmap of records -> nested dict/array"
  putStrLn "ffi hashmap-structured-values ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi hashmap program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let host = r#"
        local function checkArray(a, who)
            if type(a) ~= "table" then error(who .. ": not a table, got " .. type(a)) end
            if getmetatable(a) ~= nil then error(who .. ": got a metatable-tagged value (raw cons cell), not a plain array") end
        end
        function mp_lists(m)
            local s = 0
            for k, v in pairs(m) do
                checkArray(v, "mp_lists value " .. k)
                for i = 1, #v do
                    if type(v[i]) ~= "number" then error("mp_lists: element not a number: " .. type(v[i])) end
                    s = s + v[i]
                end
            end
            return s
        end
        function mp_maybes(m)
            local s = 0
            for k, v in pairs(m) do
                if type(v) ~= "number" then error("mp_maybes: value for " .. k .. " must be a bare number (Just unwrapped), got " .. type(v)) end
                s = s + v
            end
            return s
        end
        function mp_recs(m)
            local s = 0
            for k, v in pairs(m) do
                if type(v) ~= "table" or type(v.name) ~= "string" then error("mp_recs: value must be a record dict") end
                checkArray(v.nums, "mp_recs nums of " .. k)
                for i = 1, #v.nums do s = s + v.nums[i] end
            end
            return s
        end
    "#;
    lua.load(host).set_name("ffi_hashmap_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_hashmap_structured_values_marshalled").exec()
        .expect("ffi hashmap program should run and pass its assertions");
}

// Parity test: the argument marshaller is a COMPLETE structural dual of the
// result decoder, so a value built in mata-ll, passed to an echo host (which
// returns it unchanged), and decoded back is IDENTICAL — for every container
// (list, tuple, LuaDict record, Maybe, HashMap) and their nestings (HashMap of
// lists, list of records with Maybe fields). This is the test that catches a
// missed container in either direction at once: if the marshaller fails to
// encode a container the decoder expects, the echo round-trip diverges.
#[test]
fn ffi_arg_marshal_roundtrips_all_containers() {
    let source = r#"
import qualified Data.Map as Map

data Rec = Rec { rTag as "tag" :: String, rMaybe as "m" :: Maybe Int }
    deriving (Show, Eq, LuaDict)

echoList  :: [Int]                 -> LuaPure "echo" [Int]
echoPairs :: [(Int, String)]       -> LuaPure "echo" [(Int, String)]
echoRec   :: Rec                        -> LuaPure "echo" Rec
echoRecs  :: [Rec]                      -> LuaPure "echo" [Rec]
echoMap   :: HashMap String [Int]  -> LuaPure "echo" (HashMap String [Int])

lk :: String -> HashMap String [Int] -> [Int]
lk k m = case Map.lookup k m of
           Just v  -> v
           Nothing -> []

main :: IO ()
main = do
  -- list; list of tuples (nested tuple decodes as a single table, unlike a
  -- top-level tuple result which uses Lua multi-return); record with Just and
  -- Nothing Maybe fields; list of records.
  assert (echoList [1, 2, 3] == [1, 2, 3]) "list round-trips"
  assert (echoPairs [(1, "a"), (2, "b")] == [(1, "a"), (2, "b")]) "list of tuples round-trips (nested tuple)"
  assert (echoRec (Rec "a" (Just 9)) == Rec "a" (Just 9)) "record with Just field round-trips"
  assert (echoRec (Rec "b" Nothing) == Rec "b" Nothing) "record with Nothing field round-trips"
  assert (echoRecs [Rec "a" (Just 1), Rec "b" Nothing] == [Rec "a" (Just 1), Rec "b" Nothing]) "list of records round-trips"
  -- HashMap of lists: compare by lookup (HashMap has no derived Eq here)
  let m = echoMap (Map.fromList [("a", [1, 2]), ("b", [3, 4, 5])])
  assert (lk "a" m == [1, 2]) "hashmap-of-lists round-trips (key a)"
  assert (lk "b" m == [3, 4, 5]) "hashmap-of-lists round-trips (key b)"
  putStrLn "ffi arg-marshal round-trip parity ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("ffi parity program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // A pure echo: whatever the marshaller hands the host, hand it straight back.
    // Round-trip identity then depends ENTIRELY on the marshaller and decoder
    // being exact duals.
    lua.load("function echo(x) return x end").set_name("ffi_echo_host").exec()
        .expect("echo host loads");
    lua.load(&lua_code).set_name("ffi_arg_marshal_roundtrips_all_containers").exec()
        .expect("ffi parity program should run and pass its assertions");
}

// FFI marshalling with a fully CONSTRUCTED structure: the record crossing OUT
// is decoded from JSON — not written as a record literal — so every leaf
// (native string, bare number, nested record, list, present/absent Maybe) is
// the product of the FromJSON decoder, and the marshaller must convert what
// the decoder actually builds, not what a literal would compile to. The host
// type-checks every leaf (a raw cons cell, an unstripped Just wrapper, or a
// [Char]-structured string all fail loudly), then answers with a structure of
// its own that the result decoder must rebuild (including nil -> Nothing and
// a present field -> Just).
#[test]
fn ffi_json_constructed_record_crosses_boundary() {
    let source = r#"
import JSON

data Peer = Peer { peerHost as "host" :: String, peerPort as "port" :: Maybe Int }
    deriving (Eq, Show, FromJSON, LuaDict)

data Job = Job
        { jobName as "name" :: String
        , jobRetries as "retries" :: Int
        , jobPeers as "peers" :: [Peer]
        , jobNote as "note" :: Maybe String }
    deriving (Eq, Show, FromJSON, LuaDict)

data Verdict = Verdict
        { vOk as "ok" :: Bool
        , vTotal as "total" :: Int
        , vFirst as "first" :: Maybe String }
    deriving (Show, LuaDict)

submit :: Job -> LuaPure "submit_job" Verdict

-- The job is CONSTRUCTED by the JSON decoder: renamed keys, a nested record
-- list, a present Maybe (port 443), an absent Maybe (b.example has no port),
-- and a null Maybe (note).
job :: Job
job = case decodeJSON "{\"name\": \"scan\", \"retries\": 3, \"peers\": [{\"host\": \"a.example\", \"port\": 443}, {\"host\": \"b.example\"}], \"note\": null}" of
        Right j -> j
        Left e  -> error e

main :: IO ()
main =
  case submit job of
    Verdict ok total first -> do
      assert ok "host validated every leaf of the JSON-built job"
      assert (total == 446) "host summed retries and the one present port (3 + 443)"
      case first of
        Just h  -> assert (h == "a.example") "host's present field decodes back to Just"
        Nothing -> error "expected Just \"a.example\" back, got Nothing"
      putStrLn "ffi json-constructed record ok"
"#;
    let lib_path = Path::new("../lib");
    let lua_code = compile(source, Path::new("."), &[lib_path])
        .expect("json-constructed record program should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    // The host REQUIRES converted leaves everywhere: native strings, bare
    // numbers or nil for Maybe fields, metatable-free plain tables for the
    // record and the peer array. Whatever the FromJSON decoder built, only
    // proper marshalling satisfies these guards.
    let host = r#"
        function submit_job(job)
            local function fail(msg) error("submit_job: " .. msg) end
            if type(job) ~= "table" then fail("job is " .. type(job) .. ", not a table") end
            if getmetatable(job) ~= nil then fail("job carries a metatable (raw mata-ll value)") end
            if type(job.name) ~= "string" then
                fail("name is " .. type(job.name) .. ", not a native string (JSON-built String regression)")
            end
            if type(job.retries) ~= "number" then fail("retries is " .. type(job.retries) .. ", not a number") end
            if job.note ~= nil then fail("null note must arrive as nil, got " .. type(job.note)) end
            if type(job.peers) ~= "table" then fail("peers is " .. type(job.peers) .. ", not a table") end
            if getmetatable(job.peers) ~= nil then fail("peers carries a metatable (raw cons cell)") end
            if #job.peers ~= 2 then fail("expected 2 peers, got " .. #job.peers) end
            local total = job.retries
            for i, p in ipairs(job.peers) do
                if type(p) ~= "table" then fail("peer " .. i .. " is " .. type(p) .. ", not a table") end
                if type(p.host) ~= "string" then
                    fail("peer " .. i .. " host is " .. type(p.host) .. ", not a native string")
                end
                if p.port ~= nil and type(p.port) ~= "number" then
                    fail("peer " .. i .. " port must be a bare number or nil (Just unwrap regression), got " .. type(p.port))
                end
                total = total + (p.port or 0)
            end
            return { ok = true, total = total, first = job.peers[1].host }
        end
    "#;
    lua.load(host).set_name("ffi_json_record_host").exec().expect("host definitions load");
    lua.load(&lua_code).set_name("ffi_json_constructed_record_crosses_boundary").exec()
        .expect("json-constructed record program should run and pass its assertions");
}

// ============================================================
// FFI tests: compile MLL modules with exports, then call
// exported functions from Lua and verify return values.
// ============================================================

/// Helper: compile MLL source and return a Lua module table
pub(crate) fn compile_ffi_module(source: &str) -> (mlua::Lua, mlua::Table) {
    let lua_code = compile(source, Path::new("."), &[])
        .expect("FFI module should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let table: mlua::Table = lua.load(&lua_code)
        .set_name("ffi_test")
        .eval()
        .expect("FFI module should return a table");
    (lua, table)
}

#[test]
fn lua_iterator_result_must_be_an_explicit_list() {
    // The LuaIterator type argument always names the result list, so a bare
    // (non-list) element type is rejected: it would make the argument
    // ambiguous with a genuine list-yielding iterator (`[[Int]]`).
    let source = r#"
gm :: String -> String -> LuaIterator "string.gmatch" String

main :: IO ()
main = mapM_ putStrLn (gm "a b" "%w+")
"#;
    expect_compile_error(source, &[Path::new("../lib")], &[
        "LuaIterator requires the result to be written as an explicit",
        "[String]",
    ]);
}

#[test]
fn lua_iterator_type_argument_is_the_result_list_and_elements_decode() {
    // experiments/iterator/ regression, two properties in one:
    //
    // 1. The `LuaIterator "f" T` type argument names the RESULT list. A list
    //    argument `[Int]` reduces to `[Int]` (the iterator yields the
    //    ELEMENTS, one Int per step) — NOT `[[Int]]`. So `yields`,
    //    whose host yields plain ints, is a flat `[Int]`.
    // 2. A structured element type is DECODED per element, exactly as an
    //    ordinary FFI result: `arrs :: LuaIterator "…" [[Int]]` reduces to
    //    `[[Int]]`, and each yielded Lua array becomes a cons list (so
    //    `map sum` works). Before the fix elements were stored raw and any
    //    list op failed with "expected a list but got a raw … value".
    let src = r#"
yields :: LuaIterator "yieldints" [Int]
arrs   :: LuaIterator "yieldarrs" [[Int]]

main :: IO ()
main = do
    -- (1) list-arg iterator over a scalar-yielding host is a FLAT [Int].
    assert (take 3 yields == [10, 20, 30]) "list-arg iterator yields a flat [Int]"
    -- (2) structured element (a list) is decoded to a cons list.
    assert (map sum (take 2 arrs) == [3, 7]) "each yielded array decoded to a cons list"
    putStrLn "ok"
"#;
    let lua_code = compile(src, Path::new("."), &[])
        .expect("compile should succeed")
        .lua_code;
    let lua = mlua::Lua::new();
    // Host factories: `yieldints` yields plain ints 10,20,30; `yieldarrs`
    // yields Lua arrays {1,2},{3,4}.
    lua.load(
        r#"
        function yieldints()
            local n = 0
            return function()
                n = n + 1
                if n > 3 then return nil end
                return n * 10
            end
        end
        function yieldarrs()
            local n = 0
            return function()
                n = n + 1
                if n > 2 then return nil end
                return { 2 * n - 1, 2 * n }
            end
        end
        "#,
    )
    .exec()
    .expect("define host iterator factories");
    lua.load(&lua_code)
        .set_name("iter_semantics")
        .exec()
        .expect("LuaIterator result must be the flat list of decoded elements");
}

#[test]
fn ffi_export_pure_functions() {
    let source = r#"
export add :: Int -> Int -> Int
add x y = x + y

export double :: Int -> Int
double n = n * 2

export negate :: Int -> Int
negate n = 0 - n

export isEven :: Int -> Bool
isEven n = n `mod` 2 == 0

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Int arithmetic
    let add: mlua::Function = module.get("add").unwrap();
    let result: i64 = add.call((3, 4)).unwrap();
    assert_eq!(result, 7, "add 3 4 == 7");

    let result: i64 = add.call((0, 0)).unwrap();
    assert_eq!(result, 0, "add 0 0 == 0");

    let result: i64 = add.call((-5, 3)).unwrap();
    assert_eq!(result, -2, "add (-5) 3 == -2");

    let double: mlua::Function = module.get("double").unwrap();
    let result: i64 = double.call(21).unwrap();
    assert_eq!(result, 42, "double 21 == 42");

    let negate: mlua::Function = module.get("negate").unwrap();
    let result: i64 = negate.call(5).unwrap();
    assert_eq!(result, -5, "negate 5 == -5");

    // Bool return
    let is_even: mlua::Function = module.get("isEven").unwrap();
    let result: bool = is_even.call(4).unwrap();
    assert!(result, "isEven 4 == True");
    let result: bool = is_even.call(7).unwrap();
    assert!(!result, "isEven 7 == False");
}

#[test]
fn ffi_export_string_functions() {
    let source = r#"
export greet :: String -> String
greet name = "Hello, " <> name <> "!"

export shout :: String -> String
shout s = s <> "!!!"

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let greet: mlua::Function = module.get("greet").unwrap();
    let result: String = greet.call("world").unwrap();
    assert_eq!(result, "Hello, world!");

    let shout: mlua::Function = module.get("shout").unwrap();
    let result: String = shout.call("wow").unwrap();
    assert_eq!(result, "wow!!!");
}

#[test]
fn ffi_export_list_functions() {
    let source = r#"
range :: Int -> [Int]
range n = if n <= 0 then [] else go 1 n
  where go i m = if i > m then [] else i : go (i + 1) m

export getRange :: Int -> [Int]
getRange n = range n

export squares :: Int -> [Int]
squares n = map (\x -> x * x) (range n)

export countTo :: Int -> Int
countTo n = foldl (+) 0 (range n)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // List returned as Lua array
    let range: mlua::Function = module.get("getRange").unwrap();
    let result: Vec<i64> = range.call(5).unwrap();
    assert_eq!(result, vec![1, 2, 3, 4, 5], "range 5");

    let result: mlua::Value = range.call(0).unwrap();
    assert!(matches!(&result, mlua::Value::Table(t) if t.len().unwrap() == 0),
            "range 0 is an empty table");

    // List → List (map)
    let squares: mlua::Function = module.get("squares").unwrap();
    let result: Vec<i64> = squares.call(4).unwrap();
    assert_eq!(result, vec![1, 4, 9, 16], "squares 4");

    // List → Int (fold)
    let count: mlua::Function = module.get("countTo").unwrap();
    let result: i64 = count.call(10).unwrap();
    assert_eq!(result, 55, "countTo 10 == 55 (triangle number)");
}

#[test]
fn ffi_export_maybe_either() {
    // `Maybe a` has a designed FFI shape (nil ↔ Nothing) and is ACCEPTED.
    let source = r#"
export safeDiv :: Int -> Int -> Maybe Int
safeDiv _ 0 = Nothing
safeDiv x y = Just (x `div` y)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Maybe: Just → value, Nothing → nil
    let safe_div: mlua::Function = module.get("safeDiv").unwrap();
    let result: Option<i64> = safe_div.call((10, 3)).unwrap();
    assert_eq!(result, Some(3), "safeDiv 10 3 == Just 3");

    let result: Option<i64> = safe_div.call((10, 0)).unwrap();
    assert_eq!(result, None, "safeDiv 10 0 == Nothing");

    // Bare `Either` is a plain two-constructor ADT: outside a LuaTry/LuaIOCatch
    // result (where the pcall wrapper builds and interprets its tags) it has no
    // designed FFI shape — it would leak only as mata-ll's internal
    // `{tag, payload}` table — so an export using it directly is REJECTED. (Use
    // Maybe, a LuaDict record, or a scalar/list encoding instead.)
    expect_compile_error(
        "export classify :: Int -> Either String Int\n\
         classify n = if n < 0 then Left \"negative\" else Right n\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "Export 'classify'",
            "the result",
            "Either",
            "tagged table",
        ],
    );
}

#[test]
fn ffi_export_higher_order() {
    // MLL-side higher-order: partial application across FFI
    let source = r#"
applyTwice :: (a -> a) -> a -> a
applyTwice f x = f (f x)

double :: Int -> Int
double x = x * 2

inc :: Int -> Int
inc x = x + 1

export doubleDouble :: Int -> Int
doubleDouble n = applyTwice double n

export incInc :: Int -> Int
incInc n = applyTwice inc n

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let dd: mlua::Function = module.get("doubleDouble").unwrap();
    let result: i64 = dd.call(3).unwrap();
    assert_eq!(result, 12, "doubleDouble 3 == 12");

    let ii: mlua::Function = module.get("incInc").unwrap();
    let result: i64 = ii.call(5).unwrap();
    assert_eq!(result, 7, "incInc 5 == 7");
}

#[test]
fn ffi_export_tuples() {
    let source = r#"
export swap :: (Int, Int) -> (Int, Int)
swap (a, b) = (b, a)

export firstPlusSecond :: (Int, Int) -> Int
firstPlusSecond (a, b) = a + b

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Tuple returned as Lua array
    let swap: mlua::Function = module.get("swap").unwrap();
    let result: Vec<i64> = swap.call(vec![1, 2]).unwrap();
    assert_eq!(result, vec![2, 1], "swap (1,2) == (2,1)");

    let first_plus: mlua::Function = module.get("firstPlusSecond").unwrap();
    let result: i64 = first_plus.call(vec![10, 20]).unwrap();
    assert_eq!(result, 30, "firstPlusSecond (10,20) == 30");
}

#[test]
fn ffi_export_thunked_values() {
    // Regression: top-level values defined via point-free or partial
    // application are thunks — export wrapper must __force before calling
    let source = r#"
export increment :: Int -> Int
increment = (+1)

fib :: [Int]
fib = 1 : 1 : zipWith (+) fib (drop 1 fib)

export fibonacci :: Int -> [Int]
fibonacci = flip take fib

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let increment: mlua::Function = module.get("increment").unwrap();
    let result: i64 = increment.call(41).unwrap();
    assert_eq!(result, 42, "increment 41 == 42");

    let fibonacci: mlua::Function = module.get("fibonacci").unwrap();
    let result: Vec<i64> = fibonacci.call(8).unwrap();
    assert_eq!(result, vec![1, 1, 2, 3, 5, 8, 13, 21], "fibonacci 8");
}

#[test]
fn ffi_export_adt() {
    // A plain user `data` ADT has NO defined FFI shape: it would cross only as
    // mata-ll's internal `{tag, fields...}` table, which has no meaning to a
    // Lua host. So it is REJECTED at the boundary in BOTH directions — as an
    // argument (colorCode :: Color -> Int) and as a result
    // (mkRed :: Int -> Color). (To carry an enum across, derive LuaDict on
    // an all-nullary sum so its constructors cross as name strings; to carry a
    // record, use a LuaDict record; a newtype crosses transparently.)
    expect_compile_error(
        "data Color = Red | Green | Blue\n\
         export colorCode :: Color -> Int\ncolorCode _ = 1\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "Export 'colorCode'",
            "argument 1",
            "Color",
            "internal",
            "tagged table",
            "LuaDict",
        ],
    );

    expect_compile_error(
        "data Color = Red | Green | Blue\n\
         export mkRed :: Int -> Color\nmkRed _ = Red\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "Export 'mkRed'",
            "the result",
            "Color",
        ],
    );
}

#[test]
fn ffi_export_multi_arg() {
    // Test multi-arg exported functions and string operations
    let source = r#"
export strRepeat :: String -> Int -> String
strRepeat _ 0 = ""
strRepeat s n = s <> strRepeat s (n - 1)

export clamp :: Int -> Int -> Int -> Int
clamp lo hi x = if x < lo then lo else if x > hi then hi else x

export between :: Int -> Int -> Bool
between lo hi = lo < hi

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let str_repeat: mlua::Function = module.get("strRepeat").unwrap();
    let result: String = str_repeat.call(("ab", 3)).unwrap();
    assert_eq!(result, "ababab", "strRepeat ab 3");

    let result: String = str_repeat.call(("x", 0)).unwrap();
    assert_eq!(result, "", "strRepeat x 0");

    let clamp: mlua::Function = module.get("clamp").unwrap();
    let result: i64 = clamp.call((0, 10, 15)).unwrap();
    assert_eq!(result, 10, "clamp 0 10 15 == 10");

    let result: i64 = clamp.call((0, 10, 5)).unwrap();
    assert_eq!(result, 5, "clamp 0 10 5 == 5");

    let between: mlua::Function = module.get("between").unwrap();
    let result: bool = between.call((3, 7)).unwrap();
    assert!(result, "between 3 7 == True");
}

#[test]
fn ffi_export_deep_force() {
    // Regression: lazy thunks (e.g. from map) must be fully forced across FFI
    let source = r#"
export mapDouble :: [Int] -> [Int]
mapDouble xs = map (\x -> x * 2) xs

export mapShow :: [Int] -> [String]
mapShow xs = map show xs

export listOfStrings :: Int -> [String]
listOfStrings _ = ["hello", "world", "foo"]

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // map returning lazy thunks — must be deep-forced to Lua values
    let map_double: mlua::Function = module.get("mapDouble").unwrap();
    let result: Vec<i64> = map_double.call(vec![1, 2, 3]).unwrap();
    assert_eq!(result, vec![2, 4, 6], "mapDouble [1,2,3]");

    let map_show: mlua::Function = module.get("mapShow").unwrap();
    let result: Vec<String> = map_show.call(vec![10, 20, 30]).unwrap();
    assert_eq!(result, vec!["10", "20", "30"], "mapShow [10,20,30]");

    // List of strings — previously broken because __mll_to_lua heuristic
    // misidentified string-headed cons cells
    let list_of_strings: mlua::Function = module.get("listOfStrings").unwrap();
    let result: Vec<String> = list_of_strings.call(0).unwrap();
    assert_eq!(result, vec!["hello", "world", "foo"], "listOfStrings");
}

#[test]
fn ffi_export_lua_to_mll_lists() {
    // Lua arrays passed as arguments must be converted to MLL cons lists
    let source = r#"
export sumList :: [Int] -> Int
sumList xs = foldl (+) 0 xs

export headOf :: [Int] -> Int
headOf xs = head xs

export lengthOf :: [Int] -> Int
lengthOf [] = 0
lengthOf (_:xs) = 1 + lengthOf xs

export appendLists :: [Int] -> [Int] -> [Int]
appendLists xs ys = xs ++ ys

export reverseList :: [Int] -> [Int]
reverseList xs = foldl (flip (:)) [] xs

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Passing Lua arrays → MLL cons lists
    let sum: mlua::Function = module.get("sumList").unwrap();
    let result: i64 = sum.call(vec![1, 2, 3, 4, 5]).unwrap();
    assert_eq!(result, 15, "sumList [1..5] == 15");

    let head: mlua::Function = module.get("headOf").unwrap();
    let result: i64 = head.call(vec![42, 99]).unwrap();
    assert_eq!(result, 42, "headOf [42, 99] == 42");

    let len: mlua::Function = module.get("lengthOf").unwrap();
    let result: i64 = len.call(vec![10, 20, 30]).unwrap();
    assert_eq!(result, 3, "lengthOf [10,20,30] == 3");

    // Empty list
    let result: i64 = sum.call(Vec::<i64>::new()).unwrap();
    assert_eq!(result, 0, "sumList [] == 0");

    // Two list arguments
    let append: mlua::Function = module.get("appendLists").unwrap();
    let result: Vec<i64> = append.call((vec![1, 2], vec![3, 4])).unwrap();
    assert_eq!(result, vec![1, 2, 3, 4], "appendLists [1,2] [3,4]");

    // List → List roundtrip
    let rev: mlua::Function = module.get("reverseList").unwrap();
    let result: Vec<i64> = rev.call(vec![1, 2, 3]).unwrap();
    assert_eq!(result, vec![3, 2, 1], "reverseList [1,2,3]");
}

#[test]
fn ffi_export_string_lists() {
    // String lists: Lua string arrays → MLL [String] and back.
    // (filterLong's where-binding originally used a nonexistent `unpack`; the
    // typechecker used to swallow where-binding errors, so the broken —
    // and never-called — function compiled anyway. It now uses a real
    // string-length FFI declaration and is actually exercised.)
    let source = r#"
strLen :: String -> LuaPure "string.len" Int

export joinWith :: String -> [String] -> String
joinWith _ [] = ""
joinWith _ [x] = x
joinWith sep (x:xs) = x <> sep <> joinWith sep xs

export filterLong :: Int -> [String] -> [String]
filterLong n xs = filter (\s -> lengthS s > n) xs
  where lengthS s = strLen s

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let join: mlua::Function = module.get("joinWith").unwrap();
    let result: String = join.call((",", vec!["a", "b", "c"])).unwrap();
    assert_eq!(result, "a,b,c", "joinWith , [a,b,c]");

    let result: String = join.call(("-", vec!["hello"])).unwrap();
    assert_eq!(result, "hello", "joinWith - [hello]");

    let result: String = join.call((",", Vec::<String>::new())).unwrap();
    assert_eq!(result, "", "joinWith , []");

    let filter_long: mlua::Function = module.get("filterLong").unwrap();
    let result: Vec<String> = filter_long.call((3, vec!["hi", "hello", "hey", "world"])).unwrap();
    assert_eq!(result, vec!["hello", "world"], "filterLong 3 keeps strings longer than 3");

    // An empty MLL list crosses to the host as an empty table, matching the
    // FFI argument edge (hosts can ipairs a list result without a nil check).
    // The type descriptor distinguishes it from Nothing, which stays nil.
    let result: mlua::Value = filter_long.call((10, vec!["short", "tiny"])).unwrap();
    let table = result
        .as_table()
        .expect("empty list result must be a table, not nil");
    assert_eq!(table.raw_len(), 0, "filterLong 10 filters everything out (empty list exports as a table)");
}

#[test]
fn ffi_export_empty_list_is_table_nothing_is_nil() {
    // mata-ll represents both [] and Nothing as nil internally; the declared
    // export type is what tells them apart at the boundary. A list result
    // marshals the empty case to a fresh {} — matching the FFI argument edge,
    // so hosts can ipairs any list result without a nil check — while a Maybe
    // result keeps Nothing as nil. Before this contract change the export
    // edge collapsed a top-level [] to nil even though the same empty list
    // one level deeper (a Just []) already marshalled to {}.
    let source = r#"
export emptyList :: Int -> [Int]
emptyList n = filter (\k -> k > n) [1, 2, 3]

export justEmpty :: Int -> Maybe [Int]
justEmpty n = n `seq` Just []

export nothingAtAll :: Int -> Maybe [Int]
nothingAtAll n = n `seq` Nothing

export emptyValue :: [Int]
emptyValue = []

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    let empty_list: mlua::Function = module.get("emptyList").unwrap();
    let v: mlua::Value = empty_list.call(10).unwrap();
    let t = v.as_table().expect("[] result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "[] result is an empty table");

    let just_empty: mlua::Function = module.get("justEmpty").unwrap();
    let v: mlua::Value = just_empty.call(1).unwrap();
    let t = v.as_table().expect("Just [] result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "Just [] unwraps to an empty table");

    let nothing_at_all: mlua::Function = module.get("nothingAtAll").unwrap();
    let v: mlua::Value = nothing_at_all.call(1).unwrap();
    assert!(v.is_nil(), "Nothing stays nil");

    // A VALUE export of an empty list follows the same contract as a
    // function result (the n_args == 0 non-action emission path).
    let v: mlua::Value = module.get("emptyValue").unwrap();
    let t = v.as_table().expect("[] value export must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "[] value export is an empty table");
}

#[test]
fn ffi_export_mixed_args() {
    // Functions with both list and non-list arguments
    let source = r#"
export takeN :: Int -> [Int] -> [Int]
takeN n xs = take n xs

export dropN :: Int -> [Int] -> [Int]
dropN n xs = drop n xs

export replicate :: Int -> Int -> [Int]
replicate 0 _ = []
replicate n x = x : replicate (n - 1) x

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Int arg + list arg
    let take_n: mlua::Function = module.get("takeN").unwrap();
    let result: Vec<i64> = take_n.call((3, vec![10, 20, 30, 40, 50])).unwrap();
    assert_eq!(result, vec![10, 20, 30], "takeN 3 [10..50]");

    let drop_n: mlua::Function = module.get("dropN").unwrap();
    let result: Vec<i64> = drop_n.call((2, vec![10, 20, 30, 40])).unwrap();
    assert_eq!(result, vec![30, 40], "dropN 2 [10..40]");

    // Generate list on MLL side, no conversion needed for args
    let rep: mlua::Function = module.get("replicate").unwrap();
    let result: Vec<i64> = rep.call((4, 7)).unwrap();
    assert_eq!(result, vec![7, 7, 7, 7], "replicate 4 7");
}

#[test]
fn ffi_export_values() {
    // A VALUE export (a nullary, non-IO-action binding) must be marshalled to
    // Lua directly, by the SAME result contract a function's RETURN value uses —
    // NOT wrapped in a calling wrapper (which would emit `__force(value)(...)`
    // and crash with "attempt to call a number/table value"). It supports
    // exactly the types a function result does: a scalar, a LuaDict record
    // (keyed table), a tuple (positional table), etc. A function export and an
    // IO-action export in the same module must keep their performing wrappers.
    let source = r#"
data Config = Config { width :: Int, height :: Int }
  deriving (LuaDict)

export answer :: Int
answer = 42

export config :: Config
config = Config { width = 640, height = 480 }

export pairV :: (Int, String)
pairV = (7, "seven")

export incr :: Int -> Int
incr n = n + 1

export runIt :: IO Int
runIt = pure 99

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Scalar value: read directly as the number, NOT as a function.
    let answer: mlua::Value = module.get("answer").unwrap();
    assert!(
        matches!(&answer, mlua::Value::Integer(42)) || matches!(&answer, mlua::Value::Number(n) if *n == 42.0),
        "answer must be the marshalled value 42, got {answer:?}"
    );
    assert!(
        !matches!(answer, mlua::Value::Function(_)),
        "a value export must not be a function"
    );

    // LuaDict record value → a keyed Lua table.
    let config: mlua::Table = module.get("config").unwrap();
    let w: i64 = config.get("width").unwrap();
    let h: i64 = config.get("height").unwrap();
    assert_eq!((w, h), (640, 480), "record value marshals to a keyed table");

    // Tuple value → a positional Lua table.
    let pair: mlua::Table = module.get("pairV").unwrap();
    let fst: i64 = pair.get(1).unwrap();
    let snd: String = pair.get(2).unwrap();
    assert_eq!((fst, snd.as_str()), (7, "seven"), "tuple value marshals to a positional table");

    // Function export UNCHANGED: a callable wrapper taking its argument.
    let incr: mlua::Function = module.get("incr").unwrap();
    let r: i64 = incr.call(41).unwrap();
    assert_eq!(r, 42, "function export still works: incr 41 == 42");

    // IO-action export UNCHANGED: a wrapper that PERFORMS the action when
    // called (returning its result), not the action value itself.
    let run_it: mlua::Function = module.get("runIt").unwrap();
    let r: i64 = run_it.call(()).unwrap();
    assert_eq!(r, 99, "IO-action export performs on call: runIt () == 99");
}

#[test]
fn ffi_export_rejects_unmarshallable_types() {
    // An export signature must only use types the FFI marshaller round-trips.
    // Each rejection names the binder, the position (argument N / the result),
    // the offending type, and the crossing direction.

    // A bare type variable has no runtime representation — rejected in both an
    // argument (import) and a result (export) position.
    let e = expect_compile_error("export idf :: a -> a\nidf x = x\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'idf'",
        "argument 1",
        "argument direction",
        "the result",
        "result direction",
        "polymorphic value",
    ]);
    // The internal/freshened variable name must not leak (prettified to `a`).
    assert!(!e.contains("a890") && !e.contains("_r") && !e.contains("_lit"),
        "type variables must prettify, not leak internal names: {e}");

    // A class constraint would require a dictionary to cross.
    expect_compile_error("export addN :: Num a => a -> a\naddN x = x + x\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'addN'",
        "class constraint",
        "dictionary",
    ]);

    // A region-scoped ST handle, in both directions.
    expect_compile_error("export g :: [Int] -> ST s (STArray s)\ng xs = newSTArrayFromList xs\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'g'",
        "the result",
        "STArray",
        "region-scoped",
    ]);

    expect_compile_error("export f :: forall s. STArray s -> Int\nf _ = 5\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'f'",
        "argument 1",
        "STArray",
    ]);

    // An IO action cannot be supplied by a Lua caller (import position).
    expect_compile_error("export bad :: IO () -> Int\nbad _ = 5\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'bad'",
        "argument 1",
        "IO ()",
        "cannot supply an IO/LuaIO action",
    ]);

    // Recursion + direction-flip: a rejected type nested inside a tuple, a list,
    // and a Maybe is still caught and located.
    expect_compile_error("export t :: (Int, a) -> Int\nt (n, _) = n\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "(inside '(Int, a)')",
    ]);
    expect_compile_error("export h :: [a] -> Int\nh _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "(inside '[a]')",
    ]);
    expect_compile_error("export j :: Maybe a -> Int\nj _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "(inside 'Maybe a')",
    ]);

    // A callback whose own signature contains a rejected type. The callback's
    // RESULT is in the import direction (unwrapping its LuaIO), so an ST handle
    // there is rejected.
    expect_compile_error("export ap :: forall s. (Int -> LuaIO s (ST s (STArray s))) -> LuaIO s Int\nap f = pure 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'ap'",
        "STArray",
    ]);

    // The callback's ARGUMENT flips to the export (result) direction — a type
    // variable there is reported as a result-direction failure.
    expect_compile_error("export cb :: forall s. (a -> LuaIO s Int) -> LuaIO s Int\ncb f = pure 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'cb'",
        "result direction",
    ]);

    // A callback is marshalled ONLY as a direct top-level export argument.
    // Nested inside a container it is passed opaque by codegen, so it is
    // rejected — here a callback nested in a Maybe inside a tuple argument.
    expect_compile_error("export ap :: (Maybe (Bool -> [Int]), Int) -> Int\nap _ = 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'ap'",
        "argument 1",
        "Bool -> [Int]",
        "(inside '(Maybe (Bool -> [Int]), Int)')",
        "DIRECT top-level argument",
    ]);

    // A function nested in the RESULT is rejected (a list of functions — a bare
    // `Int -> (Bool -> Int)` would just be a two-argument export).
    expect_compile_error("export rf :: Int -> [Bool -> Int]\nrf n = [\\b -> n]\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'rf'",
        "the result",
        "Bool -> Int",
    ]);

    // A callback whose OWN argument is a callback (callback-taking-a-callback):
    // codegen passes the inner function opaque, so reject it.
    expect_compile_error("export cc :: forall s. ((Int -> Int) -> LuaIO s Int) -> LuaIO s Int\ncc _ = pure 0\nmain :: IO ()\nmain = pure ()\n", &[], &[
        "Export 'cc'",
        "callback argument",
        "Int -> Int",
    ]);
}

#[test]
fn ffi_export_deep_nesting_allowed() {
    // Deep, fully-marshallable nesting of the DESIGNED container types (tuple /
    // list / Maybe) is accepted AND round-trips — WITHOUT a nested callback (a
    // function is only marshallable as a direct top-level export argument; see
    // the reject test) and WITHOUT a bare `Either` (a plain ADT that would leak
    // as a tagged table; only a LuaTry/LuaIOCatch Either has a designed shape).
    // A second export exercises the SUPPORTED callback shape.
    let source = r#"
export deep :: (Maybe [Int], Bool) -> [Maybe (Int, String)]
deep (m, b) = case m of
    Just xs -> map (\x -> if b then Just (x, "pos") else Nothing) xs
    Nothing -> []

export cbSum :: forall s. (Int -> LuaIO s [Int]) -> LuaIO s Int
cbSum f = do
    xs <- f 3
    pure (sum xs)

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // A tuple of (Maybe of a list) and a Bool, returning a list of
    // `Maybe (Int, String)`, round-trips: `Just (x, "pos")` crosses as a
    // positional table (nil for Nothing), each inner tuple a positional table.
    let deep: mlua::Function = module.get("deep").unwrap();
    let arg = lua.create_table().unwrap();
    arg.push(lua.create_sequence_from([5, 6]).unwrap()).unwrap(); // Just [5,6]
    arg.push(true).unwrap();
    let out: mlua::Table = deep.call(arg).expect("deep tuple/list/Maybe marshals");
    let e1: mlua::Table = out.get(1).unwrap();
    assert_eq!(e1.get::<i64>(1).unwrap(), 5, "first Just tuple: value = 5");
    assert_eq!(e1.get::<String>(2).unwrap(), "pos", "first Just tuple: tag = pos");
    let e2: mlua::Table = out.get(2).unwrap();
    assert_eq!(e2.get::<i64>(1).unwrap(), 6, "second Just tuple: value = 6");
    // Nothing at the TOP of the argument's Maybe: the empty-list branch. The
    // tuple is a positional table; a `nil` at index 1 (Nothing) is set
    // explicitly by index so the Bool at index 2 keeps its slot.
    let arg2 = lua.create_table().unwrap();
    arg2.set(2, true).unwrap(); // index 1 (the Maybe) stays nil = Nothing
    let out2: mlua::Value = deep.call(arg2).unwrap();
    let empty = match out2 {
        mlua::Value::Nil => true,
        mlua::Value::Table(t) => t.raw_len() == 0,
        other => panic!("unexpected result for the empty-list branch: {other:?}"),
    };
    assert!(empty, "Nothing argument ⇒ empty result list");

    // The SUPPORTED callback shape — a top-level `(A -> LuaIO s R)` argument —
    // stays accepted (the module loaded) and runs: the host callback yields a
    // Lua array, decoded to `[Int]`, and `sum` folds it.
    let cb_sum: mlua::Function = module.get("cbSum")
        .expect("a top-level (A -> LuaIO s R) callback export is accepted");
    let cb = lua.create_function(|lua, n: i64| {
        lua.create_sequence_from((1..=n).collect::<Vec<_>>())
    }).unwrap();
    let r: i64 = cb_sum.call(cb).expect("top-level callback still works");
    assert_eq!(r, 6, "cbSum: sum (f 3) = 1+2+3");
}

#[test]
fn ffi_import_rejects_unmarshallable_types() {
    // FFI IMPORTS (LuaPure/LuaIO/LuaTry/… declarations that call INTO Lua) are
    // validated symmetrically to exports: an argument crosses OUT to the host,
    // the result crosses back IN. A plain user `data` ADT has no FFI shape (it
    // would leak as an internal tagged table), so it is rejected in BOTH.

    // ADT in an import ARGUMENT position (crosses OUT to the host).
    expect_compile_error(
        "data Color = Red | Green | Blue\n\
         paint :: Color -> LuaIO \"paint\" ()\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "FFI import 'paint'",
            "argument 1",
            "Color",
            "tagged table",
            "LuaDict",
        ],
    );

    // ADT in an import RESULT position (crosses IN from the host).
    expect_compile_error(
        "data Color = Red | Green | Blue\n\
         mkColor :: Int -> LuaIO \"mk_color\" Color\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "FFI import 'mkColor'",
            "the result",
            "Color",
        ],
    );

    // Bare `Either` in a plain (non-LuaTry) import result is also a plain ADT.
    expect_compile_error(
        "lookupIt :: String -> LuaIO \"lookup\" (Either String Int)\n\
         main :: IO ()\nmain = pure ()\n",
        &[],
        &[
            "FFI import 'lookupIt'",
            "the result",
            "Either",
        ],
    );
}

#[test]
fn ffi_marshallable_types_accepted() {
    // The full designed allowlist compiles cleanly across the FFI boundary:
    // scalars, [a], tuples, HashMap, Maybe, Any, a LuaDict record, and — the
    // critical one — a newtype over a marshallable type (the FileHandle shape).
    let source = r#"
data Cfg = Cfg { cWidth :: Int, cName :: String } deriving (Eq, LuaDict)

newtype Handle = Handle LuaUserData

-- FFI IMPORTS covering the allowlist: an argument crosses OUT, the result IN.
-- (Body-less FFI declarations; they are validated by validate_ffi_import_types.)
impScalar :: Int -> LuaPure "tostring" String
impList   :: [Int] -> LuaPure "table.unpack" Int
impMaybe  :: Maybe Int -> LuaPure "identity" (Maybe Int)
impRecord :: Cfg -> LuaPure "rawlen" Int
impHandle :: Handle -> LuaIO ":close" Handle
impTry    :: String -> LuaTry "io.open" (Either String Handle)

-- Exports covering the allowlist in argument and result positions.
export sc :: Int -> Int
sc n = n + 1

export lst :: [Int] -> [Int]
lst xs = xs

export tup :: (Int, String) -> (String, Int)
tup (n, s) = (s, n)

export hm :: HashMap String Int -> Int
hm m = hmSize m

export mb :: Maybe Int -> Maybe Int
mb x = x

export dyn :: Any -> Any
dyn x = x

export rec :: Cfg -> Int
rec c = cWidth c

-- A newtype over LuaUserData crosses transparently (the FileHandle pattern):
-- both as an argument and a result.
export passHandle :: Handle -> Handle
passHandle h = h

main :: IO ()
main = pure ()
"#;
    // Compiling at all proves the validator ACCEPTS every one of these types in
    // both directions. (Any's runtime conversion is the codegen agent's domain;
    // here we only assert the boundary check does not REJECT `Any`.)
    let (_lua, module) = compile_ffi_module(source);
    for name in ["sc", "lst", "tup", "hm", "mb", "dyn", "rec", "passHandle"] {
        let _f: mlua::Function = module.get(name)
            .unwrap_or_else(|_| panic!("export '{name}' must be present"));
    }
    // A scalar round-trips to confirm the module is live.
    let sc: mlua::Function = module.get("sc").unwrap();
    let r: i64 = sc.call(41).unwrap();
    assert_eq!(r, 42, "scalar export still works");

    // The newtype-over-LuaUserData export is a transparent wrapper: the handle
    // crosses unchanged. mlua exposes a real userdata as the Lua-standard
    // io.stdout file handle, so round-trip that and confirm identity is
    // preserved (proving no `{tag, ...}` wrapper was interposed).
    let pass_handle: mlua::Function = module.get("passHandle").unwrap();
    _lua.load("HANDLE = io.stdout").exec().unwrap();
    let handle: mlua::Value = _lua.globals().get("HANDLE").unwrap();
    assert!(matches!(handle, mlua::Value::UserData(_)),
        "io.stdout is a userdata handle");
    let back: mlua::Value = pass_handle.call(handle.clone()).unwrap();
    assert!(matches!(back, mlua::Value::UserData(_)),
        "newtype over LuaUserData passes the handle through untouched");
}

// --- Outgoing FFI callbacks (mata-ll -> Lua): the fold / threaded-state pattern.

#[test]
fn ffi_outgoing_callback_fold() {
    // A Lua host (db.fold) calls our mata-ll callback as cb(row, state) per row
    // and threads the result. Exercises a pure callback, an effectful (LuaIO s)
    // callback, and an opaque tuple state that must round-trip through Lua.
    let source = r#"
-- Pure outgoing callback: state `acc` is opaque (a polymorphic type variable).
foldRows :: String -> (Int -> acc -> acc) -> acc -> LuaPure "db.fold" acc

-- Effectful outgoing callback: returns LuaIO s acc, may do I/O per row.
foldRowsIO :: String -> (Int -> acc -> LuaIO s acc) -> acc -> LuaIO "db.fold" acc

stepIO :: Int -> Int -> LuaIO s Int
stepIO row acc = do
    liftIO (putStr "")
    pure (acc + row)

-- Pure sum into an Int accumulator (uncurry + value return).
export sumRows :: Int -> Int
sumRows seed = foldRows "select" (\row acc -> acc + row) seed

-- Opaque tuple state (sum, count): proves the state survives the Lua round-trip
-- intact (the FFI converters would otherwise flatten a tuple to a cons list).
export sumCount :: Int -> Int
sumCount _ =
    case foldRows "select" (\row acc -> case acc of (s, c) -> (s + row, c + 1)) (0, 0) of
        (s, c) -> s * 1000 + c

-- Effectful fold, returned as IO; the export wrapper runs the action.
export runEffectful :: Int -> IO Int
runEffectful seed = foldRowsIO "select" stepIO seed

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Host fold API: db.fold(query, cb, init) folds cb over rows {10, 20, 30}.
    lua.load(
        r#"
        db = {}
        function db.fold(query, cb, init)
            local rows = {10, 20, 30}
            local acc = init
            for i = 1, #rows do acc = cb(rows[i], acc) end
            return acc
        end
    "#,
    )
    .exec()
    .unwrap();

    // Pure fold: 5 + 10 + 20 + 30 = 65.
    let sum_rows: mlua::Function = module.get("sumRows").unwrap();
    let r: i64 = sum_rows.call(5).unwrap();
    assert_eq!(r, 65, "pure outgoing callback fold");

    // Opaque tuple state round-trips: sum=60, count=3 -> 60003.
    let sum_count: mlua::Function = module.get("sumCount").unwrap();
    let r: i64 = sum_count.call(0).unwrap();
    assert_eq!(r, 60003, "tuple state round-trips through Lua intact");

    // Effectful fold: 0 + 10 + 20 + 30 = 60, with the per-row action run.
    let run_eff: mlua::Function = module.get("runEffectful").unwrap();
    let r: i64 = run_eff.call(0).unwrap();
    assert_eq!(r, 60, "effectful outgoing callback fold");
}

// A declared tuple result of a LuaIO function is Lua's multi-value return,
// exactly as for LuaPure (`__mll_tup_ret`). The IO twin (`__mll_io_tup`)
// was unreachable: its selector matched `Ty::App(Con "IO", Tuple)`, a shape
// `Ty::app` normalizes to `Ty::IO(Tuple)` before it is ever inspected, so a
// `LuaIO … (String, Int)` got the single-value `__mll_io` wrapper, which
// truncates the host call to its FIRST return value.
#[test]
fn luaio_tuple_result_is_multi_return() {
    let source = r#"
getPairIO :: Int -> LuaIO "host.pairio" (String, Int)
getTripleIO :: Int -> LuaIO "host.tripleio" (Int, Bool, String)

expect :: Bool -> String -> IO ()
expect True _ = pure ()
expect False m = error m

main :: IO ()
main = do
    (s, n) <- getPairIO 5
    expect (s == "five") "first value of the IO multi-return"
    expect (n == 5) "second value of the IO multi-return"
    (a, b, c) <- getTripleIO 2
    expect (a == 4 && b == True && c == "two") "three-value IO multi-return"
    pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(
        r#"
        host = {}
        function host.pairio(n) return "five", n end
        function host.tripleio(n) return n * 2, true, "two" end
        "#,
    )
    .exec()
    .unwrap();
    // No chunk argument: the standalone form runs `main`.
    lua.load(&lua_code)
        .set_name("luaio_tuple")
        .exec()
        .expect("main's expectations hold");
}

// --- FFI result decoding: shape mismatches must fail with localized errors.

#[test]
fn ffi_decode_shape_mismatch_errors() {
    // Every shape mismatch in a value crossing the Lua FFI boundary must fail
    // with a "declared T but the host returned X" error naming WHERE (field/
    // element position and the host function) — never surface as an arbitrary
    // Lua error (nil index, arithmetic on nil) deep in user code. And the
    // checks must NOT reject valid host values (the false-positive regression
    // guarded by the n == 0 cases below).
    let source = r#"
data Cert = Cert { certName :: String, certPort :: Int }
    deriving (Show, LuaDict)

getCert :: Int -> LuaPure "host.cert" Cert
getPorts :: Int -> LuaPure "host.ports" [Int]
getPair :: Int -> LuaPure "host.pair" (String, Int)
getEntries :: Int -> LuaPure "host.entries" [(String, Int)]

export certPortOf :: Int -> Int
certPortOf n = certPort (getCert n)

sumList :: [Int] -> Int
sumList xs = case xs of
    []     -> 0
    (y:ys) -> y + sumList ys

export sumPorts :: Int -> Int
sumPorts n = sumList (getPorts n)

export pairSnd :: Int -> Int
pairSnd n =
    case getPair n of
        (_, p) -> p

sumValues :: [(String, Int)] -> Int
sumValues xs = case xs of
    []          -> 0
    ((_, v):ys) -> v + sumValues ys

export entrySum :: Int -> Int
entrySum n = sumValues (getEntries n)

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Host functions returning one valid shape (n == 0) and several broken ones.
    lua.load(
        r#"
        host = {}
        function host.cert(n)
            if n == 0 then return { certName = "ca", certPort = 443 } end
            if n == 1 then return { certName = "ca" } end          -- field missing
            if n == 2 then return "oops" end                        -- scalar, not a table
            return { certName = 7, certPort = 443 }                 -- wrong field type
        end
        function host.ports(n)
            if n == 0 then return {8000, 80, 8080} end
            if n == 1 then return 443 end                           -- scalar, not an array
            return {8000, "eighty"}                                 -- wrong element type
        end
        -- A top-level declared tuple is Lua's multi-value return convention.
        function host.pair(n)
            if n == 0 then return "a", 1 end
            if n == 1 then return "a", "b" end                      -- wrong tuple element
            return "a"                                              -- second value missing
        end
        function host.entries(n)
            if n == 0 then return { {"a", 1}, {"b", 2} } end
            if n == 1 then return { "a" } end                       -- scalar where a tuple
            return { {"a", 1}, {"b", "two"} }                       -- wrong nested element
        end
    "#,
    )
    .exec()
    .unwrap();

    // Valid shapes decode and are NOT rejected: a genuine record, list, and
    // tuple from the host all round-trip. This locks in that the scalar
    // checks fire only on real mismatches.
    let cert_port: mlua::Function = module.get("certPortOf").unwrap();
    let p: i64 = cert_port.call(0).unwrap();
    assert_eq!(p, 443, "valid record from the host decodes");
    let sum_ports: mlua::Function = module.get("sumPorts").unwrap();
    let s: i64 = sum_ports.call(0).unwrap();
    assert_eq!(s, 16160, "valid list from the host decodes");
    let pair_snd: mlua::Function = module.get("pairSnd").unwrap();
    let x: i64 = pair_snd.call(0).unwrap();
    assert_eq!(x, 1, "valid multi-return tuple from the host decodes");
    let entry_sum: mlua::Function = module.get("entrySum").unwrap();
    let x: i64 = entry_sum.call(0).unwrap();
    assert_eq!(x, 3, "valid list of tuples from the host decodes");

    // A declared record field the host left out.
    let e = cert_port.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned nil"), "got: {e}");
    assert!(e.contains("field 'certPort' of record Cert"), "got: {e}");
    assert!(e.contains("in the result of host.cert"), "got: {e}");

    // A scalar where a record was declared.
    let e = cert_port.call::<i64>(2).unwrap_err().to_string();
    assert!(e.contains("declared Cert but the host returned the string \"oops\""), "got: {e}");
    assert!(e.contains("a record must arrive from the host as a Lua table"), "got: {e}");

    // A record field of the wrong type.
    let e = cert_port.call::<i64>(3).unwrap_err().to_string();
    assert!(e.contains("declared String but the host returned the number 7"), "got: {e}");
    assert!(e.contains("field 'certName' of record Cert"), "got: {e}");

    // A scalar where a list was declared.
    let e = sum_ports.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared [Int] but the host returned the number 443"), "got: {e}");
    assert!(e.contains("a list must arrive from the host as a Lua array"), "got: {e}");
    assert!(e.contains("in the result of host.ports"), "got: {e}");

    // A list element of the wrong type.
    let e = sum_ports.call::<i64>(2).unwrap_err().to_string();
    assert!(
        e.contains("declared Int but the host returned the string \"eighty\""),
        "got: {e}"
    );
    assert!(e.contains("an element of the list declared [Int]"), "got: {e}");

    // A tuple element (multi-return value) of the wrong type.
    let e = pair_snd.call::<i64>(1).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned the string \"b\""), "got: {e}");
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");
    assert!(e.contains("in the result of host.pair"), "got: {e}");

    // A tuple element (multi-return value) the host left out entirely.
    let e = pair_snd.call::<i64>(2).unwrap_err().to_string();
    assert!(e.contains("declared Int but the host returned nil"), "got: {e}");
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");

    // A scalar where a tuple was declared (nested inside a list).
    let e = entry_sum.call::<i64>(1).unwrap_err().to_string();
    assert!(
        e.contains("declared (String, Int) but the host returned the string \"a\""),
        "got: {e}"
    );
    assert!(e.contains("a tuple must arrive from the host as a Lua array"), "got: {e}");
    assert!(e.contains("in the result of host.entries"), "got: {e}");

    // A wrong-typed element of a tuple nested inside a list.
    let e = entry_sum.call::<i64>(2).unwrap_err().to_string();
    assert!(
        e.contains("declared Int but the host returned the string \"two\""),
        "got: {e}"
    );
    assert!(e.contains("element 2 of the tuple declared (String, Int)"), "got: {e}");
}

// --- The FFI boundary is uniformly type-directed (audit findings 4, 5, 7, 8,
// --- 10, 17): every edge a value crosses — LuaTry success payloads, exported
// --- functions' arguments and results, host callbacks passed to exports, and
// --- both edges of an outgoing callback — runs the same type-directed
// --- decode/marshal machinery an ordinary FFI result/argument does.

#[test]
fn luatry_success_payload_decodes_and_error_is_stringified() {
    // Audit finding 7 (doc/audit/t9): a structured LuaTry success payload
    // (a raw Lua array where [Int] was declared) was returned undecoded
    // and later walked as a cons cell -> "attempt to index a number value".
    // And finding 17 (the LuaTry half): a non-string `err` in the Lua
    // (val, err) convention landed raw in Left :: String.
    let source = r#"
tryList   :: Int -> LuaTry "try_list" (Either String [Int])
tryNested :: Int -> LuaTry "try_nested" (Either String [[Int]])

export sumTry :: Int -> IO Int
sumTry n = do
    r <- tryList n
    case r of
        Right xs -> pure (sum xs)
        Left _   -> pure (0 - 1)

export sumNestedTry :: Int -> IO Int
sumNestedTry n = do
    r <- tryNested n
    case r of
        Right xs -> pure (sum (map sum xs))
        Left _   -> pure (0 - 1)

export errText :: Int -> IO String
errText n = do
    r <- tryList n
    case r of
        Right _ -> pure "no error"
        Left e  -> pure e

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);
    lua.load(
        r#"
        function try_list(n)
            if n == 0 then return nil, { code = 42 } end   -- non-string error object
            local r = {}
            for k = 1, n do r[k] = k end
            return r
        end
        function try_nested(n)
            local r = {}
            for k = 1, n do r[k] = { k, k * 10 } end
            return r
        end
    "#,
    )
    .exec()
    .unwrap();

    let sum_try: mlua::Function = module.get("sumTry").unwrap();
    let s: i64 = sum_try.call(3).expect("structured Right payload must decode");
    assert_eq!(s, 6, "Right [1,2,3] sums to 6");

    let sum_nested: mlua::Function = module.get("sumNestedTry").unwrap();
    let s: i64 = sum_nested.call(2).expect("nested Right payload must decode");
    assert_eq!(s, 33, "Right [[1,10],[2,20]] sums to 33");

    // A non-string error object must arrive in Left as a STRING (tostring'd),
    // so String operations on it work instead of crashing.
    let err_text: mlua::Function = module.get("errText").unwrap();
    let e: String = err_text.call(0).expect("Left of a table error must be a string");
    assert!(e.starts_with("table:"), "err tostring'd, got: {e}");
}

#[test]
fn export_arguments_decode_type_directed() {
    // Audit finding 5: exported functions cons-ified every table argument and
    // only when the TOP-LEVEL type was a list. A `Maybe Int` argument
    // never got its tagged wrapper, and structure nested under a non-list
    // argument (a tuple's list element, a record's list field, a [record])
    // crashed or corrupted.
    let source = r#"
data Tag = Tag { tName :: String, tVals :: [Int] }
    deriving (Show, Eq, LuaDict)

export pairSum :: (Int, [Int]) -> Int
pairSum (n, xs) = n + sum xs

export tagSum :: Tag -> Int
tagSum t = sum (tVals t)

export tagSums :: [Tag] -> Int
tagSums ts = sum (map tagSum ts)

export maybeOr :: Maybe Int -> Int
maybeOr (Just v) = v * 2
maybeOr Nothing  = 0 - 5

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // A tuple argument with a nested list element.
    let pair_sum: mlua::Function = module.get("pairSum").unwrap();
    let tup = lua.create_table().unwrap();
    tup.push(5).unwrap();
    tup.push(lua.create_sequence_from([1, 2, 3]).unwrap()).unwrap();
    let s: i64 = pair_sum.call(tup).expect("tuple with nested list decodes");
    assert_eq!(s, 11, "pairSum (5, [1,2,3])");

    // A record argument with a list field.
    let tag_sum: mlua::Function = module.get("tagSum").unwrap();
    let rec = lua.create_table().unwrap();
    rec.set("tName", "a").unwrap();
    rec.set("tVals", lua.create_sequence_from([1, 2, 3]).unwrap()).unwrap();
    let s: i64 = tag_sum.call(&rec).expect("record with list field decodes");
    assert_eq!(s, 6, "tagSum Tag with tVals=[1,2,3]");

    // A LIST of records: elements are decoded as records, not cons-ified.
    let tag_sums: mlua::Function = module.get("tagSums").unwrap();
    let rec2 = lua.create_table().unwrap();
    rec2.set("tName", "b").unwrap();
    rec2.set("tVals", lua.create_sequence_from([10, 20]).unwrap()).unwrap();
    let list = lua.create_table().unwrap();
    list.push(&rec).unwrap();
    list.push(&rec2).unwrap();
    let s: i64 = tag_sums.call(list).expect("[record] decodes per element");
    assert_eq!(s, 36, "tagSums over two records");

    // A Maybe argument gets its tagged wrapper: a bare host value is Just,
    // nil is Nothing.
    let maybe_or: mlua::Function = module.get("maybeOr").unwrap();
    let j: i64 = maybe_or.call(21).expect("bare value becomes Just");
    assert_eq!(j, 42, "maybeOr (Just 21)");
    let n: i64 = maybe_or.call(mlua::Value::Nil).expect("nil becomes Nothing");
    assert_eq!(n, -5, "maybeOr Nothing");

    // A shape mismatch fails with a localized ARGUMENT-direction error, not
    // silent corruption or a bare Lua error.
    let e = tag_sum.call::<i64>("oops").unwrap_err().to_string();
    assert!(e.contains("declared Tag but the host passed the string \"oops\""), "got: {e}");
    assert!(e.contains("in argument 1 of the exported function 'tagSum'"), "got: {e}");
}

#[test]
fn export_results_marshal_type_directed() {
    // Companion to finding 5, result direction: exported results went through
    // the shape-based deep-force conversion, which compacted interior
    // Nothings in a [Maybe a] (elements shifted into their slots).
    let source = r#"
export mkML :: Int -> [Maybe Int]
mkML n = map (\k -> if k `mod` 2 == 0 then Nothing else Just k) (enumFromTo 1 n)

export emptyOut :: Int -> [Int]
emptyOut n = filter (\k -> k > 100) (enumFromTo 1 n)

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);

    // Interior Nothing keeps its position as a hole; the following Just does
    // not shift into its slot. (A trailing Nothing has no Lua representation
    // — nil is the absence of a key — and stays lost, the inherent limit.)
    let mk_ml: mlua::Function = module.get("mkML").unwrap();
    let t: mlua::Table = mk_ml.call(3).expect("mkML returns a table");
    assert_eq!(t.get::<i64>(1).unwrap(), 1, "position 1 is Just 1");
    assert!(matches!(t.get::<mlua::Value>(2).unwrap(), mlua::Value::Nil),
        "position 2 is Nothing (a hole), not a shifted element");
    assert_eq!(t.get::<i64>(3).unwrap(), 3, "position 3 is Just 3");

    // An empty list result is an empty table, matching the FFI argument edge.
    let empty_out: mlua::Function = module.get("emptyOut").unwrap();
    let v: mlua::Value = empty_out.call(3).unwrap();
    let t = v.as_table().expect("empty list result must be a table, not nil");
    assert_eq!(t.raw_len(), 0, "empty exported list is an empty table");
}

#[test]
fn exported_callback_results_decode_type_directed() {
    // Audit finding 8: a host callback passed to an exported function had its
    // result converted by SHAPE (__lua_to_mll cons-ified every table), so a
    // callback returning a string-keyed table where a HashMap/record was
    // declared became nil and crashed the mata-ll consumer.
    let source = r#"
import qualified Data.Map as Map

data Pt = Pt { px :: Int, py :: Int } deriving (Show, LuaDict)

export applyM :: forall s. (Int -> LuaIO s (Map.Map String Int)) -> LuaIO s Int
applyM f = do
    mp <- f 3
    case Map.lookup "a" mp of
        Just v  -> pure v
        Nothing -> pure (0 - 99)

export applyR :: forall s. (Int -> LuaIO s Pt) -> LuaIO s Int
applyR f = do
    p <- f 2
    pure (px p * 100 + py p)

export applyMaybe :: forall s. (Int -> LuaIO s (Maybe Int)) -> LuaIO s Int
applyMaybe f = do
    m <- f 1
    case m of
        Just v  -> pure v
        Nothing -> pure (0 - 1)

export feed :: forall s. ([Int] -> LuaIO s Int) -> Int -> LuaIO s Int
feed f n = f (map (\k -> k * n) (enumFromTo 1 3))

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);

    // Map-returning callback: the string-keyed table decodes as a map.
    let apply_m: mlua::Function = module.get("applyM").unwrap();
    let cb = lua
        .load("function(n) return { a = n * 2, b = 0 } end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = apply_m.call(cb).expect("map-returning callback decodes");
    assert_eq!(v, 6, "Map.lookup \"a\" finds the callback's value");

    // Record-returning callback.
    let apply_r: mlua::Function = module.get("applyR").unwrap();
    let cb = lua
        .load("function(n) return { px = n, py = n + 1 } end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = apply_r.call(cb).expect("record-returning callback decodes");
    assert_eq!(v, 203, "Pt 2 3 -> 203");

    // Maybe-returning callback: bare value -> Just, nil -> Nothing.
    let apply_maybe: mlua::Function = module.get("applyMaybe").unwrap();
    let cb = lua.load("function(n) return n + 41 end").eval::<mlua::Function>().unwrap();
    let v: i64 = apply_maybe.call(cb).expect("bare callback result becomes Just");
    assert_eq!(v, 42);
    let cb = lua.load("function(n) return nil end").eval::<mlua::Function>().unwrap();
    let v: i64 = apply_maybe.call(cb).expect("nil callback result becomes Nothing");
    assert_eq!(v, -1);

    // And the ARGUMENT direction of the same wrapper: a list argument reaches
    // the host callback as a real Lua array it can ipairs.
    let feed: mlua::Function = module.get("feed").unwrap();
    let cb = lua
        .load("function(xs) local s = 0; for _, x in ipairs(xs) do s = s + x end; return s end")
        .eval::<mlua::Function>()
        .unwrap();
    let v: i64 = feed.call((cb, 10)).expect("list marshals out to the callback");
    assert_eq!(v, 60, "callback receives [10,20,30] as a Lua array");
}

#[test]
fn outgoing_callback_edges_agree_with_ffi_edges() {
    // Audit finding 4: an outgoing callback (a mata-ll function handed to a
    // Lua FFI function) marshalled by flags computed from the DECLARED type,
    // while the FFI call's own edges used the instantiated type. A fold whose
    // polymorphic accumulator was instantiated at a structured type had the
    // initial accumulator converted at the FFI edge but passed raw at the
    // callback edge — corrupting it silently. Both edges must use the same
    // (monomorphized) type-directed descriptors.
    let source = r#"
foldHost :: [Int] -> (Int -> acc -> acc) -> acc -> LuaPure "fold_host" acc

export listAcc :: Int -> Int
listAcc n = sum (foldHost (enumFromTo 1 n) (\x xs -> x : xs) [])

export tupleAcc :: Int -> Int
tupleAcc n =
    case foldHost (enumFromTo 1 n) (\x st -> case st of (c, xs) -> (c + 1, x : xs)) (0, []) of
        (c, xs) -> c * 1000 + sum xs

export scalarAcc :: Int -> Int
scalarAcc n = foldHost (enumFromTo 1 n) (\x c -> c + x) 0

main :: IO ()
main = pure ()
"#;
    let (lua, module) = compile_ffi_module(source);
    lua.load(
        r#"
        function fold_host(xs, f, st)
            for _, x in ipairs(xs) do st = f(x, st) end
            return st
        end
    "#,
    )
    .exec()
    .unwrap();

    // acc instantiated at [Int]: the accumulator list survives the
    // round trips through the host intact.
    let list_acc: mlua::Function = module.get("listAcc").unwrap();
    let v: i64 = list_acc.call(4).expect("[Int] accumulator round-trips");
    assert_eq!(v, 10, "sum of the accumulated list");

    // acc instantiated at (Int, [Int]): structure nested in a tuple.
    let tuple_acc: mlua::Function = module.get("tupleAcc").unwrap();
    let v: i64 = tuple_acc.call(3).expect("tuple accumulator round-trips");
    assert_eq!(v, 3006, "count 3, sum 6");

    // The scalar instantiation keeps working.
    let scalar_acc: mlua::Function = module.get("scalarAcc").unwrap();
    let v: i64 = scalar_acc.call(4).unwrap();
    assert_eq!(v, 10);
}

#[test]
fn ffi_outgoing_callback_rejects_bad_signatures() {
    // Effectful callbacks must use `LuaIO s acc`, not `IO acc`.
    expect_compile_error(
        r#"
bad :: String -> (Int -> acc -> IO acc) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
        &[],
        &[
            "LuaIO s",
        ],
    );

    // The callback's result must be the threaded state, not some other type.
    expect_compile_error(
        r#"
bad :: String -> (Int -> acc -> LuaIO s Bool) -> acc -> LuaPure "h.f" acc
main :: IO ()
main = pure ()
"#,
        &[],
        &[
            "threaded state",
        ],
    );

    // A polymorphic callback requires a polymorphic (variable) FFI return type.
    let e = expect_compile_error(
        r#"
bad :: String -> (Int -> a -> a) -> Int -> LuaPure "h.f" Int
main :: IO ()
main = pure ()
"#,
        &[],
        &[],
    );
    assert!(
        e.contains("type variable") || e.contains("threaded state"),
        "concrete state should be rejected, got: {e}"
    );
}

#[test]
fn ffi_result_marshalling_decodes_host_values() {
    // A LuaIO host returns a *raw* Lua value (arrays, dicts, nested records).
    // The compiler must decode it into the mata-ll representation per the
    // declared result type: `[record]` and `[String]` lists (tested BOTH empty
    // and non-empty) become cons lists, a `Maybe` field round-trips nil<->Nothing,
    // and scalars pass through. Regression for the FFI-boundary bugs where the
    // undecoded host value made `show` print numbers instead of the string keys
    // and `[Nothing]` for an empty (`{}`) list. The mata-ll program does its own
    // value assertions via `expect`; a decode bug makes one of them `error`.
    let source = r#"
data Params = Params { host :: String } deriving (Show, LuaDict)

data Cert = Cert { ip :: String, chain :: [Int] } deriving (Show, LuaDict)

data Resp = Resp
        { certificates :: [Cert]
        , errors :: [String]
        , note :: Maybe String
        , count :: Int }
    deriving (Show, LuaDict)

fetch :: Params -> LuaIO "luarest.fetch" Resp

expect :: Bool -> String -> IO ()
expect True _ = pure ()
expect False m = error m

len :: [a] -> Int
len [] = 0
len (_:xs) = 1 + len xs

main :: IO ()
main = do
    -- "ok" response: two certs, empty errors, present note, scalar count.
    r <- fetch (Params "ok")
    let cs = certificates r
    expect (len cs == 2) "cert list should have two elements"
    expect (ip (cs !! 0) == "1.2.3.4") "first ip must be the host string, not a number"
    expect (ip (cs !! 1) == "5.6.7.8") "second ip must be the host string, not a number"
    expect (len (chain (cs !! 0)) == 3) "nested chain list length"
    expect ((chain (cs !! 0)) !! 1 == 20) "nested chain element"
    expect (len (errors r) == 0) "empty error array must decode to the empty list"
    expect (show (errors r) == "[]") "empty error list shows as [] not [Nothing]"
    expect (count r == 42) "scalar field passes through"
    case note r of
        Just s  -> expect (s == "hi") "present Maybe field"
        Nothing -> error "note should be Just for the ok response"
    -- "bad" response: no certs, two errors, absent (nil) note.
    r2 <- fetch (Params "bad")
    expect (len (errors r2) == 2) "non-empty error list length"
    expect ((errors r2) !! 0 == "e1") "first error string"
    expect ((errors r2) !! 1 == "e2") "second error string"
    expect (len (certificates r2) == 0) "empty cert array must decode to the empty list"
    case note r2 of
        Nothing -> pure ()
        Just _  -> error "note should be Nothing when the host omits it"
    pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    // Register the Lua host `luarest.fetch`, returning a *raw* Lua value shaped
    // like a real host (arrays and dicts, not mata-ll cons cells).
    let luarest = lua.create_table().unwrap();
    let fetch = lua
        .create_function(|lua, params: mlua::Table| -> mlua::Result<mlua::Table> {
            let host: String = params.get("host")?;
            let resp = lua.create_table()?;
            if host == "ok" {
                let certs = lua.create_table()?;
                for (i, (ip, chain)) in
                    [("1.2.3.4", [10, 20, 30]), ("5.6.7.8", [1, 2, 3])].iter().enumerate()
                {
                    let c = lua.create_table()?;
                    c.set("ip", *ip)?;
                    let ch = lua.create_table()?;
                    for (j, v) in chain.iter().enumerate() {
                        ch.set(j + 1, *v)?;
                    }
                    c.set("chain", ch)?;
                    certs.set(i + 1, c)?;
                }
                resp.set("certificates", certs)?;
                resp.set("errors", lua.create_table()?)?; // empty array {}
                resp.set("note", "hi")?;
                resp.set("count", 42)?;
            } else {
                resp.set("certificates", lua.create_table()?)?; // empty array {}
                let errs = lua.create_table()?;
                errs.set(1, "e1")?;
                errs.set(2, "e2")?;
                resp.set("errors", errs)?;
                // note omitted -> nil -> Nothing
                resp.set("count", 0)?;
            }
            Ok(resp)
        })
        .unwrap();
    luarest.set("fetch", fetch).unwrap();
    lua.globals().set("luarest", luarest).unwrap();

    lua.load(&lua_code)
        .set_name("ffi_result_marshalling")
        .exec()
        .expect("host result should decode and every in-program assertion should pass");
}

#[test]
fn maybe_ffi_single_level_boundary_preserved() {
    // Interop for the common single-level case is unchanged: an exported
    // `Maybe a` marshals `Just v -> v` and `Nothing -> nil` for the Lua host.
    // (Lua's nil cannot represent nested optionals; that is an accepted limit.)
    let source = r#"
export find :: Int -> Maybe Int
find 0 = Nothing
find n = Just (n * 10)
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;
    let lua = mlua::Lua::new();
    let module: mlua::Table = lua.load(&lua_code).set_name("maybe_ffi").eval()
        .expect("should load module");
    let find: mlua::Function = module.get("find").unwrap();
    let got_nothing: mlua::Value = find.call(0i64).unwrap();
    assert!(matches!(got_nothing, mlua::Value::Nil), "Nothing should marshal to nil");
    let got_just: i64 = find.call(7i64).unwrap();
    assert_eq!(got_just, 70, "Just 70 should marshal to the bare value 70");
}

#[test]
fn luadict_on_multi_constructor_rejected() {
    // LuaDict has no tag to tell variants apart, so it only makes sense on a
    // single-constructor record. Deriving it elsewhere must fail with an
    // explanation, not silently miscompile.
    let source = r#"
data T = A { x :: Int } | B { y :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "LuaDict",
        "one constructor",
    ]);
}

#[test]
fn luadict_on_positional_fields_rejected() {
    let source = r#"
data P = P Int Int
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "LuaDict",
        "positional",
    ]);
}

#[test]
fn luadict_exported_value_is_a_named_table() {
    // A LuaDict record returned across the FFI boundary must reach Lua as a
    // real dictionary keyed by field name — not the empty table that positional
    // `ipairs` marshalling would produce. This is the whole point of LuaDict.
    let source = r#"
data Config = Config { width :: Int, height :: Int, title :: String }
  deriving (LuaDict)

export mkConfig :: Int -> Int -> Config
mkConfig w h = Config { width = w, height = h, title = "win" }

main :: IO ()
main = pure ()
"#;
    let (_lua, module) = compile_ffi_module(source);
    let mk: mlua::Function = module.get("mkConfig").unwrap();
    let cfg: mlua::Table = mk.call((80, 25)).expect("mkConfig should return a table");
    let width: i64 = cfg.get("width").expect("width key present");
    let height: i64 = cfg.get("height").expect("height key present");
    let title: String = cfg.get("title").expect("title key present");
    assert_eq!(width, 80, "named width key survives marshalling");
    assert_eq!(height, 25, "named height key survives marshalling");
    assert_eq!(title, "win", "named title key survives marshalling");
    // Positional array access must be empty — it's a dictionary, not an array.
    assert_eq!(cfg.len().unwrap(), 0, "LuaDict has no positional entries");
}

#[test]
fn luadict_renamed_keys_round_trip_ffi_boundary() {
    // `field as "key"` renames only the LuaDict table key. Both FFI directions
    // must use the renamed key: an exported record reaches Lua keyed by "key"
    // (and NOT by the Haskell field name), and a host table keyed by "key"
    // decodes back into the record — including through the type-directed
    // decoder, which the [Int] field forces (Lua array -> cons list).
    let source = r#"
data Acct = Acct
  { acctName as "name" :: String
  , acctScores as "scores" :: [Int]
  , acctActive :: Bool
  } deriving (LuaDict)

export mkAcct :: String -> Acct
mkAcct n = Acct { acctName = n, acctScores = [1, 2], acctActive = True }

fetch :: Int -> LuaIO "acct.fetch" Acct

expect :: Bool -> String -> IO ()
expect True _ = pure ()
expect False m = error m

len :: [a] -> Int
len [] = 0
len (_:xs) = 1 + len xs

main :: IO ()
main = do
    r <- fetch 1
    expect (acctName r == "zoe") "decoded renamed string key"
    expect (len (acctScores r) == 3) "decoded renamed list key length"
    expect ((acctScores r) !! 1 == 20) "decoded renamed list element"
    expect (acctActive r == True) "decoded unrenamed key"
    pure ()
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("should compile")
        .lua_code;

    let lua = mlua::Lua::new();
    // Host `acct.fetch` returns a raw Lua dict keyed by the *renamed* keys.
    let acct = lua.create_table().unwrap();
    let fetch = lua
        .create_function(|lua, _n: i64| -> mlua::Result<mlua::Table> {
            let t = lua.create_table()?;
            t.set("name", "zoe")?;
            let scores = lua.create_table()?;
            for (i, v) in [10, 20, 30].iter().enumerate() {
                scores.set(i + 1, *v)?;
            }
            t.set("scores", scores)?;
            t.set("acctActive", true)?;
            Ok(t)
        })
        .unwrap();
    acct.set("fetch", fetch).unwrap();
    lua.globals().set("acct", acct).unwrap();

    // Load as a module (chunk arg set): exports available, main skipped.
    let module: mlua::Table = lua.load(&lua_code).set_name("luadict_renamed")
        .call("luadict_renamed")
        .expect("should load module");

    // Outbound: the exported record is keyed by the renamed keys...
    let mk: mlua::Function = module.get("mkAcct").unwrap();
    let a: mlua::Table = mk.call("kim").expect("mkAcct should return a table");
    let name: String = a.get("name").expect("renamed 'name' key present");
    assert_eq!(name, "kim");
    let scores: mlua::Table = a.get("scores").expect("renamed 'scores' key present");
    assert_eq!(scores.len().unwrap(), 2);
    let active: bool = a.get("acctActive").expect("unrenamed key keeps its name");
    assert!(active);
    // ...and the Haskell field names must NOT appear as keys.
    let stray_name: mlua::Value = a.get("acctName").unwrap();
    assert!(matches!(stray_name, mlua::Value::Nil),
        "Haskell field name 'acctName' must not leak into the Lua table");
    let stray_scores: mlua::Value = a.get("acctScores").unwrap();
    assert!(matches!(stray_scores, mlua::Value::Nil),
        "Haskell field name 'acctScores' must not leak into the Lua table");

    // Inbound: run main so the fetch-and-decode assertions execute.
    let lua2 = mlua::Lua::new();
    let acct2 = lua2.create_table().unwrap();
    let fetch2 = lua2
        .create_function(|lua, _n: i64| -> mlua::Result<mlua::Table> {
            let t = lua.create_table()?;
            t.set("name", "zoe")?;
            let scores = lua.create_table()?;
            for (i, v) in [10, 20, 30].iter().enumerate() {
                scores.set(i + 1, *v)?;
            }
            t.set("scores", scores)?;
            t.set("acctActive", true)?;
            Ok(t)
        })
        .unwrap();
    acct2.set("fetch", fetch2).unwrap();
    lua2.globals().set("acct", acct2).unwrap();
    lua2.load(&lua_code).set_name("luadict_renamed_main").exec()
        .expect("host dict keyed by renamed keys should decode; every in-program assertion should pass");
}

#[test]
fn luadict_duplicate_renamed_keys_rejected() {
    // Two fields mapping to the same effective Lua key would silently
    // overwrite each other in the runtime table.
    let source = r#"
data D = D { a as "k" :: Int, b as "k" :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "LuaDict",
        "both map to the Lua key",
    ]);
}

#[test]
fn luadict_rename_colliding_with_plain_field_rejected() {
    // A rename may also collide with an *unrenamed* field's name — same
    // overwrite hazard, same rejection.
    let source = r#"
data D = D { a as "b" :: Int, b :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "LuaDict",
        "both map to the Lua key",
    ]);
}

#[test]
fn luadict_rename_without_relevant_deriving_rejected() {
    // `as` renames the field's shared external name: the LuaDict table key
    // and the JSON key of a derived ToJSON/FromJSON codec. Without any of
    // those derivings the record never crosses a boundary that keys by
    // name, so the rename would be silently meaningless. The error must
    // name all three derivings that would give the rename meaning.
    let source = r#"
data D = D { a as "k" :: Int }
    deriving (Show)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "derives none of LuaDict, ToJSON or FromJSON",
        "`deriving (LuaDict)`",
        "`deriving (ToJSON)`",
        "`deriving (FromJSON)`",
    ]);
}

#[test]
fn luadict_empty_renamed_key_rejected() {
    let source = r#"
data D = D { a as "" :: Int }
    deriving (LuaDict)

main :: IO ()
main = pure ()
"#;
    expect_compile_error(source, &[], &[
        "LuaDict",
        "empty string",
    ]);
}

// ---------------------------------------------------------------------------
// Regression tests: the FFI target string is emitted verbatim as a Lua
// callee, so it is validated at the declaration (see parser.rs
// `validate_ffi_callee`) — a malformed target is a clean compile error, never
// broken Lua.
// ---------------------------------------------------------------------------

/// An FFI target that is not a well-formed Lua callee (here: contains a
/// space) used to be pasted into a call position, emitting `a b(...)` — Lua
/// that failed to load. It must now be rejected at compile time with a
/// diagnostic naming the offending string and declaration form.
#[test]
fn ffi_target_with_space_is_rejected_at_compile_time() {
    expect_compile_error(
        r#"
foo :: Int -> LuaPure "a b" Int

export doit :: IO ()
doit = print (foo 3)
"#,
        &[],
        &[
            "invalid Lua target",
            "LuaPure \"a b\"",
        ],
    );
}

/// Other malformed shapes must be rejected the same way: an empty path
/// segment (`math..floor`) and a Lua reserved word as a name component.
#[test]
fn ffi_target_other_malformed_forms_are_rejected() {
    expect_compile_error("foo :: Int -> LuaIO \"math..floor\" Int\nmain :: IO ()\nmain = foo 3 >>= print\n", &[], &[
        "invalid Lua target",
        "math..floor",
    ]);
    expect_compile_error("foo :: Int -> LuaPure \"os.end\" Int\nmain :: IO ()\nmain = print (foo 3)\n", &[], &[
        "invalid Lua target",
        "reserved word",
    ]);
}

/// The FFI target is deliberately a Lua callee EXPRESSION, not just a name:
/// dotted paths and the arg0-method form must keep compiling — and running.
#[test]
fn ffi_target_dotted_and_method_forms_still_work() {
    let source = r#"
floorN :: Number -> LuaPure "math.floor" Int

repS :: String -> Int -> LuaPure ":rep" String

main :: IO ()
main = if floorN 3.7 == 3 && repS "ab" 2 == "abab"
         then putStrLn "ok"
         else error "dotted-path or method-form FFI produced a wrong result"
"#;
    let lua_code = compile(source, Path::new("."), &[])
        .expect("dotted-path and :method FFI targets must keep compiling")
        .lua_code;
    let lua = mlua::Lua::new();
    lua.load(&lua_code)
        .exec()
        .expect("math.floor and the string :rep method must run correctly");
}

/// Indexed paths and a dotted path with a trailing method are also legitimate
/// callee shapes; they must pass validation (compile-only — the host objects
/// don't exist in the test harness).
#[test]
fn ffi_target_indexed_and_trailing_method_forms_compile() {
    let source = r#"
runFirst :: Int -> LuaPure "handlers[1].run" Int

readCfg :: Int -> LuaPure "cfg[\"main\"].stream:read" Int

export doit :: IO ()
doit = print (runFirst 1 + readCfg 2)
"#;
    compile(source, Path::new("."), &[])
        .expect("indexed-path and path:method FFI targets must pass validation");
}
