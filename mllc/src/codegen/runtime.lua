-- MLL Runtime
local __unpack = table.unpack or unpack

-- Thunk infrastructure (non-strict evaluation)
local __thunk_mt = {}
local __cons_mt = {}
-- Tags a `Just` wrapper (see the Maybe constructor below); declared here so the
-- generic `show`/`__mll_to_lua` can identify it as an upvalue.
local __just_mt = {}
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
local function __force(x)
    if getmetatable(x) == __thunk_mt then
        if x[2] then return x[1] end
        local val = x[1]()
        x[1] = val
        x[2] = true
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
    if l.__lazy then
        l[2] = l[2]()
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
    if l.__lazy then
        -- Suspend the generator instead of running it: __thunk memoizes on
        -- first force and is stored back into the cell, so every later read
        -- (lazy or forcing) shares the one evaluation.
        l[2] = __thunk(l[2])
        l.__lazy = nil
    end
    return l[2]
end

-- List append (second arg is a thunk for laziness)
local function __mll_list_append(xs, ys_thunk)
    xs = __force(xs)
    if xs == nil then return ys_thunk() end
    return __mll_lazy_cons(__mll_head(xs), function()
        return __mll_list_append(__mll_tail(xs), ys_thunk)
    end)
end

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
    if k == "chk" then
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
            r[key] = val
        end
        return r
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
        -- Validation-only: check the elements, keep the host's array.
        for i = 1, #desc.es do
            if desc.es[i] then __mll_ffi_decode(desc.es[i], v[i], root, dir) end
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
        -- (its metatable and any undeclared fields stay intact).
        for i = 1, #desc.fs do
            local f = desc.fs[i]
            if f.d then __mll_ffi_decode(f.d, v[f.n], root, dir) end
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
        for k, val in pairs(v) do
            local x = __force(val)
            if d.v then x = __mll_arg_marshal(x, d.v) end
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
-- value); the host's result crosses Lua→mata-ll and is decoded by the
-- declared result type (ret_desc: an FFI decode descriptor, or false to pass
-- the raw value through). `root` locates decode errors.
__mll_wrap_callback_in = function(f, n, out_descs, ret_desc, root)
    return function(...)
        local args = {...}
        for i = 1, n do
            local d = out_descs[i]
            if d then args[i] = __mll_arg_marshal(args[i], d) else args[i] = __force(args[i]) end
        end
        local r = f(__unpack(args, 1, n))
        if ret_desc then r = __mll_ffi_decode(ret_desc, r, root) end
        return r
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

-- Int-position number show. On Lua 5.3+ an Int value is a native
-- integer and tostring is exact; a whole float (LuaJIT has only doubles)
-- prints via %.0f so large magnitudes never fall into e-notation.
local function __mll_show_integer(x)
    if math.type ~= nil and math.type(x) == "integer" then return tostring(x) end
    if x ~= x or x == math.huge or x == -math.huge or x % 1 ~= 0 then
        return __mll_show_double(x)
    end
    return string.format("%.0f", x)
end

-- Primitives that require Lua runtime dispatch
local function not_(x) return not __force(x) end
local function engage(f, ...)
    if select('#', ...) > 0 then return __force(f)(...) else return __force(f) end
end
local function liftIO(action) return action end
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
        -- integer-style — the erased path cannot distinguish 1 :: Int
        -- from 1.0 :: Double there; the type-directed show_Number path can
        -- and does.
        if math.type ~= nil then
            if math.type(x) == "integer" then return tostring(x) end
            return __mll_show_double(x)
        end
        if x ~= x or x == math.huge or x == -math.huge or x % 1 ~= 0 then
            return __mll_show_double(x)
        end
        return string.format("%.0f", x)
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
-- Read parsing (GHC parity). A read trims surrounding space and one
-- layer of parentheses; anything the type's grammar does not cover
-- raises GHC's exact "Prelude.read: no parse" (a catchable error).
-- The old readers accepted garbage: tonumber let read @Int return a
-- FRACTION ("3.5") or nil ("junk"), and read @Bool mapped everything
-- that was not "True" to False.
local function __mll_read_trim(s)
    s = string.gsub(s, "^%s+", "")
    s = string.gsub(s, "%s+$", "")
    if string.byte(s, 1) == 40 and string.byte(s, #s) == 41 then
        s = string.gsub(string.gsub(s, "^%(%s*", ""), "%s*%)$", "")
    end
    return s
end
local function __mll_read_int(s)
    s = __mll_read_trim(__force(s))
    if not string.match(s, "^%-?%d+$") then error("Prelude.read: no parse") end
    return tonumber(s)
end
local function __mll_read_number(s)
    s = __mll_read_trim(__force(s))
    if not (string.match(s, "^%-?%d+$")
        or string.match(s, "^%-?%d+%.%d+$")
        or string.match(s, "^%-?%d+[eE][%+%-]?%d+$")
        or string.match(s, "^%-?%d+%.%d+[eE][%+%-]?%d+$")) then
        error("Prelude.read: no parse")
    end
    return tonumber(s) + 0.0
end
local function __mll_read_bool(s)
    s = __mll_read_trim(__force(s))
    if s == "True" then return true end
    if s == "False" then return false end
    error("Prelude.read: no parse")
end
local function show_Int(x) return __mll_show_integer(__force(x)) end
-- Type-directed Double show: a Number-typed value may be held as a native
-- integer (integer-valued arithmetic on Lua 5.3+, every LuaJIT number), so
-- the double formatter is called unconditionally — GHC shows 3 :: Double
-- as "3.0".
local function show_Number(x) return __mll_show_double(__force(x)) end
local function show_String(x) return __mll_show_string(__force(x)) end
local function show_Bool(x) return show(x) end
local function show_List_(x) return show(x) end
local function show_Maybe(x) return show(x) end
-- Unit's runtime rep is nil (same as Nothing/[]), so the type-erased generic
-- `show` cannot render it; the Show () instance dispatches here type-directedly.
local function show_Unit(x) return "()" end
local function eq_Int(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Number(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_String(a, b) a = __force(a); b = __force(b); return a == b end
local function eq_Bool(a, b) a = __force(a); b = __force(b); return a == b end
-- () == () is always True; both sides are nil at runtime.
local function eq_Unit(a, b) return true end
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
local function ord_compare__Number(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__String(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function ord_compare__ByteString(a, b) a = __force(a); b = __force(b); if a < b then return 1 elseif b < a then return 3 else return 2 end end
local function semigroup_String(a, b) a = __force(a); b = __force(b); return a .. b end
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
-- head forces the element (a value-consumer under the head-consumption
-- contract): it RETURNS the head as its result, and the WHNF-return invariant
-- says a function may never return a raw thunk — the caller's one-level
-- __force would mistake the nested thunk for the value. `head [1, ⊥]` still
-- returns 1 (only the first element is forced), and a merely *stored*
-- `head xs` stays unevaluated because the call itself sits inside a thunk.
local function head(xs) return __force(__mll_head(xs)) end
local function tail(xs) return __mll_tail(xs) end
local function map(f, xs)
    f = __force(f); xs = __force(xs)
    if xs == nil then return nil end
    return __mll_lazy_cons(f(__mll_head(xs)), function()
        return map(f, __mll_tail(xs))
    end)
end
local function filter(pred, xs)
    pred = __force(pred); xs = __force(xs)
    if xs == nil then return nil end
    local h = __mll_head(xs)
    if pred(h) then
        return __mll_lazy_cons(h, function() return filter(pred, __mll_tail(xs)) end)
    else
        return filter(pred, __mll_tail(xs))
    end
end
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
        return __mll_lazy_cons(__mll_head(xs), function() return take(n - 1, __mll_tail_lazy(xs)) end)
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
        local first = __mll_cons(__mll_head(xs), nil)
        local last = first
        n = n - 1
        while n > 0 do
            xs = __force(__mll_tail_lazy(xs))
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
local function drop(n, xs)
    n = __force(n); xs = __force(xs)
    while n > 0 and xs ~= nil do
        xs = __mll_tail(xs)
        n = n - 1
    end
    return xs
end
local function zipWith(f, xs, ys)
    f = __force(f); xs = __force(xs); ys = __force(ys)
    if xs == nil or ys == nil then return nil end
    return __mll_lazy_cons(f(__mll_head(xs), __mll_head(ys)), function()
        return zipWith(f, __mll_tail(xs), __mll_tail(ys))
    end)
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
    f = __force(f); t = __force(t)
    if t == nil then return __force(z) end
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
    f = __force(f); t = __force(t)
    if t == nil then return __force(z) end
    local mt = getmetatable(t)
    if mt == __just_mt then return f(z, t[1]) end
    if mt ~= __cons_mt then
        error("foldl: type-erased fold over a structure that is not a list or Maybe")
    end
    local acc = z
    local cur = t
    while cur ~= nil do
        acc = f(acc, __mll_head(cur))
        cur = __mll_tail(cur)
    end
    return __force(acc)
end
-- Currying adapters for the hand-written generic builtins above. Compiled
-- mata-ll functions are N-ary (all count_arrows(type) arguments in one call),
-- and compiled generic code is specialized per instantiation so its call
-- sites always match. map/zipWith, however, exist as ONE erased copy and call
-- their function argument with its GENERIC arity (map: 1, zipWith: 2). When
-- the result type variable is itself instantiated to a function — e.g.
-- `map (\n -> \x -> x + n) ns` builds a list of adders — the argument's real
-- arity exceeds that view, so the compiler wraps it: absorb the builtin's
-- call, then return a closure over the remaining arguments (the same shape a
-- compiled partial application has).
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
-- Hash helper
local function __mll_hashstr(s) s = __force(s); local h = 5381 for i = 1, #s do h = ((h * 33) + string.byte(s, i)) % 2147483647 end return h end

-- HashMap runtime (backed by Lua tables)
local hashmap_empty = {}
local function hashmap_insert(k, v, m) k = __force(k); v = __force(v); m = __force(m); local t = {} for a,b in pairs(m) do t[a] = b end t[k] = v return t end
local function hashmap_lookup(k, m) k = __force(k); m = __force(m); local v = m[k] if v == nil then return nil else return Just(v) end end
local function hashmap_delete(k, m) k = __force(k); m = __force(m); local t = {} for a,b in pairs(m) do t[a] = b end t[k] = nil return t end
local function hashmap_size(m) m = __force(m); local n = 0 for _ in pairs(m) do n = n + 1 end return n end
-- Key sort comparator: Bool is a legal key type but Lua cannot `<`
-- booleans; order false < true. (Keys within one map share one type.)
local function __mll_hm_lt(a, b)
    if type(a) == "boolean" then
        return (a and 1 or 0) < (b and 1 or 0)
    end
    return a < b
end
local function hashmap_keys(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do r = __mll_cons(ks[i], r) end return r end
local function hashmap_values(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do r = __mll_cons(m[ks[i]], r) end return r end
local function hashmap_member(k, m) k = __force(k); m = __force(m); return m[k] ~= nil end
local function show_HashMap(m) m = __force(m); local parts = {} for k, v in pairs(m) do parts[#parts+1] = show(k) .. " -> " .. show(v) end table.sort(parts) return "{" .. table.concat(parts, ", ") .. "}" end
local function hashmap_fromList(xs) xs = __force(xs); local t = {} local cur = xs while cur ~= nil do local pair = __force(__mll_head(cur)) t[__force(pair[1])] = __force(pair[2]) cur = __mll_tail(cur) end return t end
local function hashmap_toList(m) m = __force(m); local r = nil local ks = {} for k in pairs(m) do ks[#ks+1] = k end table.sort(ks, __mll_hm_lt) for i = #ks, 1, -1 do r = __mll_cons({ks[i], m[ks[i]]}, r) end return r end

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
local function __mll_show_maybe(elem_show, x)
    -- Type-directed Maybe show. `Nothing` is nil; `Just p` is a tagged wrapper
    -- whose payload is field [1] (itself possibly nil, e.g. `Just Nothing`), so
    -- nesting renders faithfully: `Just Nothing`, `Just (Just 5)`, etc.
    x = __force(x)
    if x == nil then return "Nothing" end
    return "Just " .. __mll_show_arg(elem_show(x[1]))
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
-- Success: Right (decoded val), Failure: Left errmsg. `desc` is the FFI decode
-- descriptor for the success payload (false = pass through) and `root` the
-- location phrase for decode error messages — a structured success value (a
-- list, a record) crosses the FFI boundary here exactly like any other FFI
-- result and must be decoded the same way. The error value is tostring'd:
-- hosts may signal failure with a non-string error object, while Left is
-- declared String.
local function __mll_try(desc, root, val, err)
    if val == nil then
        if err == nil then return {1, "unknown error"} end
        return {1, tostring(err)}
    end
    if desc then val = __mll_ffi_decode(desc, val, root) end
    return {2, val}
end
-- LuaCatch/LuaIOCatch: run a Lua function under pcall, returning Either String a.
-- Success: Right (decoded), Failure: Left errmsg. `desc` is the FFI decode
-- descriptor for the success payload (false = pass through) and `root` the
-- Lua-side function name for decode error messages. tostring() because
-- error() can raise non-string values, while Left is declared String. Arguments
-- are already forced by the caller (outside pcall) so only the Lua function's
-- own error() is captured — a shape mismatch in the *successful* result is a
-- host-integration bug and still raises, it does not become a Left.
local function __mll_pcall(desc, root, f, ...)
    local ok, res = pcall(f, ...)
    if not ok then return {1, tostring(res)} end
    if desc then res = __mll_ffi_decode(desc, res, root) end
    return {2, res}
end
-- Exception handling: try wraps an IO action in pcall, returning Either String a
-- action is a closure (deferred by codegen) so errors happen inside pcall
local function try_(action)
    return function()
        local ok, result = pcall(action)
        if ok then return {2, __mll_unbox(result)} else return {1, tostring(result)} end
    end
end
-- catch runs an IO action; on error, passes the message to a handler
local function catch_(action, handler)
    return function()
        local ok, result = pcall(action)
        if ok then return __mll_unbox(result)
        else return __mll_run(__force(__force(handler)(tostring(result)))) end
    end
end

-- Iterator-to-lazy-list: calls a Lua iterator factory and builds a lazy MLL list.
-- Single-value iterators produce a flat list; multi-value iterators pack into tuples.
-- `decode_desc` (may be nil) is the type-directed decoder for the ELEMENT type:
-- each yielded value crosses the FFI boundary and is decoded exactly as an
-- ordinary FFI result is (a list element -> cons list, a Maybe -> Just/Nothing,
-- a mismatched shape -> a clear localized error). nil means the element already
-- matches the mata-ll representation (a scalar/opaque element) -- pass it raw.
local function __mll_iter(factory, decode_desc, root, ...)
    local iter = factory(...)
    local go
    -- `step` receives one iterator call's results as varargs, so a
    -- single-value iterator (the common case) allocates nothing per element;
    -- only a genuine multi-value yield is packed into a tuple table.
    -- Trailing nils are ignored, as `#{iter()}` did: a yield of `k, nil` is
    -- the single value k.
    local function step(...)
        local n = select('#', ...)
        while n > 0 and select(n, ...) == nil do n = n - 1 end
        if n == 0 or (...) == nil then return nil end
        local val
        if n == 1 then val = (...) else val = {...} end
        if decode_desc ~= nil then val = __mll_ffi_decode(decode_desc, val, root) end
        return __mll_lazy_cons(val, go)
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
local function eq_ByteString(a, b) return __force(a) == __force(b) end

-- MutArray runtime (mutable integer arrays, backed by Lua tables)
-- Operations are effectful and run inside LuaIO s.
-- 0-based indexing externally, 1-based internally.
-- ST array primitives: these run inside runST which provides scoping,
-- so they perform directly (no action closure wrapping needed).
local function __mll_ma_new(size, init)
    return function()
        size = __force(size); init = __force(init)
        local t = {}; for i = 1, size do t[i] = init end; return t
    end
end
local function __mll_ma_read(arr, idx)
    return function() return __force(arr)[__force(idx) + 1] end
end
local function __mll_ma_write(arr, idx, val)
    return function() __force(arr)[__force(idx) + 1] = __force(val) end
end
local function __mll_ma_modify(arr, idx, f)
    -- The action may run more than once (a stored first-class action, a
    -- list of actions traversed twice): every run must read the SAME
    -- captured index. Rebinding the upvalue (`idx = __force(idx) + 1`)
    -- made the second run modify index+1, the third index+2.
    return function()
        local a, i = __force(arr), __force(idx) + 1
        a[i] = __force(f)(a[i])
    end
end
local function __mll_ma_length(arr)
    return function() return #__force(arr) end
end
local function __mll_ma_from_list(xs)
    return function()
        xs = __force(xs); local t = {}; local cur = xs
        while cur ~= nil do t[#t+1] = __force(__mll_head(cur)); cur = __mll_tail(cur) end
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
    size = __force(size); init = __force(init)
    local t = {}; for i = 1, size do t[i] = init end; return t
end
local function __mll_st_read(arr, idx)
    return __force(arr)[__force(idx) + 1]
end
local function __mll_st_write(arr, idx, val)
    __force(arr)[__force(idx) + 1] = __force(val)
end
local function __mll_st_modify(arr, idx, f)
    arr = __force(arr); idx = __force(idx) + 1
    arr[idx] = __force(f)(arr[idx])
end
local function __mll_st_length(arr)
    return #__force(arr)
end
local function __mll_st_from_list(xs)
    xs = __force(xs); local t = {}; local cur = xs
    while cur ~= nil do t[#t+1] = __force(__mll_head(cur)); cur = __mll_tail(cur) end
    return t
end
local function __mll_st_to_list(arr)
    arr = __force(arr); local r = nil
    for i = #arr, 1, -1 do r = __mll_cons(arr[i], r) end
    return r
end
