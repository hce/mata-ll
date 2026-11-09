-- MLL Runtime
local __unpack = table.unpack or unpack

-- Thunk infrastructure (non-strict evaluation)
local __thunk_mt = {}
-- The thunk test as emitted USER code spells it (an inline ST read, see
-- codegen action.rs try_inline_st_read): an upvalue alias, so a user
-- binding that happens to be named `getmetatable` cannot shadow it.
local __mll_getmt = getmetatable
local __cons_mt = {}
-- Tags a `Just` wrapper (see the Maybe constructor below); declared here so the
-- generic `show`/`__mll_to_lua` can identify it as an upvalue.
local __just_mt = {}
-- Boxes a nil-REPRESENTED value (Nothing, [], ()) stored under a scalar
-- HashMap key: `t[k] = nil` is Lua's delete, so writing the raw value would
-- silently drop the entry (hmFromList [(7, Nothing)] must have size 1).
-- Every scalar-path write wraps nil behind this unique sentinel and every
-- read unwraps it; encoded-key maps store {k, v} entry tables and need no
-- boxing. Declared this early so the FFI marshallers can translate it at
-- the host boundary.
local __mll_hm_nilv = {}
-- Tags a SCALAR-key HashMap handle (the structural-key flavor carries
-- __mll_hme_mt instead). Declared this early so the generic __mll_to_lua
-- walker can recognize a map handle — a handle's field names would
-- otherwise be mistaken for a LuaDict record's.
local __mll_hm_mt = {}
-- Its structural-key sibling (see the HashMap runtime below).
local __mll_hme_mt = {}
-- NaN keys. Lua refuses NaN as a table index ("table index is NaN"), and
-- GHC's Data.Map — `compare nan _ = GT`, `nan == nan = False` — makes a
-- NaN key one that LANDS (the insert grows the map) and that no lookup,
-- member, delete or later insert ever matches. A scalar-path write of a
-- NaN key therefore stores under a fresh BOX `{nan, seq}` instead of the
-- NaN itself: a read with a NaN key indexes the raw NaN (allowed; it finds
-- nothing), the enumerators unbox the NaN back and place the boxes after
-- every ordinary key in insertion order (a NaN compares GT both ways, so
-- GHC enumerates a NaN inserted after the ordinary keys last), and the
-- structural encoder (__mll_key_scalar) mints a fresh token per NaN it
-- meets, which gives an encoded key the same unmatchable identity. This
-- counter numbers both. Declared this early so the FFI marshallers can
-- recognize a box.
local __mll_hm_nan_seq = 0
local function __mll_hm_nan_box(k)
    __mll_hm_nan_seq = __mll_hm_nan_seq + 1
    return {k, __mll_hm_nan_seq}
end
-- Forward declaration: the persistent-map reroot (defined with the
-- HashMap runtime below) is needed by the FFI marshallers, which are
-- defined first.
local __mll_hm_reroot
local __mll_hm_snapshot
-- Tags a suspended `pure`/`return` value — a "pure action" that has escaped its
-- defining function. `__mll_run` unwraps it WITHOUT forcing
-- or calling the payload, so a `pure ⊥` bound across a function boundary does
-- not raise until demanded, and a returned `pure <function>` is delivered as a
-- value rather than mistaken for an action closure to invoke. See gen_action.
local __mll_pure_mt = {}
local function __mll_pure(v) return setmetatable({v}, __mll_pure_mt) end
-- Unwrap a pure box to its payload (leaving anything else untouched, and NOT
-- forcing). Applied wherever the result of running an action is obtained.
local function __mll_unbox(v)
    if getmetatable(v) == __mll_pure_mt then return v[1] end
    return v
end
local function __thunk(f) return setmetatable({f, false}, __thunk_mt) end
-- Closure-free thunks: `{f, false, n, a1..an}` where f is a SHARED
-- module-level function (__mll_tkf[k], created once at load) and the
-- captured values ride in the table — no per-thunk closure, so a lazy
-- allocation is a plain table build LuaJIT can trace (FNEW aborts every
-- trace). The thunk-lift pass (thunklift.rs) rewrites eligible
-- `__thunk(function() … end)` sites to these; `{f, false}` with no arity
-- slot stays the closure form. Memoization is identical: [1] becomes the
-- value, [2] the done flag; the argument slots are cleared on evaluation
-- so captured values are not retained past the closure form's lifetime.
local function __mll_tk1(f, a) return setmetatable({f, false, 1, a}, __thunk_mt) end
local function __mll_tk2(f, a, b) return setmetatable({f, false, 2, a, b}, __thunk_mt) end
local function __mll_tk3(f, a, b, c) return setmetatable({f, false, 3, a, b, c}, __thunk_mt) end
local function __mll_tk4(f, a, b, c, d) return setmetatable({f, false, 4, a, b, c, d}, __thunk_mt) end
-- The lazy-cons GENERATOR variants: `{f, caps…}` — no metatable (a
-- `setmetatable` C call per produced cell was the closure-free tails' one
-- cost over the closures they replace on PUC Lua) and no arity slot (the
-- evaluator calls `g[1](g[2], g[3], g[4])` flat; a lifted function has
-- fixed parameters, so trailing nils are simply ignored — and a NIL
-- capture is indistinguishable from a missing one to it for the same
-- reason). The generator slot is guarded by the cell's `__lazy` flag, so
-- its consumers know the shape without a tag: `__mll_tail` evaluates a
-- table generator directly (the cell memoizes the result, exactly as for
-- a closure generator), and `__mll_tail_lazy` — which must hand back a
-- real memoizing thunk — rebuilds the struct as one.
local function __mll_gen1(f, a) return {f, a} end
local function __mll_gen2(f, a, b) return {f, a, b} end
local function __mll_gen3(f, a, b, c) return {f, a, b, c} end
local function __force(x)
    if getmetatable(x) == __thunk_mt then
        if x[2] then return x[1] end
        local n = x[3]
        local val
        -- The four-capture carrier (__mll_tk4) takes its own arm: passing
        -- a fourth (nil) argument and clearing a fourth slot on EVERY
        -- carried force measurably slowed the hot three-slot path on
        -- LuaJIT (tracker canary 8.0x -> 7.5x), so the common shapes keep
        -- their exact former sequence.
        if n == nil then val = x[1]()
        elseif n == 4 then val = x[1](x[4], x[5], x[6], x[7])
        else val = x[1](x[4], x[5], x[6])
        end
        x[1] = val
        x[2] = true
        if n ~= nil then
            x[3], x[4], x[5], x[6] = nil, nil, nil, nil
            if n == 4 then x[7] = nil end
        end
        return val
    end
    return x
end

-- seq: force the FIRST argument to WHNF, then return the SECOND evaluated to
-- WHNF (GHC's seq — the value of `seq a b` *is* the value of `b`, so forcing
-- the seq result forces `b`; this is what makes a `foldr seq z` chain fully
-- evaluate). Only WHNF: `b`'s subparts (list heads, tuple fields) keep their
-- laziness, exactly as the inline prefix/backtick lowering (`return <b>`) does.
-- Laziness is preserved at the *use site*: `__mll_seq` runs only when the seq
-- expression is itself evaluated, so a discarded `seq 1 (error "x")` (a thunked
-- binding) never calls this and never raises. This runtime primitive backs
-- every `seq` shape EXCEPT the fully-applied prefix `seq a b` and backtick
-- `a `seq` b`, which codegen lowers inline to keep `b` a proper tail call (see
-- gen_seq_inline). It is what a first-class or partially-applied `seq` becomes:
-- `foldr seq z xs`, `map (seq x) ys`, `let g = seq x in g y`. Variadic so an
-- over-applied `seq a f x y` — where the second argument is itself a function —
-- forces `a`, evaluates `f`, then applies it to the remaining arguments,
-- preserving the N-ary calling convention.
local function __mll_seq(a, b, ...)
    __force(a)
    b = __force(b)
    if select('#', ...) == 0 then return b end
    return b(...)
end

-- List primitives (internal)
--
-- HEAD-CONSUMPTION CONTRACT. A cons head is a lazy position: `x : xs` does not
-- force `x`, so a head may be an unevaluated thunk (this is what makes
-- `length [error "boom"]` return rather than raise). `__mll_head` returns the
-- head RAW — it forces the list *cell* to WHNF but NOT the element — so the two
-- kinds of consumer split as follows:
--   * A laziness-preserving consumer — one that stores the head in a new cons
--     (map/filter/take/++), passes it to a user function that decides
--     (map/foldr/zipWith), or binds it to a variable pattern — uses
--     `__mll_head` directly and never forces. (A variable-bound head is forced
--     lazily by gen_expr at each value-use.)
--   * A value-consumer — one that inspects the head itself (arithmetic, a
--     nested constructor/tuple/literal pattern, `show`, equality, indexing, or
--     a Lua builtin) — MUST wrap it: `__force(__mll_head(l))`. The generated
--     cons-pattern match does this when the head sub-pattern inspects the value
--     (see collect_pattern_conditions); runtime value-consumers do it inline.
--   * A function that RETURNS a head as its own result (`head`, `!!`) is a
--     value-consumer too, because of the runtime's WHNF-return invariant:
--     every compiled function and every codegen-emitted thunk body returns a
--     value in WHNF, never a raw thunk (compiled code emits `return
--     __force(x)` for a pattern-bound head — see gen_expr). `__force` relies
--     on that invariant and unwraps exactly ONE thunk level, so a function
--     that leaked a raw head would create a thunk-inside-a-thunk the moment a
--     call site wraps its result in `__thunk(function() return head(...)
--     end)` — and forcing that outer thunk would yield the INNER thunk as if
--     it were the value (show renders the {fn, false} pair as a 2-tuple;
--     arithmetic crashes on the table). Forcing on return does not lose
--     laziness: a call like `head(xs)` only ever executes when the
--     surrounding context demands the result to WHNF (direct calls sit in
--     strict positions; deferred ones sit inside a thunk body, which runs
--     only when forced), and at that moment Haskell forces `head xs` too.
-- Forcing `__mll_head` itself would over-force and defeat the laziness (e.g.
-- `foldr` would force elements a lazy fold never demands).
local function __mll_cons(h, t) return setmetatable({h, t}, __cons_mt) end
local function __mll_lazy_cons(h, thunk) return setmetatable({h, thunk, __lazy = true}, __cons_mt) end
-- The closure-free flavor: the generator is a `{f, caps…}` struct (see
-- __mll_gen*), marked by `__lazy = 1` so the tail readers pick the
-- evaluation form off the flag they already read — no `type()` C call per
-- cell. Every `.__lazy` truthiness test elsewhere (take's streaming arm)
-- is indifferent to which spelling marks the cell.
local function __mll_lazy_consg(h, g) return setmetatable({h, g, __lazy = 1}, __cons_mt) end
local function __mll_head(l) l = __force(l); return l[1] end
-- TAIL-CONSUMPTION CONTRACT. In GHC, `tail (x:xs) = xs` extracts the tail
-- field WITHOUT forcing it: matching a cons forces only that one cell, and the
-- extracted tail is a plain (possibly unevaluated) reference. The two tail
-- readers below split along the same line as the head readers above:
--   * __mll_tail — for a SPINE INSPECTOR: a consumer that immediately checks
--     the tail for nil / walks on (show, eq, drop, (!!), foldl, the fromList
--     converters). It forces the extracted tail to WHNF and memoizes it into
--     the cell, so the walker's `cur ~= nil` test and `cur[1]` read are sound.
--     This forces nothing GHC would not: the inspection itself is the demand.
--     It is also what a function that RETURNS a tail as its own result uses
--     (`tail`, `drop`) — the WHNF-return invariant (see the head contract)
--     forbids returning a raw thunk, and a returned tail is demanded to WHNF
--     by the very context that ran the call, exactly when Haskell forces it.
--   * __mll_tail_lazy — for a tail EXTRACTED BUT NOT INSPECTED: binding `xs`
--     in an `(x:xs)` pattern, or take's `n-1` recursion (where n-1 may be 0
--     and GHC's `take 0 xs = []` demands nothing). It forces the cell (that
--     match already happened) but yields the tail UNFORCED — pulling no
--     further spine cell, matching GHC. A lazy-cons generator is wrapped in a
--     __thunk (memoized in place, so sharing is kept) instead of being run.
--     The result may therefore be a raw thunk; that is safe everywhere a
--     pattern-bound variable can flow: gen_expr forces non-concrete variables
--     at every value-use, both runtime tail readers and __mll_head force
--     their argument at entry, and thunk bodies return WHNF so __force's
--     single unwrap suffices.
local function __mll_tail(l)
    l = __force(l)
    local lz = l.__lazy
    if lz then
        -- The suspended generator is a closure (`__lazy = true`, called) or
        -- a closure-free struct (`__lazy = 1`, its shared function applied
        -- to the carried values). The cell itself memoizes the result
        -- either way; the guard clears, so the generator runs exactly once.
        if lz == true then
            l[2] = l[2]()
        else
            local g = l[2]
            l[2] = g[1](g[2], g[3], g[4])
        end
        l.__lazy = nil
    end
    -- The tail may be an unforced thunk: a recursive cons whose tail is a
    -- variable (e.g. `x : rest`) stores it raw so the spine is not forced
    -- eagerly at construction (which would diverge on infinite lists), and a
    -- pattern-bound tail is extracted raw by __mll_tail_lazy below. Force it
    -- to WHNF here — one spine step, on demand — and memoize into the cell,
    -- so repeated walks read a plain cell, not a thunk.
    local t = l[2]
    if getmetatable(t) == __thunk_mt then
        t = __force(t)
        l[2] = t
    end
    return t
end
local function __mll_tail_lazy(l)
    l = __force(l)
    local lz = l.__lazy
    if lz then
        -- Suspend the generator instead of running it: the stored thunk
        -- memoizes on first force, so every later read (lazy or forcing)
        -- shares the one evaluation. A closure generator (`__lazy = true`)
        -- wraps in __thunk; a closure-free struct (`__lazy = 1`) rebuilds
        -- as a carried thunk.
        local g = l[2]
        if lz == true then
            g = __thunk(g)
        else
            g = setmetatable({g[1], false, 3, g[2], g[3], g[4]}, __thunk_mt)
        end
        l[2] = g
        l.__lazy = nil
    end
    return l[2]
end

-- List append (second arg is a thunk for laziness)
local __mll_append_step
local function __mll_list_append(xs, ys_thunk)
    xs = __force(xs)
    if xs == nil then
        -- The suffix suspension arrives as a closure (called) or as a
        -- closure-free thunk struct (forced) — same one evaluation.
        if getmetatable(ys_thunk) == __thunk_mt then return __force(ys_thunk) end
        return ys_thunk()
    end
    return __mll_lazy_consg(__mll_head(xs), __mll_gen2(__mll_append_step, xs, ys_thunk))
end
__mll_append_step = function(xs, ys_thunk) return __mll_list_append(__mll_tail(xs), ys_thunk) end

local function __mll_list_index(xs, n)
    n = __force(n)
    -- GHC's Prelude raises before touching the list; a negative index
    -- used to fall straight through the walk loop and return element 0.
    if n < 0 then error("Prelude.!!: negative index") end
    xs = __force(xs)
    while n > 0 do
        if xs == nil then error("(!!): index too large") end
        xs = __mll_tail(xs)
        n = n - 1
    end
    if xs == nil then error("(!!): index too large") end
    -- Force: this returns the element itself (WHNF-return invariant above);
    -- a raw head here would nest inside the caller's own thunk and escape
    -- __force's single unwrap as a bogus "value".
    return __force(__mll_head(xs))
end

-- An optional FFI argument (declared `Maybe` in the FFI signature) that sits
-- before another passed argument: unwrap `Just x` to its payload; `Nothing`
-- (nil) stays an explicit nil — Lua's own idiom for a skipped middle optional,
-- since a positional argument before another passed one cannot be omitted.
local function __mll_opt(x)
    x = __force(x)
    if getmetatable(x) == __just_mt then return __force(x[1]) end
    return x
end

-- The trailing run of optional FFI arguments, expanded in final argument
-- position: unwrap each `Just`, then drop the trailing nils and return the
-- remaining prefix as multiple values, so the callee sees `Nothing` as a
-- genuinely omitted argument — math.random(3), never math.random(3, nil),
-- which argument-count-sensitive host functions reject.
local function __mll_opt_tail(...)
    local n = select('#', ...)
    local t = {...}
    for i = 1, n do
        local v = __force(t[i])
        if getmetatable(v) == __just_mt then v = __force(v[1]) end
        t[i] = v
    end
    while n > 0 and t[n] == nil do n = n - 1 end
    return __unpack(t, 1, n)
end

-- A plain-language description of a raw host value, for FFI decode errors.
local function __mll_ffi_describe(v)
    if v == nil then return "nil" end
    local t = type(v)
    if t == "string" then return string.format("the string %q", v) end
    if t == "number" then return "the number " .. tostring(v) end
    if t == "boolean" then return "the boolean " .. tostring(v) end
    return "a " .. t .. " value"
end
-- Raise a shape-mismatch error for a value crossing the Lua FFI boundary.
-- Says WHAT was declared (desc.t), WHAT actually arrived, WHY that cannot
-- decode (`why`, optional), and WHERE: the position baked into the descriptor
-- at compile time (desc.w, e.g. "field 'ip' of record Cert") plus the
-- boundary being crossed (`root`, a full phrase such as
-- "in the result of host.cert"). `dir` = "argument" flips the wording for a
-- host-SUPPLIED value (an exported function's argument, a callback argument);
-- nil means the value is a host-returned result.
local function __mll_ffi_mismatch(desc, v, root, why, dir)
    local msg
    if dir == "argument" then
        msg = "FFI argument: declared " .. desc.t .. " but the host passed " ..
            __mll_ffi_describe(v)
    else
        msg = "FFI result: declared " .. desc.t .. " but the host returned " ..
            __mll_ffi_describe(v)
    end
    if why then msg = msg .. "; " .. why end
    local loc = {}
    if desc.w then loc[#loc + 1] = desc.w end
    if root then loc[#loc + 1] = root end
    if #loc > 0 then msg = msg .. " (" .. table.concat(loc, "; ") .. ")" end
    error(msg)
end
-- Type-directed decoder for a value that has just crossed the Lua FFI boundary
-- in the host→mata-ll direction: an FFI/LuaIO result, a LuaTry/LuaCatch
-- success payload, an exported function's argument, or a callback result.
-- `desc` is a descriptor emitted by codegen from the declared type;
-- `false`/`nil` means "already in mata-ll form, pass through". `root` is a
-- location phrase (e.g. "in the result of host.cert") threaded through for
-- error messages, and `dir` flips the message wording for host-supplied
-- arguments (see __mll_ffi_mismatch). Converts Lua arrays into cons lists,
-- validates HashMap key types, rebuilds LuaDict records field-by-field,
-- recurses through Maybe/tuples, tags a host scalar into the dynamic `Any`
-- ADT (`any`), and checks scalar leaves (`chk`) inside structures. Every
-- shape mismatch — a scalar where a list/record was
-- declared, a record field that is missing or of the wrong type — fails here,
-- localized, instead of surfacing as an arbitrary Lua error (nil index,
-- arithmetic on nil) deep in user code. Records/tuples with `rb=false` are
-- validation-only: the host's own table is returned, keeping its metatable
-- and undeclared fields.
local function __mll_ffi_decode(desc, v, root, dir)
    if not desc then return v end
    -- A thunk is a value only mata-ll itself can create (__thunk_mt is a local
    -- upvalue the host cannot reach), so meeting one here means a mata-ll
    -- value is round-tripping through the host unchanged — e.g. the threaded
    -- state of an outgoing-callback fold, whose lazy tuple fields are thunk
    -- tables. Its type was already checked at compile time; pass it through
    -- untouched. Forcing it to inspect it would both raise spurious mismatch
    -- errors and destroy the laziness the program may rely on.
    if getmetatable(v) == __thunk_mt then return v end
    local k = desc.k
    if k == "num" then
        -- A `Number` leaf (inside a structure, a LuaTry/LuaCatch payload, an
        -- exported function's argument, an iterator element). A Number is
        -- an IEEE double on every host, and a host hands numbers over in
        -- whatever subtype it has (`tonumber "5"`, `string.len`, `os.time`,
        -- a record field holding `3` — Lua's integer subtype on 5.3+): the
        -- value is normalized to the float subtype (`* 1.0` keeps a -0.0's
        -- sign; `+ 0.0` would not). nil and a non-number fail like `chk`.
        if type(v) ~= "number" then
            __mll_ffi_mismatch(desc, v, root, nil, dir)
        end
        return v * 1.0
    elseif k == "chk" then
        -- Scalar leaf inside a structure: the declared type pins down the Lua
        -- runtime type. nil (a missing field/element) also fails here.
        if type(v) ~= desc.lt then
            __mll_ffi_mismatch(desc, v, root, nil, dir)
        end
        return v
    elseif k == "list" then
        -- Lua array (1-based, possibly empty/absent) -> cons list. An empty or
        -- absent array decodes to the empty list (nil), never a bogus element.
        if v == nil then return nil end
        if type(v) ~= "table" then
            __mll_ffi_mismatch(desc, v, root,
                "a list must arrive from the host as a Lua array", dir)
        end
        local n = #v
        if desc.e and desc.e.k == "maybe" then
            -- A [Maybe a] element arrives as nil for Nothing, and `#` is not
            -- defined past a hole: scan for the maximal integer key so
            -- interior Nothings keep their positions instead of truncating
            -- the list at the first hole. (A TRAILING Nothing has no key in
            -- a Lua table at all and cannot be recovered — the inherent nil
            -- limitation, same as the argument direction.)
            for key in pairs(v) do
                if type(key) == "number" and key > n and key % 1 == 0 then
                    n = key
                end
            end
        end
        if n == 0 and next(v) ~= nil then
            __mll_ffi_mismatch(desc, v, root,
                "the table has no array part, only non-sequential keys, so it is not a Lua array", dir)
        end
        local r = nil
        for i = n, 1, -1 do
            local e = v[i]
            if desc.e then e = __mll_ffi_decode(desc.e, e, root, dir) end
            r = __mll_cons(e, r)
        end
        return r
    elseif k == "maybe" then
        -- Host nil -> Nothing; any other value -> Just <decoded payload>. `Just`
        -- is a tagged wrapper, so it must be constructed here (desc.e may be
        -- false, meaning the payload needs no further decoding). Built with
        -- __just_mt directly (the Just constructor is defined later in the
        -- prelude than this decoder).
        if v == nil then return nil end
        if desc.e then v = __mll_ffi_decode(desc.e, v, root, dir) end
        return setmetatable({v}, __just_mt)
    elseif k == "hashmap" then
        if type(v) ~= "table" then
            __mll_ffi_mismatch(desc, v, root,
                "a HashMap must arrive from the host as a keyed Lua table", dir)
        end
        local r = {}
        local n = 0
        for key, val in pairs(v) do
            local kt = type(key)
            if (desc.kt == "String" or desc.kt == "ByteString") and kt ~= "string" then
                __mll_ffi_mismatch(desc, key, root,
                    "the map is declared with String keys but this key is a " .. kt ..
                    " (e.g. a plain array); return a string-keyed table from the Lua host, " ..
                    "or declare the field as a list", dir)
            elseif (desc.kt == "Int" or desc.kt == "Number" or desc.kt == "Double") and kt ~= "number" then
                __mll_ffi_mismatch(desc, key, root,
                    "the map is declared with numeric keys but this key is a " .. kt, dir)
            end
            if desc.v then val = __mll_ffi_decode(desc.v, val, root, dir) end
            -- pairs() never yields a nil host value, but decoding can
            -- produce one (a payload whose mata-ll representation is nil);
            -- store it boxed or the entry would vanish.
            if val == nil then val = __mll_hm_nilv end
            r[key] = val
            n = n + 1
        end
        -- A fresh persistent-map root handle (see the HashMap runtime). A
        -- map declared with Number keys carries the Number flavor
        -- (`__mll_hmn_*`): the host's integral keys enumerate as doubles.
        local h = setmetatable({t = r, n = n}, __mll_hm_mt)
        if desc.kt == "Number" then h.nk = true end
        return h
    elseif k == "tuple" then
        if type(v) ~= "table" then
            __mll_ffi_mismatch(desc, v, root,
                "a tuple must arrive from the host as a Lua array", dir)
        end
        if desc.rb then
            local r = {}
            for i = 1, #desc.es do
                local e = v[i]
                if desc.es[i] then e = __mll_ffi_decode(desc.es[i], e, root, dir) end
                r[i] = e
            end
            return r
        end
        -- Validation-only: check the elements, keep the host's array. A
        -- `Number` element is the one thing written back: its normalized
        -- double replaces the host's integer-subtype value IN the host's
        -- array (the table, its metatable and any extra slots stay).
        for i = 1, #desc.es do
            local d = desc.es[i]
            if d then
                local r = __mll_ffi_decode(d, v[i], root, dir)
                if d.k == "num" then v[i] = r end
            end
        end
        return v
    elseif k == "record" then
        if type(v) ~= "table" then
            __mll_ffi_mismatch(desc, v, root,
                "a record must arrive from the host as a Lua table with its declared fields", dir)
        end
        if desc.rb then
            local r = {}
            for i = 1, #desc.fs do
                local f = desc.fs[i]
                local val = v[f.n]
                if f.d then val = __mll_ffi_decode(f.d, val, root, dir) end
                r[f.n] = val
            end
            return r
        end
        -- Validation-only: check the declared fields, keep the host's table
        -- (its metatable and any undeclared fields stay intact). A `Number`
        -- field is the one write: its normalized double replaces the
        -- host's integer-subtype value in the host's own table, so every
        -- later read of the field — a typed read, a structural show or
        -- compare — meets a double.
        for i = 1, #desc.fs do
            local f = desc.fs[i]
            if f.d then
                local r = __mll_ffi_decode(f.d, v[f.n], root, dir)
                if f.d.k == "num" then v[f.n] = r end
            end
        end
        return v
    elseif k == "any" then
        -- A host scalar -> the dynamic `Any` ADT, tagged so mata-ll code can
        -- pattern-match it. The tags are the constructor order in Prelude.mll:
        -- AnyString {1}, AnyInt {2}, AnyNumber {3}, AnyBool {4}, AnyNull {5}.
        -- nil (an absent value) is AnyNull; a number splits on its subtype so a
        -- whole number is AnyInt and a fractional/NaN/inf one is AnyNumber:
        -- native math.type on Lua 5.3+, a `% 1 == 0` test on double-only
        -- LuaJIT / 5.1-5.2. (Deliberately not __mll_math_type, whose
        -- fallback answers 'float' for every number on those interpreters —
        -- right for a subtype probe, wrong for classifying a host value.)
        if v == nil then return {5} end
        local t = type(v)
        if t == "string" then return {1, v}
        elseif t == "number" then
            local isint
            if math.type ~= nil then
                isint = math.type(v) == "integer"
            else
                isint = v % 1 == 0
            end
            if isint then return {2, v} else return {3, v} end
        elseif t == "boolean" then return {4, v}
        else
            __mll_ffi_mismatch(desc, v, root,
                "a value crossing as 'Any' must be a Lua string, number, boolean, " ..
                "or nil — 'Any' models only scalar Lua values, not a " .. t, dir)
        end
    end
    return v
end

-- The results of ONE host call as the one mata-ll value its declared type
-- names — the result rule of the FFI boundary for every form where the
-- runtime makes the call itself (LuaTry, LuaCatch/LuaIOCatch, a LuaIterator
-- step, a host callback handed to an exported function). `n`, `desc`, `root`
-- are the compiler's result decision (ffi_result in codegen/ffi.rs), `...`
-- the call's return values:
--   n      a number: the declared result is a top-level n-tuple, received
--          as Lua's multiple return values — the first n are packed into
--          one tuple table (a missing value is a nil slot, which the
--          decoder reports against the element's declared type; values
--          past the n-th are dropped). false: the result is one value, the
--          call's first.
--   desc   the decode descriptor of that value (see __mll_ffi_decode);
--          false passes it through.
--   root   the location phrase for decode errors.
-- A direct LuaPure/LuaIO call emits the same two steps inline (n locals
-- and a table constructor, then the same decoder call).
local function __mll_ffi_ret(n, desc, root, ...)
    local v
    if n then
        v = {...}
        for i = select('#', ...), n + 1, -1 do v[i] = nil end
    else
        v = (...)
    end
    if desc then return __mll_ffi_decode(desc, v, root) end
    return v
end

-- Type-directed marshalling of an mata-ll value crossing the FFI boundary in
-- the ARGUMENT direction — the dual of __mll_ffi_decode. `d` is the descriptor
-- built from the declared FFI argument type by ffi_arg_marshal_desc; it names
-- exactly the structure the host reads, so opaque values (a polymorphic
-- argument, a fold's threaded state, a plain ADT) carry no descriptor, are
-- shallow-forced by the caller, and never reach here — they pass through
-- untouched. FFI calls are strict in their arguments, so forcing here evaluates
-- nothing the call does not already demand.
--
-- NON-DESTRUCTIVE: a converted container is rebuilt into a FRESH Lua value; the
-- mata-ll value is never mutated in place, so a value passed to a host and then
-- reused in mata-ll code keeps its original representation. (A blanket __force
-- on an opaque/scalar leaf is idempotent and returns the leaf itself — shared
-- by reference into the fresh parent — which is correct: opaque leaves are not
-- rewritten.)
--
--   {k="list",e=..}    a cons list becomes a fresh 1-based Lua array; each
--                      element is forced and (if e is a descriptor, not false)
--                      recursively marshalled. Empty list (nil) -> {}.
--   {k="tuple",n=N,es} a fresh positional table; each of the N fields is forced
--                      (es[i] a nested descriptor, or false to just force).
--   {k="record",fs=..} a fresh name-keyed table; each declared field is forced
--                      (fs[i].d a nested descriptor, or false to just force) so
--                      the host reads real values.
--   {k="hashmap",v=..} a fresh string-keyed dict; each value is marshalled by v
--                      (or forced when v is false). Keys are scalars already
--                      usable as Lua keys and are kept as-is, matching the
--                      result decoder and __mll_to_lua.
--   {k="maybe",e=..}   a STRUCTURAL Maybe (record field, list element, tuple
--                      field, map value): UNWRAP it for the host — `Just x`
--                      becomes the bare `x` recursively marshalled by e (or
--                      forced when e is false), `Nothing` (nil) becomes nil.
--                      Matches __mll_to_lua and inverts the result decoder.
--   {k="just",e=..}    an OPTIONAL positional argument's payload: KEEP the `Just`
--                      wrapper (its unwrap happens later in __mll_opt/
--                      __mll_opt_tail), rebuilding a fresh wrapper around the
--                      marshalled payload, so e.g. a list inside a Just still
--                      becomes an array.
--   {k="any"}          the dynamic `Any` ADT UNWRAPPED to the bare scalar the
--                      host reads: the payload at field [2] (AnyNull's absent
--                      [2] is nil), the inverse of the decoder's `any` tagging.
local function __mll_arg_marshal(v, d)
    v = __force(v)
    if d.k == "list" then
        -- Walk the (possibly lazy) cons spine into a fresh array, exactly the
        -- conversion __mll_to_lua performs, but marshalling each element by the
        -- element descriptor so an opaque element type is passed raw rather than
        -- mangled (which a blanket __mll_to_lua would not distinguish).
        local arr = {}
        local i = 0
        local cur = v
        while cur ~= nil do
            cur = __force(cur)
            if getmetatable(cur) ~= __cons_mt then break end
            local h = __force(cur[1])
            if d.e then h = __mll_arg_marshal(h, d.e) end
            -- Use an EXPLICIT index, never `#arr + 1`: an element that marshals
            -- to nil (a `Nothing` in a `[Maybe a]`) must keep its position, not
            -- vanish and let the following element compact into its slot. (A
            -- Lua array cannot carry a length past its last non-nil element, so
            -- a *trailing* Nothing is still lost — an inherent nil limitation —
            -- but interior positions are now preserved, no shifting.)
            i = i + 1
            arr[i] = h
            cur = __mll_tail(cur)
        end
        return arr
    elseif d.k == "tuple" then
        local t = {}
        for i = 1, d.n do
            local sub = d.es[i]
            local x = __force(v[i])
            if sub then x = __mll_arg_marshal(x, sub) end
            t[i] = x
        end
        return t
    elseif d.k == "record" then
        if v == nil then return nil end
        local t = {}
        for _, f in ipairs(d.fs) do
            local x = __force(v[f.n])
            if f.d then x = __mll_arg_marshal(x, f.d) end
            t[f.n] = x
        end
        return t
    elseif d.k == "hashmap" then
        if v == nil then return nil end
        local t = {}
        -- Snapshot the store BEFORE forcing any value: forcing may read
        -- another version of the same map family, which reroots (mutates)
        -- the very table a live `pairs` would be walking.
        local ks, vs = __mll_hm_snapshot(__mll_hm_reroot(v))
        for i = 1, #ks do
            local k = ks[i]
            if type(k) == "table" then
                error("a HashMap holding a NaN key cannot cross the FFI boundary: a Lua table cannot be indexed by NaN", 2)
            end
            local x = __force(vs[i])
            -- Unbox a stored-nil value: the host convention for an absent
            -- payload is nil (the key stays absent host-side, exactly how
            -- the decoder above reads it back).
            if rawequal(x, __mll_hm_nilv) then x = nil end
            if x ~= nil and d.v then x = __mll_arg_marshal(x, d.v) end
            t[k] = x
        end
        return t
    elseif d.k == "maybe" then
        -- Structural Maybe: unwrap to the bare payload the host reads. A nested
        -- Maybe payload recurses (so `Just Nothing` flattens to nil, matching
        -- __mll_to_lua); Nothing (nil) or an already-bare value stays as-is.
        if getmetatable(v) == __just_mt then
            local p = __force(v[1])
            if d.e then return __mll_arg_marshal(p, d.e) else return p end
        end
        return v
    elseif d.k == "just" then
        -- Optional positional argument's payload: keep the Just wrapper (unwrapped
        -- later by __mll_opt/__mll_opt_tail), rebuilding a fresh wrapper so the
        -- mata-ll Maybe is not mutated.
        if getmetatable(v) == __just_mt then
            local p = __force(v[1])
            if d.e then p = __mll_arg_marshal(p, d.e) end
            return setmetatable({p}, __just_mt)
        end
        return v
    elseif d.k == "any" then
        -- The dynamic `Any` ADT UNWRAPPED to the bare scalar the host reads:
        -- the payload lives at field [2] for AnyString/AnyInt/AnyNumber/
        -- AnyBool ({tag, payload}), and AnyNull is `{5}` whose absent [2] is nil.
        -- Uniform across all five, so one `__force(v[2])` yields exactly the
        -- plain string/number/boolean/nil — the inverse of the decoder's `any`
        -- tagging. (`v` is already forced; the payload field is still lazy.)
        return __force(v[2])
    end
    return v
end

-- Deep-force an MLL value for export to Lua.
-- Converts lazy cons lists to plain Lua arrays, forces thunks, recurses into tuples.
local function __mll_to_lua(x)
    x = __force(x)
    if type(x) ~= "table" then return x end
    -- Just wrapper: hand Lua the bare payload (Nothing is already nil). Lua's nil
    -- cannot represent nesting, so `Just Nothing` flattens to nil and `Just (Just
    -- v)` flattens to v at the boundary — this unwrap keeps the common single
    -- level `Just v -> v` interop, which is all Lua can faithfully carry.
    if getmetatable(x) == __just_mt then return __mll_to_lua(x[1]) end
    -- HashMap handle: hand Lua the keyed store (rerooted to this
    -- version), stored-nil sentinels unboxed to absent keys — the same
    -- convention as the typed hashmap marshalling arm. A structural-key
    -- map has no faithful Lua-side representation (its store is keyed by
    -- an internal encoding), and the typechecker's FFI rules exclude it,
    -- so reaching one here is an internal error.
    if getmetatable(x) == __mll_hm_mt then
        local result = {}
        -- Snapshot before converting: __mll_to_lua forces values, and a
        -- forced value may reroot this family's shared store.
        local ks, vs = __mll_hm_snapshot(__mll_hm_reroot(x))
        for i = 1, #ks do
            local v = vs[i]
            if type(ks[i]) == "table" then
                error("a HashMap holding a NaN key cannot cross the FFI boundary: a Lua table cannot be indexed by NaN", 2)
            end
            if not rawequal(v, __mll_hm_nilv) then
                result[ks[i]] = __mll_to_lua(v)
            end
        end
        return result
    end
    if getmetatable(x) == __mll_hme_mt then
        error("internal: a structural-key HashMap reached the untyped FFI boundary", 2)
    end
    -- Cons list: identified by __cons_mt metatable
    if getmetatable(x) == __cons_mt then
        local result = {}
        local cur = x
        while cur ~= nil do
            cur = __force(cur)
            if getmetatable(cur) ~= __cons_mt then break end
            result[#result + 1] = __mll_to_lua(__force(cur[1]))
            cur = __mll_tail(cur)
        end
        return result
    end
    -- LuaDict record: a name-keyed table (no positional [1]). Preserve its
    -- string keys so exported functions and callbacks hand Lua a real
    -- dictionary. (Positional ADTs and tuples always fill [1]; cons lists were
    -- handled above; so a keyless [1] can only be a LuaDict or empty table.)
    -- A tuple with a LEADING nil element ({nil, 2} — an erased Nothing)
    -- has no [1] but DOES have a positive integer key: it is positional,
    -- not a LuaDict. Scan for the maximal integer key first; the dict
    -- branch below only sees tables with none.
    local maxk = 0
    for k in pairs(x) do
        if type(k) == "number" and k > maxk then maxk = k end
    end
    if maxk == 0 then
        local result = {}
        for k, v in pairs(x) do result[k] = __mll_to_lua(v) end
        return result
    end
    -- Tuple or ADT: force each element (walk to the maximal key so an
    -- interior nil element does not truncate the array)
    local result = {}
    for i = 1, maxk do result[i] = __mll_to_lua(x[i]) end
    return result
end

-- Forward declarations for mutual recursion. Every runtime symbol is a
-- local of the emitted chunk: the runtime is a guest in the host's Lua
-- state and leaks nothing into _G (a strict-globals host would refuse the
-- module otherwise); the assignments below fill these declarations.
local __lua_to_mll, __mll_wrap_callback, __mll_wrap_callback_out, __mll_wrap_callback_in
-- __mll_run is defined further down (with the runner protocol it documents)
-- but the effectful callback wrapper above it runs actions through it.
local __mll_run

-- Convert a Lua value to MLL representation at the FFI boundary.
-- Lua arrays become cons lists, functions become wrapped callbacks.
__lua_to_mll = function(x)
    if type(x) == "function" then return __mll_wrap_callback(x) end
    if type(x) ~= "table" then return x end
    if getmetatable(x) == __cons_mt then return x end
    local n = #x
    local result = nil
    for i = n, 1, -1 do result = __mll_cons(__lua_to_mll(x[i]), result) end
    return result
end

-- Wrap a Lua callback so it deep-forces all arguments before forwarding.
-- Used at the FFI boundary: Lua functions don't understand MLL thunks.
__mll_wrap_callback = function(f)
    return function(...)
        local args = {n = select('#', ...), ...}
        for i = 1, args.n do args[i] = __mll_to_lua(args[i]) end
        return __lua_to_mll(f(__unpack(args, 1, args.n)))
    end
end

-- Wrap an mata-ll callback `f` so a Lua host can call it with `n` positional
-- arguments (an outgoing callback: mata-ll function, Lua caller). mata-ll
-- functions are n-ary, so the n arguments are applied in a single call. The
-- conversions are TYPE-DIRECTED, from the callback's monomorphized type, so
-- both edges of an FFI call agree on the representation of every value:
--   descs[i]   what the host passes for argument i crosses Lua→mata-ll and is
--              decoded exactly like an FFI result ({k=...} decode descriptor);
--              {k="func"} wraps a host-passed Lua function; false passes the
--              raw value through (a scalar or an opaque value — a plain ADT,
--              userdata — which the enclosing FFI edge also passes raw).
--   run_io     the callback returns an action that must be run for its effect.
--   ret        the result crosses mata-ll→Lua: an argument-marshal descriptor
--              (see __mll_arg_marshal), true to deep-force (a scalar), or
--              false to return the raw value (opaque state round-trips
--              untouched, exactly as the enclosing FFI edge passed it in).
__mll_wrap_callback_out = function(f, n, descs, run_io, ret)
    return function(...)
        -- mata-ll functions are n-ary (all arguments at once), so collect the
        -- host's n positional arguments and apply them in a single call.
        local args = {...}
        for i = 1, n do
            local d = descs[i]
            if d then
                local v = args[i]
                if d.k == "func" then
                    if type(v) == "function" then v = __mll_wrap_callback(v) end
                else
                    v = __mll_ffi_decode(d, v, "in an argument of a mata-ll callback", "argument")
                end
                args[i] = v
            end
        end
        local r = __force(f)(__unpack(args, 1, n))
        -- Run the effectful callback's action: __mll_run's dispatch (a
        -- terminal `pure e` in the callback body returns its result boxed,
        -- not forced/called).
        if run_io then r = __mll_run(r) end
        if ret == true then return __mll_to_lua(r) end
        if ret then return __mll_arg_marshal(r, ret) end
        return r
    end
end

-- Wrap a HOST-provided Lua callback so mata-ll code can call it (an incoming
-- callback: Lua function, mata-ll caller — the dual of
-- __mll_wrap_callback_out). Used for function-typed arguments of exported
-- functions, where the declared type is known. Each of the n arguments
-- crosses mata-ll→Lua and is marshalled by its declared type (out_descs[i]:
-- an argument-marshal descriptor, or false to shallow-force an opaque/scalar
-- value); the host's results cross Lua→mata-ll as every host call's do
-- (__mll_ffi_ret with the declared result type's decision: ret_n the count
-- of a declared tuple result or false, ret_desc an FFI decode descriptor or
-- false to pass the raw value through, `root` locating decode errors).
__mll_wrap_callback_in = function(f, n, out_descs, ret_n, ret_desc, root)
    return function(...)
        local args = {...}
        for i = 1, n do
            local d = out_descs[i]
            if d then args[i] = __mll_arg_marshal(args[i], d) else args[i] = __force(args[i]) end
        end
        return __mll_ffi_ret(ret_n, ret_desc, root, f(__unpack(args, 1, n)))
    end
end

-- Run an IO action: force thunks, then call the action closure
--
-- TWO RUNNERS, ONE BOX CONVENTION. Calling an action closure returns the
-- action's result carrying AT MOST ONE pending `__mll_pure` box (produced by
-- a terminal `pure e` whose payload isn't provably safe to leave bare — see
-- gen_pure_action). `__mll_run` is the CONSUMING runner: it is used wherever
-- the result is actually bound, marshalled, or inspected, and it strips that
-- one pending box (`__mll_unbox(action())`). `__mll_run_tail` below is the
-- FORWARDING runner for `return __mll_run_tail(a)` terminals: it leaves the
-- box on and tail-calls the closure, so Lua's tail-call elimination reclaims
-- the frame and a recursive action chain (`mapM_` over a million-element
-- list) runs in constant stack. The box rides the tail chain untouched until
-- it reaches the one consuming site at the chain's root — every such site
-- (`__mll_run` itself, try_/catch_, __mll_run_st, the export and callback
-- wrappers) applies exactly one unbox to a closure-call result.
--
-- ONE ROOT APPLICATION. A direct-perform function's result needs EXACTLY ONE
-- consumer application: at most one pending closure-call, then at most one
-- unbox — which is precisely what every consuming site (__mll_run,
-- try_/catch_, the export and callback wrappers) applies to a call
-- result. The emitted terminal forms close the invariant
-- inductively: a value / FFI / fused-intrinsic terminal IS the result (no
-- pending work beyond the unbox's no-op); a `pure e` terminal is the bare
-- value or its one `__mll_pure` box (gen_pure_action); a
-- `return __mll_run_tail(a)` terminal leaves at most the one box the
-- forwarding arms preserve; and a bare `return self(...)` tail forwards the
-- callee's result unchanged — the callee's single pending application
-- simply becomes the caller's. That last form is why a direct-perform self
-- tail carries NO runner: wrapping it in `__mll_run_tail` would be a SECOND
-- application, whose `__force` is exactly the pure-payload forcing GHC
-- never performs, and whose argument position pins one stack frame per
-- recursion level where the bare form is a Lua tail call.
__mll_run = function(action)
    -- A pure action (`pure e`/`return e` that escaped its defining function)
    -- already carries its result — hand it back UNFORCED. This is the only way
    -- to distinguish "a thunk that computes which action to run" (force it to
    -- reach the closure) from "a value-action whose result happens to be a
    -- thunk or a function" (must NOT force or call it). Check before AND after
    -- forcing: the action may itself be a thunk that, once run, yields a box.
    if getmetatable(action) == __mll_pure_mt then return action[1] end
    action = __force(action)
    if getmetatable(action) == __mll_pure_mt then return action[1] end
    -- A closure whose body is a pure action returns a box (e.g. a first-class
    -- `let a = pure e`); unwrap the result of running it too.
    if type(action) == "function" then return __mll_unbox(action()) else return action end
end
-- Tail-position runner: same action dispatch as __mll_run (box-check before
-- AND after forcing, for the same thunk-vs-value-action reasons), but it
-- FORWARDS rather than consumes. The function arm is a bare `return action()`
-- — the exact syntactic form Lua eliminates the frame for — and the box arms
-- return the box ITSELF so the payload stays unforced and uncalled, and so
-- the consuming site at the chain's root strips exactly one box (unwrapping
-- here would make an action-valued payload — `pure someBoxedAction` — lose
-- its own box to the consumer's unbox). Emitted only for `return`-position
-- action terminals (gen_bind_chain / gen_tail) and effect statements whose
-- result is discarded; every value position keeps __mll_run.
local function __mll_run_tail(action)
    if getmetatable(action) == __mll_pure_mt then return action end
    action = __force(action)
    if getmetatable(action) == __mll_pure_mt then return action end
    if type(action) == "function" then return action() else return action end
end
-- runST: run the state thread AND force its result to WHNF. Demanding
-- `runST m` to WHNF is, in GHC, demanding the returned value to WHNF —
-- the state thread runs and the result is evaluated through any `pure`
-- thunk (a closure-form action whose terminal `pure e` is suspended
-- returns that suspension from __mll_run). Forcing here upholds the
-- WHNF-return invariant at the ST→pure boundary: consumers of a runST
-- value (show, arithmetic, concrete-marked bindings) may read it
-- force-free. Structured laziness is intact — WHNF of a tuple/cons/
-- constructor does not touch its fields. Do NOT use this for bind sites
-- inside a chain: there the result must stay unforced (`x <- pure ⊥`
-- binds ⊥ without raising), which is why __mll_run does not force.
local function __mll_run_st(action)
    return __force(__mll_run(action))
end

-- GHC-parity string show (showLitString). Control characters take GHC's
-- escape names, `"` and `\` are backslash-escaped, bytes above DEL are
-- numeric escapes, and GHC's `\&` rule breaks the two ambiguous
-- juxtapositions: a numeric escape followed by a literal digit
-- (`show "\1815"` is `"\181\&5"` — without `\&` the digit would extend the
-- number) and `\SO` followed by a literal `H` (`"\SO\&H"` — otherwise it
-- would read back as `\SOH`). Byte values 128-255 escape numerically by
-- byte, which matches GHC for Latin-1 code points; multi-byte encodings
-- are outside the byte-string representation's reach.
local __mll_ctrl_names = {
    [0]="NUL","SOH","STX","ETX","EOT","ENQ","ACK","BEL","BS","HT","LF","VT",
    "FF","CR","SO","SI","DLE","DC1","DC2","DC3","DC4","NAK","SYN","ETB",
    "CAN","EM","SUB","ESC","FS","GS","RS","US",
}
local function __mll_show_string(s)
    local out = {'"'}
    for i = 1, #s do
        local b = string.byte(s, i)
        local piece
        if b == 34 then piece = '\\"'
        elseif b == 92 then piece = '\\\\'
        elseif b >= 32 and b <= 126 then piece = string.char(b)
        elseif b == 10 then piece = '\\n'
        elseif b == 9 then piece = '\\t'
        elseif b == 7 then piece = '\\a'
        elseif b == 8 then piece = '\\b'
        elseif b == 12 then piece = '\\f'
        elseif b == 13 then piece = '\\r'
        elseif b == 11 then piece = '\\v'
        elseif b == 127 then piece = '\\DEL'
        elseif b > 127 then
            piece = '\\' .. b
            local nb = string.byte(s, i + 1)
            -- Digits are printable, so the next SOURCE byte being a digit
            -- is exactly "the next shown char is a digit".
            if nb ~= nil and nb >= 48 and nb <= 57 then piece = piece .. '\\&' end
        elseif b == 14 then
            piece = '\\SO'
            if string.byte(s, i + 1) == 72 then piece = piece .. '\\&' end
        else
            piece = '\\' .. __mll_ctrl_names[b]
        end
        out[#out + 1] = piece
    end
    out[#out + 1] = '"'
    return table.concat(out)
end

-- GHC-parity Double show. GHC (floatToDigits + showFloat) prints the
-- shortest decimal digit string whose reading uniquely identifies the
-- double, laid out positionally inside [0.1, 10^7) with a mandatory ".0"
-- for integral values, and as d.ddde<exp> outside that range:
--   show 1.0        == "1.0"           show 0.1  == "0.1"
--   show 12345678.0 == "1.2345678e7"   show 0.01 == "1.0e-2"
-- This is a faithful port of GHC's Burger-Dybvig implementation
-- (GHC.Internal.Float.floatToDigits), NOT a printf probe: GHC's stopping
-- bounds are strict and its tie rounds up, so in half-ulp boundary cases
-- it emits a different last digit (or one digit more) than
-- correctly-rounding shortest printers — e.g. GHC shows the double
-- 1099514114116857.25 as "1.0995141141168573e15" where %.17g yields
-- "...72e15". Verified byte-identical to GHC 9.14.1 over a 100k random-
-- bit-pattern corpus plus edge cases, on Lua 5.5 and LuaJIT.
--
-- The exact rational arithmetic runs on a little-endian base-2^24 limb
-- bignum: every limb product stays below 2^53, so the code is exact on
-- integer-less LuaJIT and never needs 5.3 operators.
-- Special values follow GHC: "NaN", "Infinity", "-Infinity"; "-0.0"
-- keeps its sign.
local __mll_show_double
do
    local floor = math.floor
    local BASE = 16777216 -- 2^24

    local function big_from(n)
        local t = {}
        while n > 0 do
            local r = n % BASE
            t[#t + 1] = r
            n = (n - r) / BASE
        end
        if #t == 0 then t[1] = 0 end
        return t
    end
    local function big_mulsmall(a, m) -- m < 2^24; in place
        local carry = 0
        for i = 1, #a do
            local v = a[i] * m + carry
            local r = v % BASE
            a[i] = r
            carry = (v - r) / BASE
        end
        while carry > 0 do
            local r = carry % BASE
            a[#a + 1] = r
            carry = (carry - r) / BASE
        end
        return a
    end
    local function big_shl(a, k) -- multiply by 2^k, in place
        local limbs = floor(k / 24)
        local rest = k - limbs * 24
        if rest > 0 then big_mulsmall(a, 2 ^ rest) end
        if limbs > 0 then
            local n = #a
            for i = n, 1, -1 do a[i + limbs] = a[i] end
            for i = 1, limbs do a[i] = 0 end
        end
        return a
    end
    local function big_mul10pow(a, k) -- multiply by 10^k, in place
        while k >= 6 do big_mulsmall(a, 1000000); k = k - 6 end
        if k > 0 then big_mulsmall(a, 10 ^ k) end
        return a
    end
    local function big_cmp(a, b)
        local na, nb = #a, #b
        while na > 1 and a[na] == 0 do na = na - 1 end
        while nb > 1 and b[nb] == 0 do nb = nb - 1 end
        if na ~= nb then return na < nb and -1 or 1 end
        for i = na, 1, -1 do
            if a[i] ~= b[i] then return a[i] < b[i] and -1 or 1 end
        end
        return 0
    end
    local function big_sub(a, b) -- a := a - b (requires a >= b)
        local borrow = 0
        for i = 1, #a do
            local v = a[i] - (b[i] or 0) - borrow
            if v < 0 then v = v + BASE; borrow = 1 else borrow = 0 end
            a[i] = v
        end
        return a
    end
    local function big_add(a, b) -- fresh a + b
        local t = {}
        local n = #a > #b and #a or #b
        local carry = 0
        for i = 1, n do
            local v = (a[i] or 0) + (b[i] or 0) + carry
            if v >= BASE then v = v - BASE; carry = 1 else carry = 0 end
            t[i] = v
        end
        if carry > 0 then t[n + 1] = carry end
        return t
    end
    local function big_copy(a)
        local t = {}
        for i = 1, #a do t[i] = a[i] end
        return t
    end

    -- floatToDigits 10 x for a positive finite double: digit array (0-9)
    -- and exponent k with x == 0.d1..dn * 10^k, exactly as GHC computes
    -- them.
    local function float_to_digits(x)
        -- decodeFloat, shifted back down for subnormals: x == f * 2^e0
        -- with f integral; normals have 2^52 <= f < 2^53, subnormals stop
        -- at the minimum exponent -1074 (scaling by 2 is always exact).
        local f, e0 = x, 0
        if f >= 9007199254740992 then      -- 2^53
            while f >= 9007199254740992 do f = f / 2; e0 = e0 + 1 end
        else
            while f < 4503599627370496 and e0 > -1074 do -- 2^52
                f = f * 2; e0 = e0 - 1
            end
        end
        local boundary = (f == 4503599627370496) -- f == 2^(p-1)
        -- x = r/s; half-ulp gaps mUp/s (up) and mDn/s (down). A boundary
        -- mantissa's predecessor is twice as close, hence the asymmetric
        -- branches.
        local r, s, mUp, mDn
        if e0 >= 0 then
            local be = big_shl(big_from(1), e0)
            if boundary then
                r = big_shl(big_from(f), e0 + 2)
                s = big_from(4)
                mUp = big_shl(big_from(1), e0 + 1)
                mDn = be
            else
                r = big_shl(big_from(f), e0 + 1)
                s = big_from(2)
                mUp = be
                mDn = big_copy(be)
            end
        else
            if e0 > -1074 and boundary then
                r = big_shl(big_from(f), 2)
                s = big_shl(big_from(1), -e0 + 2)
                mUp = big_from(2)
                mDn = big_from(1)
            else
                r = big_shl(big_from(f), 1)
                s = big_shl(big_from(1), -e0 + 1)
                mUp = big_from(1)
                mDn = big_from(1)
            end
        end
        -- k0 estimate of ceil(log10 x): GHC's rational approximation to
        -- logBase 10 2 (8651/28738) with truncating (`quot`) division,
        -- over decodeFloat's NORMALIZED exponent — for this clamped
        -- decode that is floor(log2 f) + e0.
        local bl = 0
        do
            local m = f
            while m >= 2 do m = floor(m / 2); bl = bl + 1 end
        end
        local lx = bl + e0
        local prod = lx * 8651
        local k1
        if prod >= 0 then k1 = floor(prod / 28738)
        else k1 = -floor(-prod / 28738) end
        local k = (lx >= 0) and (k1 + 1) or k1
        -- fixup: raise k until r + mUp <= 10^k * s.
        while true do
            if k >= 0 then
                local rhs = big_mul10pow(big_copy(s), k)
                if big_cmp(big_add(r, mUp), rhs) <= 0 then break end
            else
                local lhs = big_mul10pow(big_add(r, mUp), -k)
                if big_cmp(lhs, s) <= 0 then break end
            end
            k = k + 1
        end
        -- Scale into the digit-generation frame.
        if k >= 0 then
            s = big_mul10pow(big_copy(s), k)
        else
            r = big_mul10pow(r, -k)
            mUp = big_mul10pow(mUp, -k)
            mDn = big_mul10pow(mDn, -k)
        end
        -- gen: emit digits until the remainder uniquely identifies x.
        -- The low/high bounds are STRICT and the both-sides tie rounds on
        -- rn*2 vs s — GHC's exact choices.
        local digits = {}
        while true do
            big_mulsmall(r, 10)
            big_mulsmall(mUp, 10)
            big_mulsmall(mDn, 10)
            local dn = 0
            while big_cmp(r, s) >= 0 do big_sub(r, s); dn = dn + 1 end
            local low = big_cmp(r, mDn) < 0
            local high = big_cmp(big_add(r, mUp), s) > 0
            if low and not high then
                digits[#digits + 1] = dn; break
            elseif high and not low then
                digits[#digits + 1] = dn + 1; break
            elseif low and high then
                local r2 = big_copy(r)
                big_mulsmall(r2, 2)
                if big_cmp(r2, s) < 0 then digits[#digits + 1] = dn
                else digits[#digits + 1] = dn + 1 end
                break
            else
                digits[#digits + 1] = dn
            end
        end
        return digits, k
    end

    -- showFloat's layout (formatRealFloat FFGeneric Nothing).
    __mll_show_double = function(x)
        if x ~= x then return "NaN" end
        if x == math.huge then return "Infinity" end
        if x == -math.huge then return "-Infinity" end
        local sign = ""
        if x < 0 or (x == 0 and 1 / x < 0) then sign = "-"; x = -x end
        if x == 0 then return sign .. "0.0" end
        x = x + 0.0 -- a Number held as a native integer formats as its double value
        local is, e = float_to_digits(x)
        local ds = table.concat(is)
        if e < 0 or e > 7 then
            local frac = string.sub(ds, 2)
            if frac == "" then frac = "0" end
            return sign .. string.sub(ds, 1, 1) .. "." .. frac .. "e" .. (e - 1)
        end
        if e <= 0 then
            return sign .. "0." .. string.rep("0", -e) .. ds
        end
        if #ds <= e then
            return sign .. ds .. string.rep("0", e - #ds) .. ".0"
        end
        return sign .. string.sub(ds, 1, e) .. "." .. string.sub(ds, e + 1)
    end
end

-- The Int -> Number conversion of a machine number (GHC's int2Double),
-- and the canonical double of an Int on a doubles-only host. An Int has
-- no sign of zero (GHC's Int is an integer), but on LuaJIT an Int is a
-- double and its zero has two spellings: `negate 0`, `0 * (-5)`,
-- `0 `div` (-5)`, `truncate (-0.5)` all compute -0.0 (the producers stay
-- as they are — Int arithmetic is inlined and hot). Every Int operation is
-- blind to the difference (==, compare, +, table indexing), so the sign
-- shows only where the value leaves Int: text formatting (%.0f and
-- tostring print "-0") and the Number it converts to (1 / -0.0 is
-- -Infinity). Those observers go through this function first: adding +0.0
-- turns -0.0 into +0.0 (IEEE 754: the sum of opposite-signed zeros is +0
-- under round-to-nearest; LuaJIT's trace compiler does not fold `x + 0.0`
-- away, precisely because of -0.0) and leaves every other double as it
-- is. On Lua 5.3+ the same expression is the integer-to-float conversion
-- (exact below 2^53, the nearest double beyond it, as GHC's int2Double).
local function __mll_int_to_number(x) return x + 0.0 end
-- Int-position number show. On Lua 5.3+ an Int value is a native
-- integer and tostring is exact; a whole float (LuaJIT has only doubles)
-- prints via %.0f so large magnitudes never fall into e-notation, from
-- its canonical double (GHC prints every zero Int as "0"). The erased
-- `show` below takes this route for every number it cannot prove a Double.
local function __mll_show_integer(x)
    if math.type ~= nil and math.type(x) == "integer" then return tostring(x) end
    if x ~= x or x == math.huge or x == -math.huge or x % 1 ~= 0 then
        return __mll_show_double(x)
    end
    return string.format("%.0f", __mll_int_to_number(x))
end

-- Primitives that require Lua runtime dispatch
local function not_(x) return not __force(x) end
local function engage(f, ...)
    if select('#', ...) > 0 then return __force(f)(...) else return __force(f) end
end
local function liftIO(action) return action end
-- GHC's precedence rule for a shown value, read off the string a builtin
-- or derived `show` produced (exact for those strings: a negative number
-- is the only value whose show starts with '-', a constructor application
-- the only one starting with an uppercase letter and containing a space —
-- strings are quoted, lists bracketed, tuples parenthesized). A negative
-- number is parenthesized above precedence 6 (GHC's `showSignedInt`), a
-- constructor application — record syntax included — at 11. This is what
-- every builtin, derived and structural `showsPrec` applies; a
-- hand-written showsPrec decides for itself.
local function __mll_shows_prec(d, s, tail)
    d = __force(d); s = __force(s); tail = __force(tail)
    local c = string.byte(s, 1)
    if c == 45 then
        if d > 6 then return "(" .. s .. ")" .. tail end
    elseif c ~= nil and c >= 65 and c <= 90 and d > 10 and string.find(s, " ", 1, true) then
        return "(" .. s .. ")" .. tail
    end
    if tail == "" then return s end
    return s .. tail
end
local function __mll_show_arg(s)
    s = __force(s)
    -- Parenthesize a derived-Show field at argument position: a constructor
    -- application ("Con a b", "P {x = 1}") or a negative number, matching
    -- GHC's showsPrec 11. A leading '-' can only come from a number
    -- (strings are quoted, lists bracketed, constructors capitalized), and
    -- GHC parenthesizes every negative numeric field — including
    -- "-Infinity" and "-0.0" — so the bare '-' test is exact.
    local c = string.byte(s, 1)
    if c == nil then return s end
    if (c >= 65 and c <= 90 and string.find(s, " ", 1, true))
       or c == 45 then
        return "(" .. s .. ")"
    end
    return s
end
local function show(x)
    x = __force(x)
    if type(x) == "number" then
        -- Type-erased dispatch: a native integer (Lua 5.3+) is an Int,
        -- a float a Double. On LuaJIT (doubles only) a whole number prints
        -- integer-style through the Int show — the erased path cannot
        -- distinguish 1 :: Int from 1.0 :: Double there (a -0.0 prints as
        -- the Int 0); the type-directed show_Number path can and does.
        if math.type ~= nil and math.type(x) == "float" then return __mll_show_double(x) end
        return __mll_show_integer(x)
    elseif type(x) == "string" then return __mll_show_string(x)
    elseif type(x) == "boolean" then
        if x then return "True" else return "False" end
    elseif type(x) == "nil" then return "Nothing"
    elseif type(x) == "table" then
        -- An arbitrary-precision Integer is a metatable-tagged limb table; its
        -- __tostring metamethod is the decimal renderer. Detected by a marker
        -- field so this type-erased path never textually pulls the bignum lib
        -- (Integer-free programs stay lean). `==` on two Integers likewise goes
        -- through the __eq metamethod, so __mll_eq needs no change.
        local __mt = getmetatable(x)
        if __mt ~= nil and __mt.__is_integer then return tostring(x) end
        -- A Just wrapper is tagged; render it as "Just <payload>" (its payload
        -- in field [1] may itself be nil, i.e. Just Nothing). Parenthesize a
        -- payload that is a constructor application or negative number, matching
        -- GHC's showsPrec 11 (same rule as __mll_show_arg, inlined so the generic
        -- show does not depend on a helper defined later).
        if getmetatable(x) == __just_mt then
            -- The payload at argument precedence (showsPrec 11), the same
            -- rule the derived-Show fields use.
            return "Just " .. __mll_show_arg(show(x[1]))
        end
        -- A non-empty list is exactly a cons cell, identified by __cons_mt.
        -- Tuples and constructor tables are plain tables; distinguishing by
        -- shape instead (does x[2] look list-like?) misrenders a tuple whose
        -- second element happens to be a list, e.g. show (1, [2, 3]).
        if getmetatable(x) == __cons_mt then
            local parts = {}
            local cur = x
            while cur ~= nil do
                parts[#parts + 1] = show(__force(cur[1]))
                cur = __mll_tail(cur)
            end
            return "[" .. table.concat(parts, ",") .. "]"
        end
        local parts = {}
        -- Scan to the MAXIMAL integer key, not ipairs: a nil element (an
        -- erased Nothing/()/[]) is a hole ipairs stops at, truncating
        -- `show (Nothing, 2)` to "()". Holes render as their erased value
        -- (show(nil) is "Nothing"). A TRAILING nil element is
        -- representationally unrecoverable ({1, nil} stores no key 2) —
        -- the erased-arity limit; the type-directed tuple shows carry the
        -- real arity and are used wherever the element types are known.
        local maxk = 0
        for k in pairs(x) do
            if type(k) == "number" and k > maxk then maxk = k end
        end
        for i = 1, maxk do parts[i] = show(x[i]) end
        return "(" .. table.concat(parts, ",") .. ")"
    else return tostring(x) end
end
local undefined = __thunk(function() error("Prelude.undefined", 0) end)
-- Level 0: no "file:line:" position prefix on the raised message. GHC's
-- `error "boom"` delivers "boom" to a catcher; level 1 prefixed every
-- caught message with THIS function's own runtime line.
local function error_(msg) error(__force(msg), 0) end
-- MonadFail's IO/LuaIO `fail`: an ACTION that raises GHC's IOException
-- text when run (GHC: `fail s = ioError (userError s)`), through the same
-- raise `error_` uses, so `try`/`catch` deliver it exactly like an
-- `error` message. Building the action raises nothing.
local function fail_IO(msg)
    return function() error("user error (" .. __force(msg) .. ")", 0) end
end
local fail_LuaIO = fail_IO
-- Ord `max`/`min` instance methods. GHC's defaults are
--   max x y = if x <= y then y else x
--   min x y = if x <= y then x else y
-- (ties return max's SECOND argument and min's FIRST) — the if-form is kept
-- rather than math.max/math.min so the tie side and the NaN behavior match
-- GHC exactly (math.max's NaN result is platform-defined).
local function ord_max__Int(a, b) a = __force(a); b = __force(b); if a <= b then return b else return a end end
local function ord_min__Int(a, b) a = __force(a); b = __force(b); if a <= b then return a else return b end end
local function ord_max__Number(a, b) a = __force(a); b = __force(b); if a <= b then return b else return a end end
local function ord_min__Number(a, b) a = __force(a); b = __force(b); if a <= b then return a else return b end end
local function ord_max__String(a, b) a = __force(a); b = __force(b); if a <= b then return b else return a end end
local function ord_min__String(a, b) a = __force(a); b = __force(b); if a <= b then return a else return b end end
local function ord_max__ByteString(a, b) a = __force(a); b = __force(b); if a <= b then return b else return a end end
local function ord_min__ByteString(a, b) a = __force(a); b = __force(b); if a <= b then return a else return b end end
local function pure(x) return function() return x end end
local function return_(x) return function() return x end end
-- Maybe: `Just x` is a metatable-tagged one-element wrapper (tag __just_mt,
-- declared above) so it is injective even when the payload's own runtime
-- representation is nil (`Nothing`, `[]`, or a nested `Just Nothing`).
-- `Nothing` stays nil.
local function Just(x) return setmetatable({x}, __just_mt) end
local Nothing = nil
-- Bounded Int / Bounded Bool (GHC parity). On Lua 5.3+ the Int bounds are
-- exact native integers; LuaJIT has only doubles, so maxBound_Int rounds
-- to 2^63 there — the same degradation every Int past 2^53 already has.
local minBound_Int = math.mininteger or (-2^63)
local maxBound_Int = math.maxinteger or (2^63 - 1)
local minBound_Bool = false
local maxBound_Bool = true
-- LMath shims. frexp: math.frexp is compiled out of stock Lua 5.4/5.5
-- (LUA_COMPAT_MATHLIB); binding it directly made LMath.frexp a nil call
-- there while LuaJIT (which keeps it) worked. Use the native one when
-- present, else compute mantissa/exponent (x = m * 2^e with 0.5 <= |m| < 1;
-- 0, NaN and infinities pass through with exponent 0, as C's frexp does).
local function __mll_frexp(x)
    if math.frexp then return math.frexp(x) end
    if x == 0.0 or x ~= x or x == math.huge or x == -math.huge then
        return x, 0
    end
    local a = math.abs(x)
    local e = math.floor(math.log(a, 2)) + 1
    local m = a / 2.0 ^ e
    -- math.log rounding can land one step off; renormalize.
    while m >= 1.0 do m = m / 2.0; e = e + 1 end
    while m < 0.5 do m = m * 2.0; e = e - 1 end
    if x < 0 then m = -m end
    return m, e
end
-- logBase in GHC's argument order (`logBase base x`); Lua's math.log
-- takes (x, base) — binding it directly reversed the meaning under the
-- GHC-evoking name.
local function __mll_logbase(b, x) return math.log(x, b) end
local function show_Int(x) return __mll_show_integer(__force(x)) end
-- Type-directed Double show: a Number is a double on every host (the
-- producers keep it off Lua's integer subtype), and on LuaJIT every number
-- is one, so the double formatter is called unconditionally — GHC shows 3 :: Double
-- as "3.0".
local function show_Number(x) return __mll_show_double(__force(x)) end
local function show_String(x) return __mll_show_string(__force(x)) end
local function show_Bool(x) return show(x) end
-- (The type-erased container-show shims show_List_/show_Maybe and their
-- showsPrec twins are GONE: dictionary-form container show composes a
-- real dictionary — mono's structural_dict — so no well-typed
-- program reached them, and verify.rs still rejects any call that would.)
-- Unit's runtime rep is nil (same as Nothing/[]), so the type-erased generic
-- `show` cannot render it; the Show () instance dispatches here type-directedly.
local function show_Unit(x) return "()" end
-- showsPrec: the precedence rule over each type's show. The bare
-- `showsPrec` is the type-erased fallback (the showsPrec twin of `show`).
local function showsPrec(d, x, s) return __mll_shows_prec(d, show(x), s) end
local function showsPrec_Int(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_Int(x), s) end
local function showsPrec_Number(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_Number(x), s) end
local function showsPrec_String(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_String(x), s) end
local function showsPrec_Bool(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_Bool(x), s) end
local function showsPrec_Unit(d, x, s) d = __force(d); return "()" .. __force(s) end
local function eq_Int(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Number(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_String(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Bool(a, b) a = __force(a); b = __force(b); return a == b end
-- () == () is always True; both sides are nil at runtime.
local function eq_Unit(a, b) return true end
-- `/=` twins (inlined to Lua's ~= at saturated call sites, like eq_* to ==).
local function ne_Int(a, b) a = __force(a); b = __force(b); return a ~= b end
local function ne_Number(a, b) a = __force(a); b = __force(b); return a ~= b end
local function ne_String(a, b) a = __force(a); b = __force(b); return a ~= b end
local function ne_Bool(a, b) a = __force(a); b = __force(b); return a ~= b end
local function ne_Unit(a, b) return false end
-- Ord (): the single inhabitant compares EQ to itself (Ordering EQ = 2).
local function ord_lt__Unit(a, b) return false end
local function ord_gt__Unit(a, b) return false end
local function ord_le__Unit(a, b) return true end
local function ord_ge__Unit(a, b) return true end
local function ord_compare__Unit(a, b) return 2 end
-- Ord Bool (GHC parity: False < True). Lua cannot `<` booleans, so
-- compare through 0/1.
local function __mll_bool_n(x) if __force(x) then return 1 else return 0 end end
local function ord_lt__Bool(a, b) return __mll_bool_n(a) < __mll_bool_n(b) end
local function ord_gt__Bool(a, b) return __mll_bool_n(a) > __mll_bool_n(b) end
local function ord_le__Bool(a, b) return __mll_bool_n(a) <= __mll_bool_n(b) end
local function ord_ge__Bool(a, b) return __mll_bool_n(a) >= __mll_bool_n(b) end
local function ord_compare__Bool(a, b)
    local x, y = __mll_bool_n(a), __mll_bool_n(b)
    if x < y then return 1 elseif y < x then return 3 else return 2 end
end
local function ord_max__Bool(a, b) if __mll_bool_n(a) <= __mll_bool_n(b) then return __force(b) else return __force(a) end end
local function ord_min__Bool(a, b) if __mll_bool_n(a) <= __mll_bool_n(b) then return __force(a) else return __force(b) end end
local function ord_max__Unit(a, b) __force(a); return __force(b) end
local function ord_min__Unit(a, b) local r = __force(a); __force(b); return r end
local function __mll_eq(a, b) a = __force(a); b = __force(b); return a == b end
local function ord_lt__Int(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_lt__Number(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_lt__String(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_gt__Int(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_gt__Number(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_gt__String(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_le__Int(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_le__Number(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_le__String(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_ge__Int(a, b) a = __force(a); b = __force(b); return a >= b end
local function ord_ge__Number(a, b) a = __force(a); b = __force(b); return a >= b end
local function ord_ge__String(a, b) a = __force(a); b = __force(b); return a >= b end
-- ByteString is a Lua string; `<` is byte-lexicographic, same as String.
local function ord_lt__ByteString(a, b) a = __force(a); b = __force(b); return a < b end
local function ord_gt__ByteString(a, b) a = __force(a); b = __force(b); return a > b end
local function ord_le__ByteString(a, b) a = __force(a); b = __force(b); return a <= b end
local function ord_ge__ByteString(a, b) a = __force(a); b = __force(b); return a >= b end
-- compare returns the Ordering enum: LT=1, EQ=2, GT=3 (constructor index)
local function ord_compare__Int(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
-- GHC's Double compare: `<` then `==` then GT — so every NaN comparison is GT.
local function ord_compare__Number(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif a == b then return 2 else return 3 end end
local function ord_compare__String(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__ByteString(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function semigroup_String(a, b) a = __force(a); b = __force(b); return a .. b end
-- mconcat at String: the Monoid String instance overrides the class default
-- (foldr mappend mempty) with this table.concat builder — the flat fold is
-- O(total bytes^2) (each `..` copies the whole suffix), the builder is
-- linear. Forcing is identical to the default whenever the result is
-- demanded: producing the string forces every spine cell and every element,
-- left to right, exactly as the fold would (String is WHNF-atomic, so there
-- is no partial demand to preserve).
local function string_mconcat_prim(l)
    local t, n = {}, 0
    l = __force(l)
    while l ~= nil do
        n = n + 1
        t[n] = __force(l[1])
        l = __mll_tail(l)
    end
    return table.concat(t)
end
-- Numeric-class method helpers (Num / Fractional / Integral). The arithmetic
-- OPERATORS (+ - * / div mod quot rem) never reach these — they inline to Lua
-- operators / the strict div/mod/quot/rem cores. Only the NAMED methods land
-- here, and only when a program references them (tree-shaken like every other
-- helper). fromInteger/fromRational are the identity at Int/Number
-- (the representations coincide); a user Num type supplies its own via its
-- instance, so these built-in ones are pure passthroughs.
local function negate_Int(x) return -__force(x) end
local function negate_Number(x) return -__force(x) end
local function abs_Int(x) x = __force(x); if x < 0 then return -x else return x end end
-- GHC parity: abs (-0.0) is 0.0 (the `x < 0` test let -0.0 through
-- unchanged). `0.0 - x` clears the sign for both zeros; NaN falls to
-- the else and stays NaN, like GHC.
local function abs_Number(x) x = __force(x); if x <= 0 then return 0.0 - x else return x end end
local function signum_Int(x) x = __force(x); if x < 0 then return -1 elseif x > 0 then return 1 else return 0 end end
-- GHC's exact definition (GHC.Float): x > 0 -> 1, x < 0 -> -1,
-- otherwise -> x ITSELF — so signum NaN is NaN and signum (-0.0) is
-- -0.0 (runghc-confirmed; the round-3 finding's claim of -1.0 for NaN
-- was wrong, and the old code returned +0.0 for -0.0).
local function signum_Number(x) x = __force(x); if x > 0 then return 1.0 elseif x < 0 then return -1.0 else return x end end
-- fromInteger_Int / fromInteger_Number narrow an Integer to the machine type;
-- defined after the Integer library below (they use its helpers). A numeric
-- LITERAL never reaches them — Int/Number literals emit bare and Integer
-- literals use fromInteger_Integer — so these back only explicit
-- `fromInteger`/`fromIntegral` conversions.
local function recip_Number(x) return 1.0 / __force(x) end
local function fromRational_Number(x) return __force(x) end
--#include runtime_integer.lua
-- `read` for the five builtin Read instances: GHC.Read's instances over
-- Text.Read.Lex's lexer, as ONE reader shared by all five. Every instance
-- reads the same outer shape — optional surrounding white space, any
-- number of matched parenthesis layers with white space inside each
-- (`parens`), the type's item, and nothing left over — and one item: for
-- Int, Integer and Number GHC's readNumber (an optional `-` lexeme, never
-- `+`, white space, one number lexeme; at Number also the identifiers NaN
-- and Infinity, and the sign is applied after conversion, so `-0` reads
-- as -0.0 and `-Infinity` as itself); for Bool the identifier True or
-- False; for String one Haskell string literal with lexLitChar's escape
-- set. Anything else is GHC's exact "Prelude.read: no parse" — raised at
-- level 0 like head/tail's bottoms, the message IS the diagnostic. The
-- lexer is left-biased and maximal-munch exactly where ReadP's is (`<++`
-- in lexNumber, lexFrac, lexExp): `5.` leaves the `.` unread, `1e` the
-- `e`, `0x` the `x`, so each fails at the leftover check as it does in
-- GHC. The old readers were five hand-written pattern sets that drifted
-- apart: one took `+5` and a lone `(`, one needed a matched pair, none
-- knew a radix prefix or more than one paren layer, `tonumber` handed an
-- out-of-range Int on as a float, and read @String was the identity.
local function __mll_read_fail() error("Prelude.read: no parse", 0) end
-- Data.Char.isSpace over the runtime's byte strings: space, \t \n \v \f
-- \r, and byte 0xA0 — GHC's isSpace holds for U+00A0, which that byte is
-- under the Latin-1 reading show already uses for bytes above DEL.
local function __mll_read_space(s, i)
    while true do
        local c = string.byte(s, i)
        if c == 32 or c == 160 or (c ~= nil and c >= 9 and c <= 13) then i = i + 1 else return i end
    end
end
-- The value of byte `c` as a digit of `base` (2, 8, 10 or 16; hex letters
-- in either case), or nil when it is not one.
local function __mll_read_digit(c, base)
    if c == nil then return nil end
    local v
    if c <= 57 then
        if c < 48 then return nil end
        v = c - 48
    elseif c >= 97 and c <= 102 then v = c - 87
    elseif c >= 65 and c <= 70 then v = c - 55
    else return nil end
    if v < base then return v end
    return nil
end
-- One number lexeme (Text.Read.Lex lexNumber), `s[i]` being a digit. A
-- `0` followed by x/X, o/O or b/B and at least one digit of that base is
-- a radix literal (MkNumber: digits only, no fraction or exponent —
-- `0x1.8` leaves `.8` unread). Otherwise decimal digits, then a fraction
-- only when `.` is followed by a digit and an exponent only when e/E, an
-- optional sign, is followed by a digit (MkDecimal). Returns the index
-- after the lexeme, the base, the span of the integral digits, the span
-- of the fraction digits (nil, nil without one) and the exponent as a
-- signed float (nil without one; a float so a long exponent saturates to
-- +-inf instead of wrapping the integer subtype).
local function __mll_read_lex_number(s, i)
    if string.byte(s, i) == 48 then
        local c2 = string.byte(s, i + 1)
        local base
        if c2 == 120 or c2 == 88 then base = 16
        elseif c2 == 111 or c2 == 79 then base = 8
        elseif c2 == 98 or c2 == 66 then base = 2 end
        if base and __mll_read_digit(string.byte(s, i + 2), base) then
            local j = i + 3
            while __mll_read_digit(string.byte(s, j), base) do j = j + 1 end
            return j, base, i + 2, j - 1
        end
    end
    local j = i + 1
    while __mll_read_digit(string.byte(s, j), 10) do j = j + 1 end
    local ip_last = j - 1
    local fp_first, fp_last
    if string.byte(s, j) == 46 and __mll_read_digit(string.byte(s, j + 1), 10) then
        fp_first = j + 1
        j = j + 2
        while __mll_read_digit(string.byte(s, j), 10) do j = j + 1 end
        fp_last = j - 1
    end
    local ex
    local e = string.byte(s, j)
    if e == 101 or e == 69 then
        local k, sign = j + 1, 1.0
        local sc = string.byte(s, k)
        if sc == 45 then sign = -1.0; k = k + 1 elseif sc == 43 then k = k + 1 end
        if __mll_read_digit(string.byte(s, k), 10) then
            local m = k + 1
            while __mll_read_digit(string.byte(s, m), 10) do m = m + 1 end
            ex = sign * tonumber(string.sub(s, k, m - 1))
            j = m
        end
    end
    return j, 10, i, ip_last, fp_first, fp_last, ex
end
-- Digit counts up to which every value of the base is exact in a double
-- and in an int64 alike (below 2^53), so the digits fold natively; a
-- longer run is folded by the Integer library and narrowed from there.
local __mll_read_exact_digits = {[2] = 53, [8] = 17, [10] = 15, [16] = 13}
local function __mll_read_fold(base, s, first, last)
    local acc = 0
    for i = first, last do acc = acc * base + __mll_read_digit(string.byte(s, i), base) end
    return acc
end
-- readNumber's sign: one `-` lexeme, then white space. `+` is not a sign
-- in Haskell's grammar, and `-(5)` is not a negative number (after the
-- sign readNumber lexes one more lexeme and converts it; a `(` is not a
-- number), so the digits must follow directly.
local function __mll_read_signed(s, i)
    if string.byte(s, i) == 45 then return true, __mll_read_space(s, i + 1) end
    return false, i
end
-- Int: convertInt — a lexeme numberToInteger accepts (a radix literal, or
-- decimal digits with neither fraction nor exponent: `1e3` and `1.0` are
-- no parse at Int) through fromInteger. With 64-bit integers the digits
-- fold in integer arithmetic, which wraps modulo 2^64 exactly like GHC's
-- Integer -> Int narrowing (`9223372036854775808` reads as minBound); on
-- a doubles-only host the Int is the nearest double, as every Int there
-- is. `0 - v`, not `-v`: a negated zero must stay the integer 0, not -0.0.
local function __mll_read_item_int(s, i)
    local neg
    neg, i = __mll_read_signed(s, i)
    if not __mll_read_digit(string.byte(s, i), 10) then __mll_read_fail() end
    local j, base, first, last, fp_first, _, ex = __mll_read_lex_number(s, i)
    if fp_first or ex then __mll_read_fail() end
    local v
    if last - first < __mll_read_exact_digits[base] then
        v = __mll_read_fold(base, s, first, last)
    else
        v = __int_to_machine(__int_from_digits(base, s, first, last))
    end
    if neg then v = 0 - v end
    return v, j
end
-- Integer: the same lexeme, kept exact.
local function __mll_read_item_integer(s, i)
    local neg
    neg, i = __mll_read_signed(s, i)
    if not __mll_read_digit(string.byte(s, i), 10) then __mll_read_fail() end
    local j, base, first, last, fp_first, _, ex = __mll_read_lex_number(s, i)
    if fp_first or ex then __mll_read_fail() end
    local v = __int_from_digits(base, s, first, last)
    if neg then v = negate_Integer(v) end
    return v, j
end
-- convertFrac over a decimal lexeme that has a fraction or an exponent:
-- numberToRangedRational, then fromRational. A mantissa of zeros is zero
-- whatever the exponent says; otherwise the position of the first
-- significant digit plus the exponent places the value — past 10^1027 it
-- is Infinity and below 10^-1024 it is zero before any rational is built
-- (floatRange Double, widened by 3), and in between fromRational is the
-- correctly rounded double, which both hosts' decimal converters produce
-- (checked against GHC on the classic hard cases, 770-digit midpoints
-- included). The text handed to the host is the significant digits and
-- one small integer exponent, so neither leading zeros nor an exponent
-- wider than the host's integer nor the integer subtype can enter.
local function __mll_read_decimal(s, first, last, fp_first, fp_last, ex)
    local fp = ""
    if fp_first then fp = string.sub(s, fp_first, fp_last) end
    ex = ex or 0.0
    local digits = string.gsub(string.sub(s, first, last), "^0+", "")
    local first_digit
    if #digits > 0 then
        first_digit = #digits
        digits = digits .. fp
    else
        local zeros = #string.match(fp, "^0*")
        if zeros == #fp then return 0.0 end
        first_digit = -zeros
        digits = string.sub(fp, zeros + 1)
    end
    local place = first_digit + ex
    if place > 1027 then return math.huge end
    if place < -1024 then return 0.0 end
    return tonumber(digits .. "e" .. string.format("%d", ex - #fp))
end
-- Number: convertFrac — NaN, Infinity, or a number lexeme; integral
-- syntax (decimal or radix) is the exact integer's nearest double
-- (fromRational of an integer is integerToDouble), the rest goes through
-- __mll_read_decimal. The result is a float on every host, and the sign
-- is applied to the converted value (`-0` is -0.0, `-1e-400` is -0.0).
local function __mll_read_item_number(s, i)
    local neg
    neg, i = __mll_read_signed(s, i)
    local c = string.byte(s, i)
    local v, j
    if c == 78 and string.sub(s, i, i + 2) == "NaN" then
        v = 0.0 / 0.0; j = i + 3
    elseif c == 73 and string.sub(s, i, i + 7) == "Infinity" then
        v = math.huge; j = i + 8
    elseif __mll_read_digit(c, 10) then
        local base, first, last, fp_first, fp_last, ex
        j, base, first, last, fp_first, fp_last, ex = __mll_read_lex_number(s, i)
        if fp_first == nil and ex == nil then
            if last - first < __mll_read_exact_digits[base] then
                v = __mll_read_fold(base, s, first, last) * 1.0
            else
                v = __int_to_double(__int_from_digits(base, s, first, last))
            end
        else
            v = __mll_read_decimal(s, first, last, fp_first, fp_last, ex)
        end
    else
        __mll_read_fail()
    end
    if neg then v = -v end
    return v, j
end
-- Bool: the identifier True or False. A longer identifier (`Truex`,
-- `True'`) fails at the leftover check, as the longer Ident lexeme fails
-- GHC's `choose`.
local function __mll_read_item_bool(s, i)
    if string.sub(s, i, i + 3) == "True" then return true, i + 4 end
    if string.sub(s, i, i + 4) == "False" then return false, i + 5 end
    __mll_read_fail()
end
-- The named ASCII escapes of a string literal (`\NUL` .. `\US`, `\SP`,
-- `\DEL`), the inverse of show's control-name table.
local __mll_read_ascii_names = {SP = 32, DEL = 127}
for code, name in pairs(__mll_ctrl_names) do __mll_read_ascii_names[name] = code end
-- One escape of lexLitChar, `s[i]` being the byte after the backslash:
-- the escape characters a b f n r t v \ " ', a control escape `\^@` ..
-- `\^_`, a numeric escape (decimal digits, `\x`/`\X` hex, `\o`/`\O`
-- octal; every digit is consumed, as lexInteger does), or a named ASCII
-- escape with `\SOH` winning over `\SO` (the report's maximal munch: the
-- three-letter names are tried first). Returns the byte and the index
-- after the escape. A numeric escape above 255 is no parse: a byte string
-- has no cell for it (GHC reads it as one Char up to \1114111 — the
-- documented byte-string deviation; `show` never emits one).
local function __mll_read_escape(s, i)
    local e = string.byte(s, i)
    if e == nil then __mll_read_fail() end
    if e == 110 then return 10, i + 1 end
    if e == 116 then return 9, i + 1 end
    if e == 92 or e == 34 or e == 39 then return e, i + 1 end
    if e == 114 then return 13, i + 1 end
    if e == 97 then return 7, i + 1 end
    if e == 98 then return 8, i + 1 end
    if e == 102 then return 12, i + 1 end
    if e == 118 then return 11, i + 1 end
    if e == 94 then
        local c = string.byte(s, i + 1)
        if c == nil or c < 64 or c > 95 then __mll_read_fail() end
        return c - 64, i + 2
    end
    local base, j = 10, i
    if e == 120 or e == 88 then base = 16; j = i + 1
    elseif e == 111 or e == 79 then base = 8; j = i + 1 end
    local d = __mll_read_digit(string.byte(s, j), base)
    if d then
        local v = 0
        while d do
            if v <= 255 then v = v * base + d end
            j = j + 1
            d = __mll_read_digit(string.byte(s, j), base)
        end
        if v > 255 then __mll_read_fail() end
        return v, j
    end
    local code = __mll_read_ascii_names[string.sub(s, i, i + 2)]
    if code then return code, i + 3 end
    code = __mll_read_ascii_names[string.sub(s, i, i + 1)]
    if code then return code, i + 2 end
    __mll_read_fail()
end
-- One string literal (lexString), `s[i]` being the opening quote: plain
-- bytes are copied by runs, `\&` is the empty escape, a backslash
-- followed by white space is a string gap (white space up to a closing
-- backslash, contributing nothing), and every other backslash starts a
-- lexLitChar escape. An unterminated literal is no parse. Returns the
-- decoded string and the index after the closing quote.
local function __mll_read_lex_string(s, i)
    local out, n = {}, 0
    i = i + 1
    while true do
        local j = string.find(s, '["\\]', i)
        if j == nil then __mll_read_fail() end
        if j > i then n = n + 1; out[n] = string.sub(s, i, j - 1) end
        if string.byte(s, j) == 34 then return table.concat(out), j + 1 end
        local e = string.byte(s, j + 1)
        if e == 38 then
            i = j + 2
        elseif e == 32 or e == 160 or (e ~= nil and e >= 9 and e <= 13) then
            i = __mll_read_space(s, j + 1)
            if string.byte(s, i) ~= 92 then __mll_read_fail() end
            i = i + 1
        else
            local v
            v, i = __mll_read_escape(s, j + 1)
            n = n + 1; out[n] = string.char(v)
        end
    end
end
local function __mll_read_item_string(s, i)
    if string.byte(s, i) ~= 34 then __mll_read_fail() end
    return __mll_read_lex_string(s, i)
end
-- `parens` and readEither's leftover check around one item: white space,
-- any depth of `(`, the item, white space, that many `)` each followed by
-- white space, end of input.
local function __mll_read_run(s, item)
    s = __force(s)
    local i = __mll_read_space(s, 1)
    local depth = 0
    while string.byte(s, i) == 40 do depth = depth + 1; i = __mll_read_space(s, i + 1) end
    local v
    v, i = item(s, i)
    i = __mll_read_space(s, i)
    for _ = 1, depth do
        if string.byte(s, i) ~= 41 then __mll_read_fail() end
        i = __mll_read_space(s, i + 1)
    end
    if i <= #s then __mll_read_fail() end
    return v
end
local function __mll_read_int(s) return __mll_read_run(s, __mll_read_item_int) end
local function read_Integer(s) return __mll_read_run(s, __mll_read_item_integer) end
local function __mll_read_number(s) return __mll_read_run(s, __mll_read_item_number) end
local function __mll_read_bool(s) return __mll_read_run(s, __mll_read_item_bool) end
local function __mll_read_string(s) return __mll_read_run(s, __mll_read_item_string) end
-- head forces the element (a value-consumer under the head-consumption
-- contract): it RETURNS the head as its result, and the WHNF-return invariant
-- says a function may never return a raw thunk — the caller's one-level
-- __force would mistake the nested thunk for the value. `head [1, ⊥]` still
-- returns 1 (only the first element is forced), and a merely *stored*
-- `head xs` stays unevaluated because the call itself sits inside a thunk.
-- Both check for [] themselves: every internal __mll_head/__mll_tail
-- consumer (map, filter, the pattern matcher, ...) tests the cell before
-- reading it, but head/tail are the USER-facing partial functions — their
-- empty-list bottom must carry GHC's message, not a raw Lua nil-index
-- error. Level 0 like error_/undefined: the message IS the diagnostic, a
-- runtime.lua position would only obscure it.
local function head(xs)
    xs = __force(xs)
    if xs == nil then error("Prelude.head: empty list", 0) end
    return __force(__mll_head(xs))
end
local function tail(xs)
    xs = __force(xs)
    if xs == nil then error("Prelude.tail: empty list", 0) end
    return __mll_tail(xs)
end
-- map/filter/zipWith recurse through an inner `go` so the function
-- argument is forced ONCE per call, not once per element, and the head
-- read is a direct `xs[1]` — `xs` was just forced and nil-checked, which
-- is exactly what __mll_head would redo. The per-element forces this
-- removes were measured at 17 __force calls per element on the list
-- pipeline, 94% of them landing on already-plain values.
-- The shared per-element suspension of the lazy list producers: call the
-- continuation with the forced tail. A closure-free thunk carrying
-- (k, xs) replaces the `function() return go(__mll_tail(xs)) end` closure
-- each produced cell used to allocate — the FNEW that broke every LuaJIT
-- trace through a lazy pipeline.
local function __mll_step_tail(k, xs) return k(__mll_tail(xs)) end
-- The two-list step (zipWith) walks the FIRST spine first, as GHC's
-- `go [] _ = []; go _ [] = []; go (x:xs) (y:ys) = …` matches: the first
-- tail is forced, and when it ends the second tail is never touched —
-- `zipWith f [1] (2 : ⊥)` is `[f 1 2]`. Only then is the second tail
-- forced, which `go` would do first thing anyway (its first list is a
-- cons now, so the second is the next thing GHC matches). Both stay
-- direct __mll_tail reads: the generator runs in place and memoizes into
-- the cell, no suspension is allocated.
local function __mll_step_tail2(k, xs, ys)
    local xt = __mll_tail(xs)
    if xt == nil then return nil end
    return k(xt, __mll_tail(ys))
end
-- Suspended head application for the lazy generics: `f x` (and `f x y`)
-- per element, built WITHOUT running or even forcing f — GHC's
-- `map f (x:xs) = f x : map f xs` runs f only when a head is demanded,
-- so `length (map ⊥ xs)` is `length xs` and a filter/spine consumer of
-- a map never applies f to the elements it skips. The __force inside is
-- the application's own demand on f (one type probe when f is already a
-- function value).
local function __mll_ap1(f, x) return __force(f)(x) end
local function __mll_ap2(f, x, y) return __force(f)(x, y) end
local function map(f, xs)
    local function go(xs)
        xs = __force(xs)
        if xs == nil then return nil end
        return __mll_lazy_consg(__mll_tk2(__mll_ap1, f, xs[1]), __mll_gen2(__mll_step_tail, go, xs))
    end
    return go(xs)
end
local function filter(pred, xs)
    local function go(xs)
        xs = __force(xs)
        if xs == nil then return nil end
        local h = xs[1]
        if pred(h) then
            return __mll_lazy_consg(h, __mll_gen2(__mll_step_tail, go, xs))
        else
            return go(__mll_tail(xs))
        end
    end
    -- GHC parity: pred is forced only when there is an element to test
    -- (`filter ⊥ []` is []); the one force serves the whole walk.
    xs = __force(xs)
    if xs == nil then return nil end
    pred = __force(pred)
    return go(xs)
end
local __mll_take_step
local function take(n, xs)
    -- GHC: `take n _ | n <= 0 = []` — do NOT force the list when nothing is
    -- taken, so `take 0 (error "x")` is `[]`, not a crash.
    n = __force(n)
    if n <= 0 then return nil end
    xs = __force(xs)
    if xs == nil then return nil end
    -- The recursion extracts the tail LAZILY: when n - 1 is 0 the `n <= 0`
    -- clause above returns [] without touching the list, so `take 2 (1:2:⊥)`
    -- is [1, 2] exactly as in GHC — the strict __mll_tail here would pull one
    -- cell past the n requested. For n - 1 > 0 the recursive call forces the
    -- extracted tail at entry, so nothing is delayed that GHC demands.
    if xs.__lazy then
        return __mll_lazy_consg(__mll_head(xs), __mll_gen2(__mll_take_step, n - 1, xs))
    else
        -- Realized spine: build the taken prefix ITERATIVELY. The recursive
        -- form (`__mll_cons(h, take(n - 1, tail))`) cost one Lua frame per
        -- element, so `take 1000000` over a memoized, already-walked list
        -- overflowed the stack even though the lazy arm above streams in
        -- O(1). The loop builds the same eager cells by appending in place;
        -- heads stay unforced, and n reaching 0 stops BEFORE the next tail
        -- is forced (`take 2 (1:2:⊥)` is [1, 2], as in the recursion). A
        -- still-lazy cell mid-spine hands the remainder back to the lazy
        -- arm: one frame, then O(1) again.
        -- "Realized" is decided one cell at a time, by the tail field
        -- itself: a plain cell or nil is walked, an evaluated thunk is read
        -- through, and a SUSPENDED tail (an unevaluated thunk — the `rest`
        -- of `1 : rest`, a knot's own binding, a bottom) ends the walk
        -- there, exactly like a lazy cell does: the prefix so far is
        -- returned with a lazy tail that takes the remainder on demand.
        -- GHC's `take n (x : xs) = x : take (n - 1) xs` forces nothing past
        -- the cell it returns, so `head (take 2 (1 : ⊥))` is 1 and the knot
        -- `xs = 1 : ys; ys = take 3 xs` is [1, 1, 1, 1] — forcing the
        -- suspended tail here raised the bottom and re-entered the knot's
        -- own thunk (a stack overflow).
        local first = __mll_cons(__mll_head(xs), nil)
        local last = first
        n = n - 1
        while n > 0 do
            local t = xs[2]
            if getmetatable(t) == __thunk_mt then
                if not t[2] then
                    last[2] = __mll_gen2(__mll_take_step, n, xs)
                    last.__lazy = 1
                    return first
                end
                t = t[1]
            end
            xs = t
            if xs == nil then break end
            if xs.__lazy then
                last[2] = take(n, xs)
                return first
            end
            local cell = __mll_cons(__mll_head(xs), nil)
            last[2] = cell
            last = cell
            n = n - 1
        end
        return first
    end
end
__mll_take_step = function(n, xs) return take(n, __mll_tail_lazy(xs)) end
local function drop(n, xs)
    n = __force(n); xs = __force(xs)
    while n > 0 and xs ~= nil do
        xs = __mll_tail(xs)
        n = n - 1
    end
    return xs
end
local function zipWith(f, xs, ys)
    -- Lazy in f, heads suspended — see map. GHC's `go [] _ = []` comes
    -- first: the second list is forced only once the first is a cons
    -- (`zipWith f [] ⊥` is []); the step (__mll_step_tail2) keeps that
    -- order for the tails.
    local function go(xs, ys)
        xs = __force(xs)
        if xs == nil then return nil end
        ys = __force(ys)
        if ys == nil then return nil end
        return __mll_lazy_consg(__mll_tk3(__mll_ap2, f, xs[1], ys[1]), __mll_gen3(__mll_step_tail2, go, xs, ys))
    end
    return go(xs, ys)
end
-- Type-erased Foldable fallbacks. Typed code dispatches foldr/foldl to
-- foldr_List/foldr_Maybe/foldr_Either at compile time; these run only in
-- type-erased generic contexts (the same role `map` plays for fmap and the
-- generic `show` plays for Show). They handle the structures whose runtime
-- shape is self-describing: nil ([] and Nothing both fold to the seed),
-- cons lists, and Just wrappers. Either cannot be recognized type-erased
-- (Left/Right are plain constructor tables), so it fails loudly.
-- foldr keeps the compiled version's laziness: the recursive fold is a
-- thunk, so a short-circuiting f terminates on infinite lists.
local function foldr(f, z, t)
    -- Lazy in f until there is a structure to fold (foldr ⊥ z [] is z).
    t = __force(t)
    if t == nil then return __force(z) end
    f = __force(f)
    local mt = getmetatable(t)
    if mt == __just_mt then return f(t[1], z) end
    if mt == __cons_mt then
        return f(__mll_head(t), __thunk(function()
            return foldr(f, z, __mll_tail(t))
        end))
    end
    error("foldr: type-erased fold over a structure that is not a list or Maybe")
end
local function foldl(f, z, t)
    -- Lazy in f until there is a structure to fold (foldl ⊥ z [] is z).
    t = __force(t)
    if t == nil then return __force(z) end
    f = __force(f)
    local mt = getmetatable(t)
    if mt == __just_mt then return f(z, t[1]) end
    if mt ~= __cons_mt then
        error("foldl: type-erased fold over a structure that is not a list or Maybe")
    end
    -- Each step SUSPENDS `f acc x`, exactly as the compiled `[]` instance
    -- does (`foldl f (f acc x) xs` builds a thunk carrier per element):
    -- GHC's foldl is lazy in the accumulator, so `foldl (\_ x -> x `seq`
    -- 1) 0 [undefined, []]` is 1 — the first application is never
    -- demanded. Applying f eagerly per element ran it on `undefined`.
    -- The chain is forced once, at the end, like the instance's
    -- `__force(acc)` on the empty-structure path.
    local acc = z
    local cur = t
    while cur ~= nil do
        local prev, x = acc, __mll_head(cur)
        acc = __thunk(function() return f(prev, x) end)
        cur = __mll_tail(cur)
    end
    return __force(acc)
end
-- The type-erased twin of the Foldable `foldl'` method (its Lua name:
-- sanitize_name turns the prime into `_prime`), reached exactly like the
-- erased foldl/foldr above — a use whose element type never resolved
-- (`length [error "x"]`) cannot dispatch to the `[]` instance. GHC's
-- foldl' (GHC.List's for lists, Data.Foldable's default for Maybe — the
-- same rule) forces the INCOMING accumulator before each application: the
-- seed is forced whenever the structure is non-empty (⊥ as the seed
-- raises on `[1]` and on `Just 1`), every application's result is the
-- next step's forced accumulator, and on an empty structure the seed is
-- returned untouched — forced here only because a function's result is
-- demanded to WHNF anyway, as is the last application's.
local function foldl_prime(f, z, t)
    t = __force(t)
    if t == nil then return __force(z) end
    f = __force(f)
    local mt = getmetatable(t)
    if mt == __just_mt then return __force(f(__force(z), t[1])) end
    if mt ~= __cons_mt then
        error("foldl': type-erased fold over a structure that is not a list or Maybe")
    end
    local acc = __force(z)
    local cur = t
    while cur ~= nil do
        acc = __force(f(acc, __mll_head(cur)))
        cur = __mll_tail(cur)
    end
    return acc
end
-- Currying adapters. Compiled mata-ll functions are N-ary (all
-- count_arrows(type) arguments in one call), and compiled generic code is
-- specialized per instantiation so its call sites always match. Every runtime
-- definition that calls a function value it was handed (map, zipWith, the
-- erased folds, the IORef/ST modifiers, the ByteString traversals, catch, the
-- FFI callback wrapper) exists ONCE and calls it with its GENERIC arity — the
-- arity intrinsics.rs RUNTIME_CALLBACKS records for that parameter. When a
-- result type variable is instantiated to a function — `map (\n -> \x -> x +
-- n) ns` builds a list of adders, `modifyIORef r (\h x -> h x * 2)` updates a
-- stored function — the argument's real arity exceeds that view, so the
-- compiler wraps it: absorb the runtime's call, then return a closure over
-- the remaining arguments (the same shape a compiled partial application
-- has). __mll_curry_n is the general form for an FFI callback of any arity.
local function __mll_curry1(f)
    return function(x)
        return function(...) return __force(f)(x, ...) end
    end
end
local function __mll_curry2(f)
    return function(x, y)
        return function(...) return __force(f)(x, y, ...) end
    end
end
local function __mll_curry_n(f, n)
    return function(...)
        local pre = {...}
        return function(...)
            local args = {}
            for i = 1, n do args[i] = pre[i] end
            local m = select('#', ...)
            for i = 1, m do args[n + i] = (select(i, ...)) end
            return __force(f)(__unpack(args, 1, n + m))
        end
    end
end
-- Hash helper
local function __mll_hashstr(s) s = __force(s); local h = 5381 for i = 1, #s do h = ((h * 33) + string.byte(s, i)) % 2147483647 end return h end

-- HashMap runtime: persistent maps over ONE mutable Lua table per version
-- family (Conchon–Filliâtre persistent hash tables — diff + reroot). A map
-- value is a HANDLE: a root `{t = store, n = size}` owning the live table,
-- or a diff `{k = key, v = old-value-or-nil, p = next-handle-toward-root}`
-- recording how its version differs from the neighbor one step closer to
-- the root (v == nil means the key is ABSENT in this version — safe
-- because stored values are never Lua nil, the __mll_hm_nilv sentinel
-- boxes those). A write mutates the live table and flips the old handle
-- into a diff, so linear derivation (each new map built from the last, the
-- shape folds produce) costs O(1) per write and reads on the newest
-- version stay one raw index. Reading an OLD version reroots first:
-- replays the diff chain onto the store, reversing each link, so that
-- version becomes the root. Purely internal — observationally the maps
-- are as persistent as before; single-threaded Lua makes the mutation
-- safe, and no runtime function yields mid-operation.
-- THE ORDERING RULE every operation on a version obeys: everything that
-- can run user code — forcing the map handle, forcing the key and the
-- value, ENCODING the key (a structural encoder forces every component
-- of the key) — happens BEFORE the operation fetches that version's
-- store (`m.t`, or __mll_hm_reroot), and nothing that can force a thunk
-- stands between the store fetch and the index. A forced thunk may read
-- another version of the same family, and that read reroots: the store
-- table a `local t = m.t` already holds is mutated into the other
-- version's contents, so an index taken after it answers for the wrong
-- version (H1: `hmLookup (2, hmSize m2) m1` found m2's entry because
-- the key was encoded after `m.t` was fetched). The enumerators that
-- force values while walking take the snapshot discipline below for the
-- same reason.
-- (__mll_hm_mt / __mll_hme_mt are declared at the top of the runtime:
-- __mll_to_lua and the FFI marshallers close over them.)
-- The shared canonical empty map (hmEmpty is one value program-wide). It
-- is FROZEN: writes copy instead of mutating/flipping it, so no diff
-- chain ever runs through it and every build from empty gets its own
-- store. Both key flavors share it (no metatable: the scalar-flavor
-- observers handle an empty store, and the flavored inserts create the
-- properly tagged root themselves).
local hashmap_empty = setmetatable({t = {}, n = 0, frozen = true}, __mll_hm_mt)
-- Make `m` the root of its version family and return the live store.
-- The one non-local-cost path: replaying a chain longer than the map
-- itself would cost more than a copy, so past that cap the version is
-- MATERIALIZED (fresh store cloned from the root, diffs applied onto it,
-- old root untouched) — two roots then coexist and ping-ponging reads
-- between far-apart versions degrade to the old copy-per-op behavior
-- instead of thrashing the chain.
__mll_hm_reroot = function(m)
    if m.t ~= nil then return m.t end
    local path, cur = {}, m
    while cur.t == nil do path[#path + 1] = cur; cur = cur.p end
    local t, n = cur.t, cur.n
    local cap = n < 16 and 16 or n
    if #path > cap then
        local c = {}
        for a, b in pairs(t) do c[a] = b end
        for i = #path, 1, -1 do
            local d = path[i]
            local old = c[d.k]
            if d.v == nil then
                if old ~= nil then n = n - 1 end
            elseif old == nil then
                n = n + 1
            end
            c[d.k] = d.v
        end
        m.k, m.v, m.p = nil, nil, nil
        m.t, m.n = c, n
        return c
    end
    local holder = cur
    for i = #path, 1, -1 do
        local d = path[i]
        local key = d.k
        local old = t[key]
        if d.v == nil then
            if old ~= nil then n = n - 1 end
        elseif old == nil then
            n = n + 1
        end
        t[key] = d.v
        holder.t, holder.n = nil, nil
        holder.k, holder.v, holder.p = key, old, d
        holder = d
    end
    m.k, m.v, m.p = nil, nil, nil
    m.t, m.n = t, n
    return t
end
-- Copy a live store's entries into parallel key/value arrays. Every
-- enumerator that FORCES values while walking a store must walk this
-- snapshot instead: forcing a value can read another version of the same
-- family, and that read reroots — mutates — the store `pairs` would be
-- iterating (a silent wrong-answer, not a Lua error, since the keys
-- being changed are ones the walk has not reached yet).
__mll_hm_snapshot = function(t)
    local ks, vs, n = {}, {}, 0
    for k, v in pairs(t) do n = n + 1; ks[n] = k; vs[n] = v end
    return ks, vs
end
-- The scalar keyed ops reject a table key loudly: a structural key that
-- slipped past the threading rewrite (a purged specialization, an erased
-- key type nothing ever gave evidence for) would otherwise key by Lua
-- table IDENTITY, a silent wrong-answer.
local function __mll_hm_scalar_key(k)
    k = __force(k)
    if type(k) == "table" then
        error("internal: a structural HashMap key reached the scalar hashmap path", 2)
    end
    return k
end
-- The write/read bodies below inline their entry forces: a key is a
-- table only when it is a thunk (or a structural key that slipped the
-- threading rewrite — scalar_key's loud error), so the type gate skips
-- the scalar_key call for the common already-scalar key, and the
-- metatable gates skip the __force call for already-WHNF values (the
-- gate is __force's own first test, so semantics are identical — every
-- argument is still forced before use, which is what the strictness
-- rows promise). Writes also take the newest-version store directly
-- (m.t) instead of paying a reroot call that would return it anyway —
-- the shape hashmap_lookup always had.
local function hashmap_insert(k, v, m)
    if type(k) == "table" then k = __mll_hm_scalar_key(k) end
    if k ~= k then k = __mll_hm_nan_box(k) end
    if getmetatable(v) == __thunk_mt then v = __force(v) end
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    if v == nil then v = __mll_hm_nilv end
    if m.frozen then
        local t = {}
        local n = m.n
        for a, b in pairs(m.t) do t[a] = b end
        if t[k] == nil then n = n + 1 end
        t[k] = v
        return setmetatable({t = t, n = n}, __mll_hm_mt)
    end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local old = t[k]
    t[k] = v
    local h = setmetatable({t = t, n = old == nil and m.n + 1 or m.n}, __mll_hm_mt)
    m.t, m.n = nil, nil
    m.k, m.v, m.p = k, old, h
    return h
end
local function hashmap_lookup(k, m)
    if type(k) == "table" then k = __mll_hm_scalar_key(k) end
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local v = t[k]
    if v == nil then return nil elseif rawequal(v, __mll_hm_nilv) then return Just(nil) else return Just(v) end
end
-- The fused `case hmLookup k m of Just v -> …; Nothing -> …` scrutinee
-- (see try_fused_hm_lookup_case in codegen): the raw slot, sentinel
-- preserved — nil means ABSENT (the Nothing branch); the emitted Just
-- branch unwraps the sentinel to the stored nil. Skips the Just cell
-- hashmap_lookup allocates per hit for a result the case tears apart
-- on the next line.
local function __mll_hm_slot(k, m)
    if type(k) == "table" then k = __mll_hm_scalar_key(k) end
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    return t[k]
end
local function hashmap_delete(k, m)
    if type(k) == "table" then k = __mll_hm_scalar_key(k) end
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local old = t[k]
    if old == nil then return m end
    if m.frozen then
        local c = {}
        for a, b in pairs(t) do c[a] = b end
        c[k] = nil
        return setmetatable({t = c, n = m.n - 1}, __mll_hm_mt)
    end
    t[k] = nil
    local h = setmetatable({t = t, n = m.n - 1}, __mll_hm_mt)
    m.t, m.n = nil, nil
    m.k, m.v, m.p = k, old, h
    return h
end
local function hashmap_size(m)
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    if m.t == nil then __mll_hm_reroot(m) end
    return m.n
end
-- Key sort comparator: Bool is a legal key type but Lua cannot `<`
-- booleans; order false < true. (Keys within one map share one type.) A
-- NaN box sorts after every ordinary key, boxes among themselves by
-- insertion sequence — a total order, which the raw NaN would not give.
local function __mll_hm_lt(a, b)
    if type(a) == "table" then
        return type(b) == "table" and a[2] < b[2]
    end
    if type(b) == "table" then return true end
    if type(a) == "boolean" then
        return (a and 1 or 0) < (b and 1 or 0)
    end
    return a < b
end
-- The enumerators hand a NaN box back as the NaN it boxes.
local function hashmap_keys(m) m = __force(m); local t = __mll_hm_reroot(m) local r = nil local ks = {} for k in pairs(t) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do local k = ks[i] if type(k) == "table" then k = k[1] end r = __mll_cons(k, r) end return r end
local function hashmap_values(m) m = __force(m); local t = __mll_hm_reroot(m) local r = nil local ks = {} for k in pairs(t) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do local v = t[ks[i]] if rawequal(v, __mll_hm_nilv) then v = nil end r = __mll_cons(v, r) end return r end
local function hashmap_member(k, m) if type(k) == "table" then k = __mll_hm_scalar_key(k) end if getmetatable(m) == __thunk_mt then m = __force(m) end local t = m.t if t == nil then t = __mll_hm_reroot(m) end return t[k] ~= nil end
-- show forces every value; a forced value may read another version of
-- this family and reroot the store, so the entries are snapshotted first
-- (never `pairs` over the live store across a force).
local function show_HashMap(m)
    m = __force(m)
    local ks, vs = __mll_hm_snapshot(__mll_hm_reroot(m))
    local parts = {}
    if getmetatable(m) == __mll_hme_mt then
        for i = 1, #vs do local e = vs[i]; parts[#parts+1] = show(e[1]) .. " -> " .. show(e[2]) end
    else
        -- A Number-flavored handle (`nk`) knows its key type: the keys take
        -- the type-directed Double show (a doubles-only host cannot tell
        -- 2 from 2.0 in the erased `show`); the values stay type-erased.
        for i = 1, #ks do local k, v = ks[i], vs[i]; if rawequal(v, __mll_hm_nilv) then v = nil end; if type(k) == "table" then k = k[1] end; parts[#parts+1] = (m.nk and show_Number(k * 1.0) or show(k)) .. " -> " .. show(v) end
    end
    table.sort(parts)
    return "{" .. table.concat(parts, ", ") .. "}"
end
local function showsPrec_HashMap(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_HashMap(x), s) end
local function hashmap_fromList(xs) xs = __force(xs); local t = {} local n = 0 local cur = xs while cur ~= nil do local pair = __force(__mll_head(cur)) local v = __force(pair[2]) if v == nil then v = __mll_hm_nilv end local k = __mll_hm_scalar_key(pair[1]) if k ~= k then k = __mll_hm_nan_box(k) end if t[k] == nil then n = n + 1 end t[k] = v cur = __mll_tail(cur) end return setmetatable({t = t, n = n}, __mll_hm_mt) end

-- Structural-key HashMaps (A17). A scalar key indexes the store table
-- directly (the functions above); a STRUCTURAL key (tuple, list, Maybe of
-- hashables) is a Lua table with identity semantics, so the encoded
-- variants below key the store on an injective string ENCODING of the key
-- and hold {key, value} entries. The handle metatable marks the flavor
-- (show, the FFI marshaller and the dictionary-routed family branch on
-- it). The encoder and the key COMPARE (A16, the structural Ord) are the
-- key TYPE's Hashable evidence: a specialized body threads the generated
-- ones, a dictionary-form body reads them off its Hashable dictionary
-- (__mll_hmd_* below), so every body encodes one key type one way and
-- maps built by either kind of body are read by the other. The writes
-- stamp the compare on the handle (`cmp`): a map carries its own key
-- order, exactly as GHC's Data.Map tree does, so hmKeys and friends need
-- no evidence at all and come back in true Ord order from any body.
local function __mll_key_scalar(x)
    x = __force(x)
    local t = type(x)
    if t == "string" then return "s" .. #x .. ":" .. x end
    if t == "boolean" then return x and "b1" or "b0" end
    -- A NaN encodes to a fresh token per call (see __mll_hm_nan_seq at the
    -- top): the key lands and nothing ever matches it. "u" is no other
    -- encoding's prefix (s/b/i/n/N/J/[/( are taken).
    if x ~= x then __mll_hm_nan_seq = __mll_hm_nan_seq + 1; return "u" .. __mll_hm_nan_seq end
    -- The encoding is by numeric VALUE, as `==` (and GHC) equate keys: an
    -- integral float encodes as the equal integer when one exists, so the
    -- integer-subtype 2 and a leaked 2.0 — and -0.0 and 0.0 — are ONE key
    -- (the scalar store gets this for free from Lua's own key
    -- normalization; this encoder used to split them).
    if math.type ~= nil then
        if math.type(x) == "integer" then return "i" .. x end
        local i = math.tointeger(x)
        if i ~= nil then return "i" .. i end
        return "n" .. string.format("%.17g", x)
    end
    -- One float type (LuaJIT): only -0.0 needs the value rule by hand.
    if x == 0 then return "n0" end
    return "n" .. string.format("%.17g", x)
end
-- The Number key type's encoder: the scalar encoder's value-based encoding
-- under an identity of its own, which is what the dictionary-routed family
-- dispatches on — a Number-keyed map takes the scalar store with the
-- Number flavor (`__mll_hmn_*` below), not the plain scalar functions.
local function __mll_key_number(x) return __mll_key_scalar(x) end
local function __mll_key_list(enc, l) l = __force(l); local parts = {} local cur = l while cur ~= nil do parts[#parts+1] = enc(__force(cur[1])) cur = __mll_tail(cur) end return "[" .. table.concat(parts, ",") .. "]" end
local function __mll_key_maybe(enc, x) x = __force(x); if x == nil then return "N" end return "J(" .. enc(x[1]) .. ")" end
-- The flavor tag lives on the HANDLE (roots and diffs both carry it:
-- flips and materializations mutate handles in place, so a handle keeps
-- its metatable for life), and so does the key compare (`cmp`): every
-- write stamps it on the handle it creates — the writer always holds the
-- key type's evidence, a reader may not — and a flip never clears it.
-- Entries {key, value} are never nil, so the diff convention (v == nil
-- means absent) holds here too.
-- The encoded twins inline their entry forces exactly like the scalar
-- write bodies above (a structural key stays a table, so its force is
-- gated on the thunk metatable, not on type).
local function __mll_hme_insert(enc, cmp, k, v, m)
    if getmetatable(k) == __thunk_mt then k = __force(k) end
    if getmetatable(v) == __thunk_mt then v = __force(v) end
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local seq = __mll_hm_nan_seq
    local ek = enc(k)
    -- An entry whose key holds a NaN records the token sequence (slot 3)
    -- so the enumerators can order it after the ordinary entries.
    local e = {k, v}
    if __mll_hm_nan_seq ~= seq then e[3] = __mll_hm_nan_seq end
    if m.frozen then
        local t = {}
        local n = m.n
        for a, b in pairs(m.t) do t[a] = b end
        if t[ek] == nil then n = n + 1 end
        t[ek] = e
        return setmetatable({t = t, n = n, cmp = cmp}, __mll_hme_mt)
    end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local old = t[ek]
    t[ek] = e
    local h = setmetatable({t = t, n = old == nil and m.n + 1 or m.n, cmp = cmp}, __mll_hme_mt)
    m.t, m.n = nil, nil
    m.k, m.v, m.p = ek, old, h
    return h
end
-- The reads encode the key BEFORE they touch the store (the ordering
-- rule in the header): `enc` forces the key's components, and a
-- component that reads another version of this family reroots the store
-- a `local t = m.t` taken earlier would still point at — the index then
-- answered for THAT version (`hmLookup (2, hmSize m2) m1` found m2's
-- entry). The writes above and the delete below already encode first.
local function __mll_hme_lookup(enc, k, m)
    if getmetatable(k) == __thunk_mt then k = __force(k) end
    local ek = enc(k)
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local e = t[ek]
    if e == nil then return nil else return Just(e[2]) end
end
local function __mll_hme_delete(enc, k, m)
    if getmetatable(k) == __thunk_mt then k = __force(k) end
    local ek = enc(k)
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    local old = t[ek]
    if old == nil then return m end
    if m.frozen then
        local c = {}
        for a, b in pairs(t) do c[a] = b end
        c[ek] = nil
        return setmetatable({t = c, n = m.n - 1, cmp = m.cmp}, __mll_hme_mt)
    end
    t[ek] = nil
    local h = setmetatable({t = t, n = m.n - 1, cmp = m.cmp}, __mll_hme_mt)
    m.t, m.n = nil, nil
    m.k, m.v, m.p = ek, old, h
    return h
end
local function __mll_hme_member(enc, k, m)
    if getmetatable(k) == __thunk_mt then k = __force(k) end
    local ek = enc(k)
    if getmetatable(m) == __thunk_mt then m = __force(m) end
    local t = m.t
    if t == nil then t = __mll_hm_reroot(m) end
    return t[ek] ~= nil
end
local function __mll_hme_fromList(enc, cmp, xs) xs = __force(xs); local t = {} local n = 0 local cur = xs while cur ~= nil do local p = __force(__mll_head(cur)) local k = __force(p[1]) local seq = __mll_hm_nan_seq local ek = enc(k) local e = {k, __force(p[2])} if __mll_hm_nan_seq ~= seq then e[3] = __mll_hm_nan_seq end if t[ek] == nil then n = n + 1 end t[ek] = e cur = __mll_tail(cur) end return setmetatable({t = t, n = n, cmp = cmp}, __mll_hme_mt) end
-- The enumerators sort by the compare the writes stamped on the handle
-- (the shared empty map has none and no entries to sort). NaN-bearing
-- entries (slot 3 set) are kept out of the compare sort — a NaN compares
-- GT both ways, which is no order at all — and appended after the
-- ordinary entries in insertion sequence, like the scalar boxes.
local function __mll_hme_sorted(m) m = __force(m); local t = __mll_hm_reroot(m) local cmp = m.cmp local es, ns = {}, {} for _, e in pairs(t) do if e[3] == nil then es[#es+1] = e else ns[#ns+1] = e end end table.sort(es, function(x, y) return cmp(x[1], y[1]) == 1 end) if #ns > 0 then table.sort(ns, function(x, y) return x[3] < y[3] end) for i = 1, #ns do es[#es+1] = ns[i] end end return es end
local function __mll_hme_keys(m) local es = __mll_hme_sorted(m) local r = nil for i = #es, 1, -1 do r = __mll_cons(es[i][1], r) end return r end
local function __mll_hme_values(m) local es = __mll_hme_sorted(m) local r = nil for i = #es, 1, -1 do r = __mll_cons(es[i][2], r) end return r end
local function __mll_hme_toList(m) local es = __mll_hme_sorted(m) local r = nil for i = #es, 1, -1 do r = __mll_cons({es[i][1], es[i][2]}, r) end return r end
local function hashmap_toList(m) m = __force(m); local t = __mll_hm_reroot(m) local r = nil local ks = {} for k in pairs(t) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do local k = ks[i] local v = t[k] if rawequal(v, __mll_hm_nilv) then v = nil end if type(k) == "table" then k = k[1] end r = __mll_cons({k, v}, r) end return r end
-- Number-keyed maps: the scalar store (a Number key is a Lua number, and
-- a read finds it by value) under the Number FLAVOR. Lua 5.3+ normalizes a
-- float table key with an integral value to the integer subtype, so a key
-- stored as 3.0 enumerates as the integer 3 — and a Number is an IEEE
-- double on every host (arithmetic on the leaked integer wrapped at 2^63
-- and `negate 0` lost its sign). The writes stamp the handle `nk` (reroot
-- moves stores between handles and never creates one, so a stamp lives
-- for the handle's life; the shared empty map has no keys to convert),
-- and the enumerators of a stamped handle — these, the dictionary-routed
-- __mll_hmd_keys/toList and show_HashMap — hand every key back as a
-- double (`* 1.0`; a NaN box unboxes as before). The store holds the zero
-- key as +0 on every host, so -0.0 and 0.0 are one key that reads back as
-- 0.0 — the documented deviation from Data.Map, which keeps the spelling
-- inserted last. A specialized body at key type Number calls these
-- directly (mono's hm_op_use); lookup, member and the fused slot read stay
-- the plain scalar functions.
local function __mll_hmn_insert(k, v, m) local h = hashmap_insert(k, v, m) h.nk = true return h end
local function __mll_hmn_delete(k, m) local h = hashmap_delete(k, m) if h ~= m then h.nk = true end return h end
local function __mll_hmn_fromList(xs) local h = hashmap_fromList(xs) h.nk = true return h end
local function __mll_hmn_keys(m) m = __force(m); local t = __mll_hm_reroot(m) local r = nil local ks = {} for k in pairs(t) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do local k = ks[i] if type(k) == "table" then k = k[1] else k = k * 1.0 end r = __mll_cons(k, r) end return r end
local function __mll_hmn_toList(m) m = __force(m); local t = __mll_hm_reroot(m) local r = nil local ks = {} for k in pairs(t) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do local k = ks[i] local v = t[k] if rawequal(v, __mll_hm_nilv) then v = nil end if type(k) == "table" then k = k[1] else k = k * 1.0 end r = __mll_cons({k, v}, r) end return r end
-- The dictionary-routed family: a Hashable-constrained body compiled with
-- dictionary passing (the Data.Map/Data.Set wrappers past the
-- specialization limit, a function over an existential key) serves every
-- key type through one copy, so it reads the key type's encoder and
-- compare off its Hashable dictionary and the flavor is decided here, at
-- run time, by ONE rule: a key whose encoder is the scalar encoder is a
-- scalar key and takes the scalar store, byte-for-byte the path a
-- specialized body takes (so maps built here interoperate with
-- statically-specialized accessors); the Number encoder takes the scalar
-- store under the Number flavor (above), again what a specialized body at
-- Number does; any other encoder is a structural shape's and takes the
-- encoded store with that same typed encoder. The keyless enumerators
-- have no key type to ask and need none: the handle carries the flavor
-- and the compare.
local function __mll_hmd_insert(enc, cmp, k, v, m) if enc == __mll_key_scalar then return hashmap_insert(k, v, m) end if enc == __mll_key_number then return __mll_hmn_insert(k, v, m) end return __mll_hme_insert(enc, cmp, k, v, m) end
local function __mll_hmd_lookup(enc, k, m) if enc == __mll_key_scalar or enc == __mll_key_number then return hashmap_lookup(k, m) end return __mll_hme_lookup(enc, k, m) end
local function __mll_hmd_delete(enc, k, m) if enc == __mll_key_scalar then return hashmap_delete(k, m) end if enc == __mll_key_number then return __mll_hmn_delete(k, m) end return __mll_hme_delete(enc, k, m) end
local function __mll_hmd_member(enc, k, m) if enc == __mll_key_scalar or enc == __mll_key_number then return hashmap_member(k, m) end return __mll_hme_member(enc, k, m) end
local function __mll_hmd_fromList(enc, cmp, xs) if enc == __mll_key_scalar then return hashmap_fromList(xs) end if enc == __mll_key_number then return __mll_hmn_fromList(xs) end return __mll_hme_fromList(enc, cmp, xs) end
local function __mll_hmd_keys(m) m = __force(m) if getmetatable(m) == __mll_hme_mt then return __mll_hme_keys(m) end if m.nk then return __mll_hmn_keys(m) end return hashmap_keys(m) end
local function __mll_hmd_values(m) m = __force(m) if getmetatable(m) == __mll_hme_mt then return __mll_hme_values(m) end return hashmap_values(m) end
local function __mll_hmd_toList(m) m = __force(m) if getmetatable(m) == __mll_hme_mt then return __mll_hme_toList(m) end if m.nk then return __mll_hmn_toList(m) end return hashmap_toList(m) end



-- Specialized list show: uses a typed element show function
local function __mll_list_eq(elem_eq, a, b)
    a = __force(a); b = __force(b)
    while true do
        if a == nil and b == nil then return true end
        if a == nil or b == nil then return false end
        if not elem_eq(__force(a[1]), __force(b[1])) then return false end
        a = __mll_tail(a); b = __mll_tail(b)
    end
end
local function __mll_maybe_eq(elem_eq, a, b)
    a = __force(a); b = __force(b)
    if a == nil and b == nil then return true end
    if a == nil or b == nil then return false end
    -- Both are Just wrappers; compare the unwrapped payloads.
    return elem_eq(a[1], b[1])
end
local function __mll_cmp(a, b)
    -- Type-erased Ordering fallback (the compare analog of __mll_eq): the
    -- last resort at a genuinely polymorphic position. Numbers and strings
    -- order natively; booleans get False < True by hand (Lua cannot `<`
    -- them); nil (an erased Nothing) sorts first, GHC's Maybe order; and
    -- TABLES (erased tuples, cons cells, Just wrappers) compare
    -- lexicographically over their numeric slots, nil-aware — shorter
    -- list first, Nothing before Just, exactly the structural compares.
    -- The result is the Ordering constructor index: LT=1, EQ=2, GT=3.
    a = __force(a); b = __force(b)
    if a == b then return 2 end
    if a == nil then return 1 end
    if b == nil then return 3 end
    if type(a) == "table" and type(b) == "table" then
        local na, nb = 0, 0
        for j in pairs(a) do if type(j) == "number" and j > na then na = j end end
        for j in pairs(b) do if type(j) == "number" and j > nb then nb = j end end
        local n = na > nb and na or nb
        for i = 1, n do
            local c = __mll_cmp(a[i], b[i])
            if c ~= 2 then return c end
        end
        return 2
    end
    if a == false then return 1 end
    if b == false then return 3 end
    if a < b then return 1 end
    return 3
end
local function __mll_list_cmp(elem_cmp, a, b)
    -- Lexicographic list compare threading the element comparator (the
    -- compare analog of __mll_list_eq, walking the same way). Ordering is
    -- its constructor index (LT=1, EQ=2, GT=3), as ord_compare__Int & co.
    a = __force(a); b = __force(b)
    while true do
        if a == nil and b == nil then return 2 end
        if a == nil then return 1 end
        if b == nil then return 3 end
        local c = elem_cmp(__force(a[1]), __force(b[1]))
        if c ~= 2 then return c end
        a = __mll_tail(a); b = __mll_tail(b)
    end
end
local function __mll_maybe_cmp(elem_cmp, a, b)
    -- Nothing < Just _, exactly GHC's derived Ord for Maybe.
    a = __force(a); b = __force(b)
    if a == nil and b == nil then return 2 end
    if a == nil then return 1 end
    if b == nil then return 3 end
    return elem_cmp(a[1], b[1])
end

local function __mll_show_maybe(elem_shows_prec, x)
    -- Type-directed Maybe show. `Nothing` is nil; `Just p` is a tagged wrapper
    -- whose payload is field [1] (itself possibly nil, e.g. `Just Nothing`), so
    -- nesting renders faithfully: `Just Nothing`, `Just (Just 5)`, etc. The
    -- payload is shown at precedence 11 through ITS type's showsPrec (GHC's
    -- derived `Show (Maybe a)`): a hand-written showsPrec decides its own
    -- parentheses, a show-only instance gets none.
    x = __force(x)
    if x == nil then return "Nothing" end
    return "Just " .. elem_shows_prec(11, x[1], "")
end
local function __mll_show_list(elem_show, xs)
    xs = __force(xs)
    if xs == nil then return "[]" end
    -- A non-empty mata-ll list is exactly a cons cell (strict or lazy), tagged
    -- with __cons_mt — same guard the generic `show` uses. A raw Lua table here
    -- means a value crossed the Lua FFI boundary without being decoded; reading
    -- its nil head would silently render "[Nothing]", so fail loudly instead.
    if type(xs) ~= "table" or getmetatable(xs) ~= __cons_mt then
        error("show: expected a list but got a raw " .. type(xs) ..
              " value (an undecoded Lua FFI result?)")
    end
    local parts = {}
    local cur = xs
    while cur ~= nil do
        parts[#parts + 1] = elem_show(__force(__mll_head(cur)))
        cur = __mll_tail(cur)
    end
    return "[" .. table.concat(parts, ",") .. "]"
end

-- Lua error convention wrapper: converts (val, err) to Either String a.
-- Success: Right (decoded result), Failure: Left errmsg. `...` are the host
-- call's return values. A first value of nil is the failure, the second
-- value its message; the error value is tostring'd: hosts may signal failure
-- with a non-string error object, while Left is declared String. Any other
-- first value is success, and the SUCCESS payload crosses the FFI boundary
-- exactly like any other FFI result (__mll_ffi_ret with the payload type's
-- decision `n`, `desc`, `root`): a declared tuple is the call's first n
-- values, a structured value (a list, a record) is decoded.
local function __mll_try(n, desc, root, ...)
    local val, err = ...
    if val == nil then
        if err == nil then return {1, "unknown error"} end
        return {1, tostring(err)}
    end
    return {2, __mll_ffi_ret(n, desc, root, ...)}
end
-- LuaCatch/LuaIOCatch: run a Lua function under pcall, returning Either String a.
-- Success: Right (decoded result), Failure: Left errmsg. The success payload
-- is everything the function returned, received exactly like any other FFI
-- result (__mll_ffi_ret with the payload type's decision `n`, `desc`,
-- `root`): a declared tuple is the function's first n return values.
-- tostring() because error() can raise non-string values, while Left is
-- declared String. Arguments are already forced by the caller (outside
-- pcall) so only the Lua function's own error() is captured — a shape
-- mismatch in the *successful* result is a host-integration bug and still
-- raises, it does not become a Left.
local function __mll_pcall_ret(n, desc, root, ok, ...)
    if not ok then return {1, tostring((...))} end
    return {2, __mll_ffi_ret(n, desc, root, ...)}
end
local function __mll_pcall(n, desc, root, f, ...)
    return __mll_pcall_ret(n, desc, root, pcall(f, ...))
end
-- Exception handling: `try` and `catch` are ordinary first-class functions
-- over ACTION VALUES. Their action argument arrives under the plain lazy
-- argument convention, so it is anything an action-typed expression
-- evaluates to: an action closure, a pure box, or a thunk that computes
-- either. The returned action runs it through a runner INSIDE the pcall,
-- so computing the action value (forcing that thunk) and performing it
-- both happen under the handler, as GHC's catch# applies the action inside
-- its frame: `catch (error "x") h` and `try (if b then a else ⊥)` are
-- caught. Neither runner forces the action's result, so a ⊥ the action
-- returns without forcing escapes the handler unraised, as in GHC. The
-- handler is applied and its action run outside the pcall: an exception
-- it raises propagates.
--
-- Box protocol (see the runners): try_ CONSUMES the action's result to
-- wrap it in Right, so it runs the action with the consuming `__mll_run`.
-- catch_'s result is its action's or its handler's result unchanged, so it
-- runs both with the forwarding `__mll_run_tail` and returns what that
-- yields — at most the one pending box, which the consumer of catch_'s own
-- action strips, exactly as if the inner action had been run in its place.
local function try_(action)
    return function()
        local ok, result = pcall(__mll_run, action)
        if ok then return {2, result} else return {1, tostring(result)} end
    end
end
local function catch_(action, handler)
    return function()
        local ok, result = pcall(__mll_run_tail, action)
        if ok then return result end
        return __mll_run_tail(__force(handler)(tostring(result)))
    end
end

-- Iterator-to-lazy-list: calls a Lua iterator factory and builds a lazy MLL list.
-- Each iterator step is a host call whose results are ONE element, received
-- exactly like any other FFI result (__mll_ffi_ret with the ELEMENT type's
-- decision `n`, `desc`, `root`): an element declared as a tuple is the
-- step's first n values (a pairs-style `k, v` step is a `(k, v)`), any
-- other element is the step's first value, and the value is decoded by its
-- descriptor (a list element -> cons list, a Maybe -> Just/Nothing, a
-- mismatched shape -> a clear localized error). A step whose first value is
-- nil ends the list, whatever follows it (`nil, 5` is the end).
local function __mll_iter(factory, n, desc, root, ...)
    local iter = factory(...)
    local go
    -- `step` receives one iterator call's results as varargs, so a
    -- single-value element (the common case) allocates nothing per element;
    -- only a declared tuple element is packed into a table.
    local function step(...)
        if (...) == nil then return nil end
        return __mll_lazy_cons(__mll_ffi_ret(n, desc, root, ...), go)
    end
    go = function() return step(iter()) end
    return go()
end

local getArgs = function()
    local result = nil
    if arg then
        for i = #arg, 1, -1 do result = __mll_cons(arg[i], result) end
    end
    return result
end
-- Prelude exit :: ExitValue -> IO (). ExitValue is a mixed ADT, so values
-- are tag tables: Normal = {1}, Err code = {2, code} (fields may be thunks).
local function exit_(v)
    return function()
        v = __force(v)
        if v[1] == 1 then os.exit(0) else os.exit(__force(v[2])) end
    end
end

-- Bitwise operations (Lua 5.3+ native, LuaJIT bit.*, or bit32)
local __mll_bxor, __mll_band, __mll_bor, __mll_bnot, __mll_shl, __mll_shr
if (loadstring or load)('return 0 ~ 0') then
    __mll_bxor = (loadstring or load)('local F=... return function(a,b) return F(a) ~ F(b) end')(__force)
    __mll_band = (loadstring or load)('local F=... return function(a,b) return F(a) & F(b) end')(__force)
    __mll_bor  = (loadstring or load)('local F=... return function(a,b) return F(a) | F(b) end')(__force)
    __mll_bnot = (loadstring or load)('local F=... return function(a) return ~F(a) end')(__force)
    __mll_shl  = (loadstring or load)('local F=... return function(a,b) return F(a) << F(b) end')(__force)
    __mll_shr  = (loadstring or load)('local F=... return function(a,b) return F(a) >> F(b) end')(__force)
else
    local __ok, __mll_bit = pcall(function() return (type(jit) == 'table' and require('bit')) or bit32 or require('bit') end)
    if not __ok then __mll_bit = nil end
    if __mll_bit then
    function __mll_bxor(a, b) return __mll_bit.bxor(__force(a), __force(b)) end
    function __mll_band(a, b) return __mll_bit.band(__force(a), __force(b)) end
    function __mll_bor(a, b) return __mll_bit.bor(__force(a), __force(b)) end
    function __mll_bnot(a) return __mll_bit.bnot(__force(a)) end
    function __mll_shl(a, b) return __mll_bit.lshift(__force(a), __force(b)) end
    function __mll_shr(a, b) return __mll_bit.rshift(__force(a), __force(b)) end
    end
end

-- Int division and modulo (Haskell `div`/`mod`: FLOOR semantics — the
-- quotient rounds toward negative infinity, the remainder takes the sign of
-- the divisor; the constant folder in fold.rs mirrors exactly this). A zero
-- divisor raises on every host: mathematically there is no result, and the
-- old float path (`math.floor(a/0)`) returned `inf` — a float silently
-- flowing on as if it were an Int. On Lua 5.3+ the native integer floor
-- division `//` is exact over the full 64-bit range; on LuaJIT / Lua 5.1-5.2
-- every number is an IEEE-754 double, so math.floor(a/b) is the best those
-- hosts can do and quotients are exact only while the operands fit in 2^53
-- (a documented host limitation — see doc/articles/CAVEATS.md). The `//`
-- form is compiled through load so this file still parses on 5.1 hosts,
-- the same technique as the bitwise ops above.
local __mll_div, __mll_mod
do
    local mk = (loadstring or load)('return function(a, b) return a // b end')
    local floordiv = mk and mk() or function(a, b) return math.floor(a / b) end
    __mll_div = function(a, b)
        if b == 0 then error("divide by zero: `div` has no result when the divisor is 0") end
        return floordiv(a, b)
    end
    __mll_mod = function(a, b)
        if b == 0 then error("divide by zero: `mod` has no result when the divisor is 0") end
        return a % b
    end
end

-- First-class `div` / `mod`. The inline `a `div` b` / prefix-inline path passes
-- operands already forced (gen_forced), so __mll_div/__mll_mod above are the
-- strict cores and never re-force — keeping the arithmetic hot path (e.g. the
-- tracker mixer) free of redundant forces. But a first-class, partially applied,
-- or higher-order use — `div 7 2`, `map (div 10) xs`, `foldr div z xs` — reaches
-- the callee as a plain value and may be handed unforced (thunk) arguments by
-- its caller (a lazy list element, a passed-through parameter). These wrappers
-- force both arguments to WHNF before the strict core, so `div`/`mod` are total
-- functions in every application form, exactly as the backtick form is. `__force`
-- is idempotent, so an already-forced argument costs only the metatable probe.
local function __mll_div_fn(a, b) return __mll_div(__force(a), __force(b)) end
local function __mll_mod_fn(a, b) return __mll_mod(__force(a), __force(b)) end

-- Integral `quot`/`rem`: truncate toward zero (remainder takes the DIVIDEND's
-- sign), unlike `div`/`mod` which floor (remainder takes the divisor's sign).
-- Computed exactly from the floor division `__mll_div` and native `%`: floor and
-- truncation agree except when the operands' signs differ and the division is
-- inexact, where truncation is one greater. `rem a b = a - b*(quot a b)`.
local __mll_quot, __mll_rem
do
    __mll_quot = function(a, b)
        if b == 0 then error("divide by zero: `quot` has no result when the divisor is 0") end
        local q = __mll_div(a, b)
        if a % b ~= 0 and (a < 0) ~= (b < 0) then q = q + 1 end
        return q
    end
    __mll_rem = function(a, b)
        if b == 0 then error("divide by zero: `rem` has no result when the divisor is 0") end
        return a - b * __mll_quot(a, b)
    end
end
local function __mll_quot_fn(a, b) return __mll_quot(__force(a), __force(b)) end
local function __mll_rem_fn(a, b) return __mll_rem(__force(a), __force(b)) end

-- Integral quotRem/divMod: both quotient and remainder as a pair (a 2-tuple is
-- a Lua `{q, r}` table). Defined here, after __mll_quot/__mll_rem/__mll_div/
-- __mll_mod, since they call those strict cores.
local function quotRem_Int(a, b) a = __force(a); b = __force(b); return { __mll_quot(a, b), __mll_rem(a, b) } end
local function divMod_Int(a, b) a = __force(a); b = __force(b); return { __mll_div(a, b), __mll_mod(a, b) } end

-- Rounding a Number to an Int: the Prelude's `floor` and `ceiling`
-- (`truncate` and `round` are defined over them). GHC's `floor :: Double
-- -> Int` is `fromInteger . floor`: the exact integer below x, narrowed to
-- Int the way every Integer -> Int conversion is — the value modulo 2^64
-- — and 0 for NaN and the infinities (decodeFloat gives those a mantissa
-- times 2^972, a multiple of 2^64). Lua's math.floor/math.ceil return
-- that integer only while it fits a 64-bit integer; for NaN, ±inf and |x|
-- >= 2^63 they return the FLOAT — not a value of type Int at all: it
-- showed as `Infinity` or as a 301-digit numeral, and `fromIntegral` of it
-- sent the Integer limb loop around forever. The fast path (the native
-- call plus two comparisons; a fitting result lies in [-2^63, 2^63), the
-- test Lua itself applies before it returns an integer) is what hot
-- numeric code pays; everything else takes the wrap below.
--
-- The wrap is exact without `//`, bit operations or a bignum: a double of
-- magnitude >= 2^63 is an integer multiple of its ulp, which is >= 2^11
-- there, so math.fmod(r, 2^64) — C's fmod, exact for every pair of
-- doubles — is a multiple of 2^11 of magnitude below 2^64: at most 53
-- significant bits, hence representable, with r's sign. Moving it into
-- [-2^63, 2^63) adds or subtracts 2^64 once, again exact (a multiple of
-- 2^11 of magnitude <= 2^63). That is the representative of r modulo 2^64
-- in Int's range — GHC's integerToInt, the low 64 bits read as two's
-- complement. On Lua 5.3+ math.tointeger makes it the integer subtype
-- (the float -2^63 converts, so every value in the interval does); on a
-- doubles-only host the double IS the Int, and since every wrapped value
-- is a multiple of 2^11 it is GHC's exact Int there too, not a rounding
-- of it — only arithmetic on it past 2^53 is the host's documented limit.
local __mll_floor, __mll_ceil
do
    local mfloor, mceil, fmod, tointeger = math.floor, math.ceil, math.fmod, math.tointeger
    local huge = math.huge
    local TWO63, TWO64 = 9223372036854775808.0, 18446744073709551616.0
    local function wrap(r)
        if r ~= r or r == huge or r == -huge then return 0 end
        local m = fmod(r, TWO64)
        if m >= TWO63 then m = m - TWO64 elseif m < -TWO63 then m = m + TWO64 end
        if tointeger then return tointeger(m) end
        return m
    end
    __mll_floor = function(x)
        local r = mfloor(x)
        if r >= -TWO63 and r < TWO63 then return r end
        return wrap(r)
    end
    __mll_ceil = function(x)
        local r = mceil(x)
        if r >= -TWO63 and r < TWO63 then return r end
        return wrap(r)
    end
end

-- Number subtype probe (Lua 5.3+ native math.type, else a portable fallback).
-- LuaJIT and Lua 5.1/5.2 have no integer subtype: every number is an IEEE-754
-- double, so "float" is the correct answer for any number there. Non-numbers
-- yield nil, matching math.type's contract.
local __mll_math_type
if math.type then
    function __mll_math_type(x) return math.type(__force(x)) end
else
    function __mll_math_type(x)
        if type(__force(x)) == 'number' then return 'float' end
        return nil
    end
end

-- Array primitives (O(1) indexed access, built from MLL lists)
local function __mll_array_from_list(xs)
    xs = __force(xs)
    local arr = {}
    local cur = xs
    while cur ~= nil do
        arr[#arr + 1] = __force(__mll_head(cur))
        cur = __mll_tail(cur)
    end
    return arr
end
local function __mll_array_index(arr, i) return __force(arr)[__force(i) + 1] end
local function __mll_array_length(arr) return #__force(arr) end

-- ByteString runtime (backed by Lua strings)
-- All indices are 0-based in MLL, converted to 1-based for Lua internally.
local __mll_bs_empty = ""
local __mll_bs; do
    local F = __force
    local sb, sc, sr, ss = string.byte, string.char, string.rep, string.sub
    __mll_bs = {
        function(s) return #F(s) end,                                           -- [1] length
        function(s, i)                                                          -- [2] index
            -- Bounds-checked (declared total -> Int): out of range used to
            -- return NIL and crash far away as "arithmetic on nil".
            s=F(s); i=F(i)
            if i < 0 or i >= #s then error("bsIndex: index out of range") end
            return sb(s, i + 1)
        end,
        function(s, i, len) s=F(s); i=F(i); len=F(len); return ss(s, i+1, i+len) end, -- [3] sub
        function(b) return sc(F(b)) end,                                        -- [4] singleton
        function(a, b) return F(a) .. F(b) end,                                -- [5] concat
        function(s) return #F(s) == 0 end,                                      -- [6] null
        function(s) s=F(s); if #s == 0 then error("bsHead: empty ByteString") end return sb(s, 1) end, -- [7] head
        function(s) s=F(s); if #s == 0 then error("bsTail: empty ByteString") end return ss(s, 2) end, -- [8] tail
        function(b, s) return sc(F(b)) .. F(s) end,                             -- [9] cons
        function(s, b) return F(s) .. sc(F(b)) end,                             -- [10] snoc
        function(n, b) return sr(sc(F(b)), F(n)) end,                           -- [11] replicate
        function(xs)                                                             -- [12] pack
            xs = F(xs); local t = {}; local cur = xs
            while cur ~= nil do t[#t+1] = sc(F(__mll_head(cur))); cur = __mll_tail(cur) end
            return table.concat(t)
        end,
        function(s)                                                              -- [13] unpack
            s = F(s); local r = nil
            for i = #s, 1, -1 do r = __mll_cons(sb(s, i), r) end
            return r
        end,
        function(f, s)                                                           -- [14] map
            f=F(f); s=F(s); local t = {}
            for i = 1, #s do t[i] = sc(F(f(sb(s, i)))) end
            return table.concat(t)
        end,
        function(f, acc, s)                                                      -- [15] foldl
            -- Compiled functions are N-ary: the step is called with both
            -- arguments, and its result — nil included (a `Nothing`, a `()`,
            -- an empty list at a polymorphic accumulator type) — IS the new
            -- accumulator. (A "nil means curried, retry with one argument"
            -- fallback once lived here; it could never be right.)
            f=F(f); acc=F(acc); s=F(s)
            for i = 1, #s do acc = F(f(acc, sb(s, i))) end
            return acc
        end,
        function(a, b)                                                           -- [16] xor
            -- Truncates to the SHORTER operand, same as zipwith below —
            -- iterating over #a read past a shorter b (nil bytes into the
            -- bitwise op, a crash) (F9).
            a=F(a); b=F(b); local len=math.min(#a, #b); local t = {}
            for i = 1, len do t[i] = sc(__mll_bxor(sb(a, i), sb(b, i))) end
            return table.concat(t)
        end,
        function(f, a, b)                                                        -- [17] zipwith
            f=F(f); a=F(a); b=F(b); local len=math.min(#a, #b); local t = {}
            for i = 1, len do t[i] = sc(F(f(sb(a, i), sb(b, i)))) end
            return table.concat(t)
        end,
        function(s) return F(s) end,                                             -- [18] tostring
        function(s) return F(s) end,                                             -- [19] fromstring
        function(s, i)                                                           -- [20] getU16LE
            s=F(s); i=F(i)
            if i < 0 or i + 2 > #s then error("bsGetU16LE: index out of range") end
            i=i+1; local lo,hi=sb(s,i),sb(s,i+1); return lo+hi*256
        end,
        function(s, i)                                                           -- [21] getU32LE
            s=F(s); i=F(i)
            if i < 0 or i + 4 > #s then error("bsGetU32LE: index out of range") end
            i=i+1; local a,b,c,d=sb(s,i),sb(s,i+1),sb(s,i+2),sb(s,i+3); return a+b*256+c*65536+d*16777216
        end,
        function(s, i)                                                           -- [22] getI8 (signed)
            s=F(s); i=F(i)
            if i < 0 or i >= #s then error("bsGetI8: index out of range") end
            local v=sb(s,i+1); if v>=128 then return v-256 else return v end
        end,
        function(s, i)                                                           -- [23] getI16LE (signed)
            s=F(s); i=F(i)
            if i < 0 or i + 2 > #s then error("bsGetI16LE: index out of range") end
            i=i+1; local v=sb(s,i)+sb(s,i+1)*256; if v>=32768 then return v-65536 else return v end
        end,
        function(v)                                                              -- [24] putI16LE (signed int to 2-byte BS)
            v=F(v); if v<0 then v=v+65536 end; return sc(v%256, math.floor(v/256)%256)
        end,
        function(xs)                                                             -- [25] concatList
            xs = F(xs); local t = {}; local cur = xs
            while cur ~= nil do t[#t+1] = F(__mll_head(cur)); cur = __mll_tail(cur) end
            return table.concat(t)
        end,
    }
end
local function show_ByteString(s) s = __force(s); local t = {} for i = 1, #s do t[i] = string.format("%02x", string.byte(s, i)) end return "ByteString " .. table.concat(t) end
local function showsPrec_ByteString(d, x, s) d = __force(d); x = __force(x); s = __force(s); return __mll_shows_prec(d, show_ByteString(x), s) end
local function eq_ByteString(a, b) return __force(a) == __force(b) end
local function ne_ByteString(a, b) return __force(a) ~= __force(b) end

-- MutArray runtime (mutable integer arrays, backed by Lua tables)
-- Operations are effectful and run inside LuaIO s.
-- 0-based indexing externally, 1-based internally.
-- ST array primitives: these run inside runST which provides scoping,
-- so they perform directly (no action closure wrapping needed).
--
-- Laziness is GHC's boxed STArray: a slot holds the value AS STORED —
-- possibly a thunk — because `newSTArray n x`, `writeSTArray a i x` and
-- `newSTArrayFromList xs` are lazy in the element (`writeSTArray a i
-- undefined` never read is silent, as under GHC; the slots of a fresh
-- array share one initializer thunk, memoized on first force).
-- `readSTArray` forces the slot it returns (mata-ll's one deviation:
-- GHC hands the thunk back unforced), so a bound read is WHNF and its
-- uses stay force-free; `modifySTArray` calls f on the stored value and
-- stores f's result forced to WHNF (a strict modify, like modifyIORef').
--
-- A read that forces a slot's thunk WRITES THE VALUE BACK into the slot.
-- Observationally nothing changes (the thunk is memoized either way, and
-- an element is an Int, never a nil-represented value, so no hole can
-- appear); what changes is the slot's TYPE as the JIT sees it. A slot
-- holds a thunk only between its store and its first read, so with the
-- write-back the reads a hot loop performs see numbers, and the read
-- helper is monomorphic where it matters. Without it every re-read of a
-- stored suspension went through the thunk table, the read helper's
-- root trace on LuaJIT was specialized to whichever slot type the
-- recording call happened to load (a number or a thunk), and the other
-- type left the trace on every call for the rest of the run — the
-- tracker canary sat at 6.6x in a third of runs and 8x in the rest,
-- decided by hotcount-hash collisions under ASLR (2026-09-07).
local function __mll_ma_new(size, init)
    return function()
        size = __force(size)
        local t = {}; for i = 1, size do t[i] = init end; return t
    end
end
local function __mll_ma_read(arr, idx)
    return function()
        local t, i = __force(arr), __force(idx) + 1
        local v = t[i]
        if getmetatable(v) == __thunk_mt then v = __force(v); t[i] = v end
        return v
    end
end
local function __mll_ma_write(arr, idx, val)
    return function() __force(arr)[__force(idx) + 1] = val end
end
local function __mll_ma_modify(arr, idx, f)
    -- The action may run more than once (a stored first-class action, a
    -- list of actions traversed twice): every run must read the SAME
    -- captured index. Rebinding the upvalue (`idx = __force(idx) + 1`)
    -- made the second run modify index+1, the third index+2.
    return function()
        local a, i = __force(arr), __force(idx) + 1
        a[i] = __force(__force(f)(a[i]))
    end
end
local function __mll_ma_length(arr)
    return function() return #__force(arr) end
end
local function __mll_ma_from_list(xs)
    return function()
        xs = __force(xs); local t = {}; local cur = xs
        while cur ~= nil do t[#t+1] = __mll_head(cur); cur = __mll_tail(cur) end
        return t
    end
end
local function __mll_ma_to_list(arr)
    return function()
        arr = __force(arr); local r = nil
        for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
        return r
    end
end
-- Fused ST array ops: identical effects to __mll_ma_* but performed
-- immediately (no action-closure allocation, no __mll_run dispatch). The
-- codegen emits these only in run-once do-block position; first-class ST
-- actions keep the __mll_ma_* closure form. See st_intrinsic_fused.
local function __mll_st_new(size, init)
    size = __force(size)
    local t = {}; for i = 1, size do t[i] = init end; return t
end
local function __mll_st_read(arr, idx)
    -- The slot's thunk check inline (one call level less than an outer
    -- __force on the hot read path; a number slot returns directly), and
    -- the forced value written back (see the laziness note above). A
    -- bind-chain read whose array and index are duplicable expressions
    -- is not spelled as this call at all but INLINE at its site (codegen
    -- action.rs try_inline_st_read) with __mll_st_settle as its thunk
    -- arm; this helper serves the remaining shapes.
    local t, i = __force(arr), __force(idx) + 1
    local v = t[i]
    if getmetatable(v) == __thunk_mt then v = __force(v); t[i] = v end
    return v
end
-- The thunk arm of an inline read: `x <- readSTArray arr i` is emitted at
-- its site as the slot load and the thunk test, and only a slot still
-- holding its stored suspension comes here — forced, written back,
-- returned. The rare arm out of line keeps the site small; the common
-- arm inline is what matters on LuaJIT: a small polymorphic helper
-- called from interpreted code gets a function-root trace specialized
-- to whichever slot type the recording call loaded, and the other type
-- exits that trace on every call, its side traces running off into the
-- caller and aborting — the bimodal tracker canary. With the load in the
-- caller's own bytecode the type split lives where side traces can
-- attach (inside the caller's loop) or costs nothing (interpreted).
local function __mll_st_settle(t, i, v)
    v = __force(v); t[i] = v; return v
end
local function __mll_st_write(arr, idx, val)
    __force(arr)[__force(idx) + 1] = val
end
local function __mll_st_modify(arr, idx, f)
    arr = __force(arr); idx = __force(idx) + 1
    arr[idx] = __force(__force(f)(arr[idx]))
end
local function __mll_st_length(arr)
    return #__force(arr)
end
local function __mll_st_from_list(xs)
    xs = __force(xs); local t = {}; local cur = xs
    while cur ~= nil do t[#t+1] = __mll_head(cur); cur = __mll_tail(cur) end
    return t
end
local function __mll_st_to_list(arr)
    arr = __force(arr); local r = nil
    for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
    return r
end
-- IORef runtime (Data.IORef): a ref is one metatable-tagged slot. The tag
-- exists so a ref is never mistaken for a structural value (eq_IORef is
-- pointer identity, GHC's `instance Eq (IORef a)`; the mt carries no __eq,
-- so plain == on two cells IS identity). The slot holds the value AS
-- STORED — possibly a thunk: writes don't force (GHC's writeIORef is lazy
-- in the value), and reads hand the stored value back unforced, so a bound
-- `x <- readIORef r` shares one memoization with the cell exactly like
-- GHC shares the thunk. nil-represented values (Nothing, [], ()) need no
-- boxing here: a slot read/write has no presence semantics (unlike the
-- scalar HashMap store, where t[k] = nil deletes the entry).
local __mll_ioref_mt = {}
-- First-class action closures (a stored/passed `writeIORef r 5` builds the
-- action without performing it; building forces NOTHING). The modify
-- lesson from __mll_ma_modify applies: a first-class action may run more
-- than once, so every run re-reads the cell and no upvalue is rebound.
local function __mll_ref_new(v)
    return function() return setmetatable({v}, __mll_ioref_mt) end
end
local function __mll_ref_read(r)
    return function() return __force(r)[1] end
end
local function __mll_ref_write(r, v)
    return function() __force(r)[1] = v end
end
local function __mll_ref_modify(r, f)
    -- GHC's lazy modifyIORef: read >>= write . f — the cell ends up holding
    -- the UNEVALUATED `f old`, with `old` sampled at perform time. Forcing
    -- the thunk later memoizes into it, shared with every reader.
    return function()
        local c = __force(r)
        local old = c[1]
        c[1] = __thunk(function() return __force(f)(old) end)
    end
end
local function __mll_ref_modify_strict(r, f)
    -- modifyIORef': forces the NEW value to WHNF before the store (GHC:
    -- `let x' = f x in x' `seq` writeIORef ref x'`). The old value is
    -- passed to f as stored — f decides whether to evaluate it.
    return function()
        local c = __force(r)
        c[1] = __force(__force(f)(c[1]))
    end
end
-- Fused twins: identical effects, performed immediately — emitted only in
-- run-once do-block position (see ioref arms in st_intrinsic_fused). Note
-- __mll_ioref_read may return a thunk: it is the one fused intrinsic whose
-- result is NOT claimed WHNF (fused_result_is_whnf in codegen/action.rs).
local function __mll_ioref_new(v)
    return setmetatable({v}, __mll_ioref_mt)
end
local function __mll_ioref_read(r)
    return __force(r)[1]
end
local function __mll_ioref_write(r, v)
    __force(r)[1] = v
end
local function __mll_ioref_modify(r, f)
    local c = __force(r)
    local old = c[1]
    c[1] = __thunk(function() return __force(f)(old) end)
end
local function __mll_ioref_modify_strict(r, f)
    local c = __force(r)
    c[1] = __force(__force(f)(c[1]))
end
local function ne_IORef(a, b) return __force(a) ~= __force(b) end
local function eq_IORef(a, b)
    -- Pointer identity (GHC: two refs are == iff they are the same ref).
    -- No __eq metamethod exists on __mll_ioref_mt, so == is raw identity.
    return __force(a) == __force(b)
end
local function __assert_whnf(x)
    -- TEST-ONLY claim checker (WHNF refutation, compile_with_whnf_refutation
    -- in lib.rs): wraps each emission whose codegen predicate CLAIMED the
    -- value is already forced (expr_yields_whnf, the concrete_vars bare
    -- reads that action_result_is_whnf justifies, infix_yields_whnf).
    -- Inspecting the metatable forces nothing, so a refutation is loud and a
    -- confirmation changes no behavior. Production compiles never reference
    -- this, so the on-demand subset never emits it.
    if getmetatable(x) == __thunk_mt then
        error("WHNF claim refuted: a claimed forced position holds an unforced thunk", 2)
    end
    return x
end
local function __show_Number_checked(x)
    -- TEST-ONLY invariant checker (rebound over show_Number by the
    -- WHNF-refutation build, like __force below): a Number is an IEEE
    -- double on every host and never carries Lua's integer subtype — every
    -- producer (a literal, the folder, fromInteger, read, an FFI result, a
    -- map key read back) delivers the float subtype. A leak shows up where
    -- it is printed, so the type-directed show is the probe; a doubles-only
    -- host (no math.type) has nothing to refute.
    x = __force(x)
    if math.type ~= nil and math.type(x) == "integer" then
        error("Number subtype invariant refuted: a Number value holds Lua's integer subtype", 2)
    end
    return __mll_show_double(x)
end
local function __assert_purebare(x)
    -- TEST-ONLY claim checker for pure_value_bare_is_safe: a pure value may
    -- escape to a caller's __mll_run unboxed only when forcing it is a no-op
    -- (not a thunk) AND it can never be a Lua function (which __mll_run's
    -- action test would wrongly call). Either half failing is a refutation.
    if getmetatable(x) == __thunk_mt or type(x) == "function" then
        error("pure bare claim refuted: an escaping pure value is a thunk or a Lua function", 2)
    end
    return x
end
local function __force_checked(x)
    -- TEST-ONLY checked twin of __force (defined near the top of this file;
    -- keep the memoization protocol — the closure-free arity dispatch
    -- included — in step with it). The WHNF-refutation mode rebinds
    -- `__force = __force_checked` after the prelude block: every prelude
    -- caller closes over the VARIABLE __force, so the rebind covers them
    -- all. The added check is the one prose invariant __force relies on
    -- (see the comment block above __mll_head): a thunk body must return
    -- WHNF, never a raw thunk, because __force unwraps exactly one level.
    -- A nested thunk here is a codegen site that forgot a force.
    if getmetatable(x) == __thunk_mt then
        if x[2] then return x[1] end
        local n = x[3]
        local val
        if n == nil then val = x[1]()
        elseif n == 4 then val = x[1](x[4], x[5], x[6], x[7])
        else val = x[1](x[4], x[5], x[6])
        end
        if getmetatable(val) == __thunk_mt then
            error("one level force invariant refuted: a thunk body returned a raw thunk", 2)
        end
        x[1] = val
        x[2] = true
        if n ~= nil then
            x[3], x[4], x[5], x[6] = nil, nil, nil, nil
            if n == 4 then x[7] = nil end
        end
        return val
    end
    return x
end
