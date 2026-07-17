-- MLL Runtime (on-demand subset)
-- MLL Runtime
local __unpack = table.unpack or unpack

-- Thunk infrastructure (non-strict evaluation)
local __thunk_mt = {}
local __cons_mt = {}
-- Tags a `Just` wrapper (see the Maybe constructor below); declared here so the
-- generic `show`/`__mll_to_lua` can identify it as an upvalue.
local __just_mt = {}
-- Tags a suspended `pure`/`return` value — a "pure action" that has escaped its
-- defining function. `__mll_run` (and __mll_perform) unwrap it WITHOUT forcing
-- or calling the payload, so a `pure ⊥` bound across a function boundary does
-- not raise until demanded, and a returned `pure <function>` is delivered as a
-- value rather than mistaken for an action closure to invoke. See gen_action.
local __mll_pure_mt = {}
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
local function __mll_tail(l)
    l = __force(l)
    if l.__lazy then
        l[2] = l[2]()
        l.__lazy = nil
    end
    -- The tail may be an unforced thunk: a recursive cons whose tail is a
    -- variable (e.g. `x : rest`) stores it raw so the spine is not forced
    -- eagerly at construction (which would diverge on infinite lists). Force
    -- it to WHNF here — one spine step, on demand — and memoize, so the cell
    -- meets the "tail is WHNF" invariant that show/eq/append rely on.
    local t = l[2]
    if getmetatable(t) == __thunk_mt then
        t = __force(t)
        l[2] = t
    end
    return t
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
    if x[1] == nil then
        local result = {}
        for k, v in pairs(x) do result[k] = __mll_to_lua(v) end
        return result
    end
    -- Tuple or ADT: force each element
    local result = {}
    for i, v in ipairs(x) do result[i] = __mll_to_lua(v) end
    return result
end

-- Forward declarations for mutual recursion
local __lua_to_mll, __mll_wrap_callback

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

-- Run an IO action: force thunks, then call the action closure
local function __mll_run(action)
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
-- Perform an IO action (normally a function closure; a pure action carries its
-- result and is returned unforced, exactly as in __mll_run)
local function __mll_perform(action)
    if getmetatable(action) == __mll_pure_mt then return action[1] end
    action = __force(action)
    if getmetatable(action) == __mll_pure_mt then return action[1] end
    return __mll_unbox(action())
end
local function show(x)
    x = __force(x)
    if type(x) == "number" then return tostring(x)
    elseif type(x) == "string" then return x
    elseif type(x) == "boolean" then
        if x then return "True" else return "False" end
    elseif type(x) == "nil" then return "Nothing"
    elseif type(x) == "table" then
        -- A Just wrapper is tagged; render it as "Just <payload>" (its payload
        -- in field [1] may itself be nil, i.e. Just Nothing). Parenthesize a
        -- payload that is a constructor application or negative number, matching
        -- GHC's showsPrec 11 (same rule as __mll_show_arg, inlined so the generic
        -- show does not depend on a helper defined later).
        if getmetatable(x) == __just_mt then
            local inner = show(x[1])
            local c = string.byte(inner, 1)
            local d = string.byte(inner, 2)
            if c ~= nil and ((c >= 65 and c <= 90 and string.find(inner, " ", 1, true))
               or (c == 45 and d ~= nil and d >= 48 and d <= 57)) then
                inner = "(" .. inner .. ")"
            end
            return "Just " .. inner
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
            return "[" .. table.concat(parts, ", ") .. "]"
        end
        local parts = {}
        for i, v in ipairs(x) do parts[i] = show(v) end
        if type(x[1]) == "string" then return x[1] .. "(" .. table.concat(parts, ", ", 2) .. ")"
        else return "(" .. table.concat(parts, ", ") .. ")" end
    else return tostring(x) end
end
local function pure(x) return function() return x end end
-- Maybe: `Just x` is a metatable-tagged one-element wrapper (tag __just_mt,
-- declared above) so it is injective even when the payload's own runtime
-- representation is nil (`Nothing`, `[]`, or a nested `Just Nothing`).
-- `Nothing` stays nil.
local function Just(x) return setmetatable({x}, __just_mt) end
local Nothing = nil
local function show_Integer(x) return show(x) end
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
    if xs.__lazy then
        return __mll_lazy_cons(__mll_head(xs), function() return take(n - 1, __mll_tail(xs)) end)
    else
        return __mll_cons(__mll_head(xs), take(n - 1, __mll_tail(xs)))
    end
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
local function __mll_show_arg(s)
    s = __force(s)
    -- Parenthesize a derived-Show field at argument position: a constructor
    -- application ("Con a b") or a negative number, matching GHC's showsPrec 11.
    local c = string.byte(s, 1)
    if c == nil then return s end
    local d = string.byte(s, 2)
    if (c >= 65 and c <= 90 and string.find(s, " ", 1, true))
       or (c == 45 and d ~= nil and d >= 48 and d <= 57) then
        return "(" .. s .. ")"
    end
    return s
end

-- Generated by MATA-LL compiler (https://matall.org/)
local __MLLC_VERSION = "0.1.4"
local __MLLC_COMMIT = "a57e5564d5f0322ce078a9257a35c9716fcbeb77"

local __mll_fn = {}
__mll_fn[1] = function(_arg0)
    local _ffi0 = __force(_arg0)
    return print(_ffi0)
end

__mll_fn[2] = function(_arg0)
    local x = _arg0
    return __mll_run(__mll_fn[1]((show(x))))
end

__mll_fn[3] = 123

__mll_fn[4] = function(_eta0)
    return __force(function(_sec)
        return (__force(_sec) + __mll_fn[3])
    end)(_eta0)
end

__mll_fn[5] = function()
    return __mll_run(__mll_fn[6])
end

__mll_fn[7] = function(_arg0)
    local x = _arg0
    return __mll_run(__mll_fn[1]((show_Integer(x))))
end

__mll_fn[6] = function()
    return __mll_run(__mll_fn[7](__thunk(function() return __mll_fn[4](123) end)))
end


-- Entry point (skip when loaded via require)
local __mll_modname = ...
if __mll_modname == nil then __mll_run(__mll_fn[6]()) end

-- Exports
return {
    __MLLC_VERSION = __MLLC_VERSION,
    __MLLC_COMMIT = __MLLC_COMMIT,
    foo = (function()
        local __result = __force(__mll_fn[3])
        return __mll_to_lua(__result)
    end)(),
    bar = function(a1)
        local __result = __force(__mll_fn[4])(a1)
        if getmetatable(__result) == __mll_pure_mt then __result = __result[1]
        elseif type(__result) == "function" then __result = __mll_unbox(__result()) end
        return __mll_to_lua(__result)
    end,
    run = function(...)
        local args = {n = select('#', ...), ...}
        for i = 1, args.n do args[i] = __lua_to_mll(args[i]) end
        local __result = __force(__mll_fn[5])(__unpack(args, 1, args.n))
        if getmetatable(__result) == __mll_pure_mt then __result = __result[1]
        elseif type(__result) == "function" then __result = __mll_unbox(__result()) end
        return __mll_to_lua(__result)
    end,
}
