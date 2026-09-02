-- mllc runtime: the arbitrary-precision Integer library (limb helpers,
-- divmod, metatable, decimal conversion, and the Integer-backed numeric
-- methods). runtime.rs splices everything below the `--#begin` line into
-- runtime.lua's text (at its `--#include runtime_integer.lua` marker) to
-- form the runtime prelude; the assembled text is byte-identical to the
-- historical single-file runtime.lua, so chunk boundaries, tree-shaking,
-- and emitted programs are unchanged.
--#begin
-- ===================================================================
-- Arbitrary-precision Integer (GHC's `Integer`, the numeric default).
--
-- Representation: a table `{sign, l0, l1, ...}` — sign in {-1,0,1}, limbs
-- little-endian in base 2^24, tagged with `__integer_mt`. Zero is `{0}`.
-- Base 2^24 is a POWER OF TWO, so `% B` and `/ B` are exact bit-shifts in
-- IEEE doubles: the whole library is exact on both Lua 5.3+ (native int)
-- and LuaJIT / 5.1 (doubles only), and uses no `//` or bitwise operators.
-- Always-boxed; add/sub/mul/divmod take small-MAGNITUDE fast paths
-- (__int_small/__int_box) that compute natively when both operands fit
-- two limbs, without ever changing what an Integer value IS.
-- ===================================================================
local __INT_B = 16777216  -- 2^24

-- ---- magnitude helpers: little-endian limb arrays, no sign ----
local function __int_utrim(m)
    local n = #m
    while n > 0 and m[n] == 0 do m[n] = nil; n = n - 1 end
    return m
end
local function __int_ucmp(a, b)
    local la, lb = #a, #b
    if la ~= lb then if la < lb then return -1 else return 1 end end
    for i = la, 1, -1 do
        local x, y = a[i], b[i]
        if x ~= y then if x < y then return -1 else return 1 end end
    end
    return 0
end
local function __int_uadd(a, b)
    local r, carry = {}, 0
    local n = #a; if #b > n then n = #b end
    for i = 1, n do
        local s = (a[i] or 0) + (b[i] or 0) + carry
        if s >= __INT_B then r[i] = s - __INT_B; carry = 1 else r[i] = s; carry = 0 end
    end
    if carry > 0 then r[n + 1] = carry end
    return r
end
local function __int_usub(a, b)  -- a - b, requires |a| >= |b|
    local r, borrow = {}, 0
    for i = 1, #a do
        local s = a[i] - (b[i] or 0) - borrow
        if s < 0 then r[i] = s + __INT_B; borrow = 1 else r[i] = s; borrow = 0 end
    end
    return __int_utrim(r)
end
local function __int_umul(a, b)
    local r = {}
    local la, lb = #a, #b
    for i = 1, la + lb do r[i] = 0 end
    for i = 1, la do
        local ai = a[i]
        if ai ~= 0 then
            local carry = 0
            for j = 1, lb do
                -- acc < 2^24 + 2^48 + 2^25 < 2^49 — exact in a double.
                local acc = r[i + j - 1] + ai * b[j] + carry
                local lo = acc % __INT_B
                r[i + j - 1] = lo
                carry = (acc - lo) / __INT_B
            end
            local k = i + lb
            while carry ~= 0 do
                local acc = (r[k] or 0) + carry
                local lo = acc % __INT_B
                r[k] = lo
                carry = (acc - lo) / __INT_B
                k = k + 1
            end
        end
    end
    return __int_utrim(r)
end
local function __int_umul1(a, d)  -- magnitude * single limb d (0 < d < B); fresh table
    local r, carry = {}, 0
    for i = 1, #a do
        local p = a[i] * d + carry   -- < 2^48 + 2^24, exact in a double
        local lo = p % __INT_B
        r[i] = lo
        carry = (p - lo) / __INT_B
    end
    if carry > 0 then r[#a + 1] = carry end
    return r
end
-- Schoolbook base-2^24 division of magnitudes (V nonzero): returns quotient,
-- remainder. Knuth's Algorithm D — O(#U * #V) limb operations, where the old
-- bit-by-bit binary loop was O(bits(U) * #V). Every intermediate stays below
-- 2^49, so the whole routine is exact in IEEE doubles; the floor of a native
-- float division is trusted only where the true quotient's distance to the
-- next integer (>= 1/divisor > 2^-24) exceeds the rounding error (< 2^-28).
local function __int_udivmod(U, V)
    if __int_ucmp(U, V) < 0 then return {}, U end
    local n = #V
    if n == 1 then
        -- Short division by one limb: carry < v the whole way down.
        local v, q, rem = V[1], {}, 0
        for i = #U, 1, -1 do
            local cur = rem * __INT_B + U[i]   -- < v*B < 2^48, exact
            local qd = math.floor(cur / v)     -- < B by the rem < v invariant
            q[i] = qd
            rem = cur - qd * v
        end
        if rem == 0 then return __int_utrim(q), {} end
        return __int_utrim(q), {rem}
    end
    -- D1 normalize: scale both by d so the divisor's top limb is >= B/2 —
    -- that is what makes the two-limb qhat estimate at most 1 too large.
    -- v keeps exactly n limbs (v[n]*d + carry <= d*(V[n]+1) - 1 < B); u gets
    -- one extra slot for the window top (u is a fresh copy — D4 mutates it).
    local d = math.floor(__INT_B / (V[n] + 1))
    local u = __int_umul1(U, d)
    local v = __int_umul1(V, d)
    local m = #U - n                 -- quotient limb count - 1 (0-based j below)
    if u[#U + 1] == nil then u[#U + 1] = 0 end
    local q = {}
    for j = m, 0, -1 do
        -- D3 estimate: qhat = floor of the window's top two limbs over v[n],
        -- then correct against the third limb until at most 1 too large.
        local num = u[j + n + 1] * __INT_B + u[j + n]   -- < 2^49, exact
        local qhat = math.floor(num / v[n])
        if qhat >= __INT_B then qhat = __INT_B - 1 end
        local rhat = num - qhat * v[n]
        while rhat < __INT_B and qhat * v[n - 1] > rhat * __INT_B + u[j + n - 1] do
            qhat = qhat - 1
            rhat = rhat + v[n]
        end
        -- D4 multiply-subtract qhat*v from the window u[j+1 .. j+n+1].
        local borrow, carry = 0, 0
        for i = 1, n do
            local p = qhat * v[i] + carry   -- < 2^48, exact
            local plo = p % __INT_B
            carry = (p - plo) / __INT_B
            local t = u[j + i] - plo - borrow
            if t < 0 then u[j + i] = t + __INT_B; borrow = 1
            else u[j + i] = t; borrow = 0 end
        end
        local top = u[j + n + 1] - carry - borrow
        if top < 0 then
            -- D6 add back (qhat was 1 too large; top == -1 and the add's
            -- final carry restores it to 0). Rare: ~2/B of the steps.
            qhat = qhat - 1
            local c = 0
            for i = 1, n do
                local s = u[j + i] + v[i] + c
                if s >= __INT_B then u[j + i] = s - __INT_B; c = 1
                else u[j + i] = s; c = 0 end
            end
            top = top + c
        end
        u[j + n + 1] = top
        q[j + 1] = qhat
    end
    -- D8 denormalize: the remainder is u[1..n] / d (short division; exact).
    local r, rem = {}, 0
    for i = n, 1, -1 do
        local cur = rem * __INT_B + u[i]   -- < d*B < 2^47, exact
        local qd = math.floor(cur / d)
        r[i] = qd
        rem = cur - qd * d
    end
    return __int_utrim(q), __int_utrim(r)
end

-- ---- signed operations on Integer tables ----
local function __int_mag(x)  -- copy the limbs out (drop the sign)
    local m = {}
    for i = 2, #x do m[i - 1] = x[i] end
    return m
end
local function __int_ucmp_t(a, b)  -- magnitude compare reading tables in place
    local la, lb = #a - 1, #b - 1
    if la ~= lb then if la < lb then return -1 else return 1 end end
    for i = la, 1, -1 do
        local x, y = a[i + 1], b[i + 1]
        if x ~= y then if x < y then return -1 else return 1 end end
    end
    return 0
end
local function __int_cmp(a, b)  -- full signed compare -> -1/0/1
    local sa, sb = a[1], b[1]
    if sa ~= sb then if sa < sb then return -1 else return 1 end end
    if sa == 0 then return 0 end
    local c = __int_ucmp_t(a, b)
    if sa < 0 then return -c else return c end
end
local function __int_tostring(x)
    local sign = x[1]
    if sign == 0 then return "0" end
    local m = __int_mag(x)
    local groups, D, n = {}, 10000000, #m  -- 10^7 < 2^24
    while n > 0 do
        local rem = 0
        for i = n, 1, -1 do
            local cur = rem * __INT_B + m[i]   -- < 2^48, exact
            local qd = math.floor(cur / D)
            m[i] = qd
            rem = cur - qd * D
        end
        while n > 0 and m[n] == 0 do n = n - 1 end
        groups[#groups + 1] = rem
    end
    -- %d, not tostring: limbs can carry Lua's float subtype (see
    -- __int_limb), and a float-typed group would print "6.0". Every
    -- group is < 10^7, so %d is exact on every host.
    local s = string.format("%d", groups[#groups])
    for i = #groups - 1, 1, -1 do s = s .. string.format("%07d", groups[i]) end
    if sign < 0 then s = "-" .. s end
    return s
end

-- Metatable. Type-erased identity: `show` via __tostring, a marker field so
-- generic `show` detects Integers without naming the library, and __eq/__lt/__le
-- so raw `==`/`<`/`<=` are correct. PLUS arithmetic metamethods (__add/__sub/
-- __mul/__mod/__unm) so any INLINE Lua arithmetic on Integers is correct — an
-- operator used as a first-class value (`let f = (+)`), a section (`(* 2)`), or
-- a context the monomorphizer left inline. The arithmetic functions are
-- forward-declared here so the metamethod closures can capture them.
local add_Integer, sub_Integer, mul_Integer, negate_Integer, mod_Integer, fromInteger_Integer
local function __int_coerce(v)
    if type(v) == "table" then return v end
    return fromInteger_Integer(v)
end
local __integer_mt = {
    __is_integer = true,
    __tostring = __int_tostring,
    -- __eq needs no coercion: Lua invokes it only when BOTH operands are
    -- tables, so a plain number can never reach it. __lt/__le DO fire on a
    -- mixed table/number pair, and __int_cmp indexes both operands — the
    -- same leaked-machine-number sources the arithmetic metamethods coerce
    -- against crashed a comparison with "attempt to index a number" (F10).
    __eq = function(a, b) return __int_cmp(a, b) == 0 end,
    __lt = function(a, b) return __int_cmp(__int_coerce(a), __int_coerce(b)) < 0 end,
    __le = function(a, b) return __int_cmp(__int_coerce(a), __int_coerce(b)) <= 0 end,
    __add = function(a, b) return add_Integer(__int_coerce(a), __int_coerce(b)) end,
    __sub = function(a, b) return sub_Integer(__int_coerce(a), __int_coerce(b)) end,
    __mul = function(a, b) return mul_Integer(__int_coerce(a), __int_coerce(b)) end,
    __mod = function(a, b) return mod_Integer(__int_coerce(a), __int_coerce(b)) end,
    __unm = function(a) return negate_Integer(a) end,
}
local __int_zero = setmetatable({0}, __integer_mt)
local function __int_mk(sign, mag)
    __int_utrim(mag)
    if #mag == 0 then return __int_zero end
    local t = {sign}
    for i = 1, #mag do t[i + 1] = mag[i] end
    return setmetatable(t, __integer_mt)
end

-- ---- small-magnitude fast paths ----
-- The signed machine value of an Integer whose magnitude fits TWO limbs
-- (< 2^48 — exact in an IEEE double and in a Lua 5.3+ integer alike), or
-- nil for anything larger. The representation stays always-boxed; these
-- only let add/sub/mul/divmod compute natively when the VALUES are small,
-- which skips the limb walks and their per-op table traffic.
local function __int_small(x)
    local n = #x
    if n == 2 then return x[1] * x[2] end
    if n == 3 then return x[1] * (x[2] + x[3] * 16777216) end
    if n == 1 then return 0 end
    return nil
end
-- Box a signed integer-valued machine number (any size a double holds
-- exactly; the loop runs at most thrice for the < 2^49 sums below).
local function __int_box(v)
    if v == 0 then return __int_zero end
    local sign = 1
    if v < 0 then sign = -1; v = -v end
    local t = {sign}
    local i = 2
    while v ~= 0 do
        local lo = v % __INT_B
        t[i] = lo
        v = (v - lo) / __INT_B
        i = i + 1
    end
    return setmetatable(t, __integer_mt)
end

-- Build an Integer from a machine number (the `fromInteger` conversion for a
-- literal that resolved to Integer, and for fromIntegral :: Int -> Integer).
fromInteger_Integer = function(n)
    n = __force(n)
    if type(n) == "table" then return n end  -- already an Integer
    if n == 0 then return __int_zero end
    if n < 0 and -n < 0 then return __int_mk(-1, {0, 0, 32768}) end  -- -(2^63)
    local sign = 1
    if n < 0 then sign = -1; n = -n end
    local mag = {}
    while n ~= 0 do
        local lo = n % __INT_B
        mag[#mag + 1] = lo
        n = (n - lo) / __INT_B      -- exact: (n-lo) is a multiple of 2^24
    end
    return __int_mk(sign, mag)
end
-- Parse a decimal string into an Integer (backs big literals and read_Integer).
local function __int_from_decimal(s)
    local sign, start = 1, 1
    local c0 = string.byte(s, 1)
    if c0 == 45 then sign = -1; start = 2 elseif c0 == 43 then start = 2 end
    local mag = {}
    for i = start, #s do
        local carry = string.byte(s, i) - 48
        for k = 1, #mag do
            local v = mag[k] * 10 + carry   -- < 2^28, exact
            local lo = v % __INT_B
            mag[k] = lo
            carry = (v - lo) / __INT_B
        end
        while carry ~= 0 do
            local lo = carry % __INT_B
            mag[#mag + 1] = lo
            carry = (carry - lo) / __INT_B
        end
    end
    return __int_mk(sign, mag)
end

negate_Integer = function(x)
    x = __force(x)
    if x[1] == 0 then return x end
    local t = {-x[1]}
    for i = 2, #x do t[i] = x[i] end
    return setmetatable(t, __integer_mt)
end
local function abs_Integer(x)
    x = __force(x)
    if x[1] >= 0 then return x end
    return negate_Integer(x)
end
local function signum_Integer(x)
    return fromInteger_Integer(__force(x)[1])
end
add_Integer = function(a, b)
    a = __force(a); b = __force(b)
    local va, vb = __int_small(a), __int_small(b)
    if va and vb then return __int_box(va + vb) end  -- |sum| < 2^49, exact
    local sa, sb = a[1], b[1]
    if sa == 0 then return b end
    if sb == 0 then return a end
    local ma, mb = __int_mag(a), __int_mag(b)
    if sa == sb then return __int_mk(sa, __int_uadd(ma, mb)) end
    local c = __int_ucmp(ma, mb)
    if c == 0 then return __int_zero end
    if c > 0 then return __int_mk(sa, __int_usub(ma, mb)) end
    return __int_mk(sb, __int_usub(mb, ma))
end
sub_Integer = function(a, b)
    a = __force(a); b = __force(b)
    local va, vb = __int_small(a), __int_small(b)
    if va and vb then return __int_box(va - vb) end  -- |diff| < 2^49, exact
    return add_Integer(a, negate_Integer(b))
end
mul_Integer = function(a, b)
    a = __force(a); b = __force(b)
    local sa, sb = a[1], b[1]
    if sa == 0 or sb == 0 then return __int_zero end
    local va, vb = __int_small(a), __int_small(b)
    if va and vb then
        local ma = va; if ma < 0 then ma = -ma end
        local mb = vb; if mb < 0 then mb = -mb end
        -- Exactness bound with rounding slack: 2^52/mb is itself a rounded
        -- quotient, so ma <= it only guarantees ma*mb < 2^52 + mb — still
        -- comfortably under 2^53 (mb < 2^48), where every product of two
        -- exact operands is exact. Aiming at 2^53 directly would leave no
        -- room for that rounding.
        if ma <= 4503599627370496 / mb then return __int_box(va * vb) end
    end
    return __int_mk(sa * sb, __int_umul(__int_mag(a), __int_mag(b)))
end
-- Truncating quotient/remainder (remainder takes the DIVIDEND's sign).
local function __int_qr_trunc(a, b)
    local va, vb = __int_small(a), __int_small(b)
    if va and vb then
        -- Native magnitude division with a one-step correction: ma/mb is
        -- the correctly-rounded real quotient (both exact, < 2^48), so
        -- floor of it can be off by at most one — q*mb and the remainder
        -- stay < 2^49, exact, which makes the check itself exact.
        local ma = va; if ma < 0 then ma = -ma end
        local mb = vb; if mb < 0 then mb = -mb end
        local q = math.floor(ma / mb)
        local r = ma - q * mb
        if r < 0 then q = q - 1; r = r + mb
        elseif r >= mb then q = q + 1; r = r - mb end
        local sq = 1; if (va < 0) ~= (vb < 0) then sq = -1 end
        if va < 0 then r = -r end
        return __int_box(sq * q), __int_box(r)
    end
    local qm, rm = __int_udivmod(__int_mag(a), __int_mag(b))
    return __int_mk(a[1] * b[1], qm), __int_mk(a[1], rm)
end
local function quotRem_Integer(a, b)
    a = __force(a); b = __force(b)
    if b[1] == 0 then error("divide by zero: `quotRem` has no result when the divisor is 0") end
    local q, r = __int_qr_trunc(a, b)
    return {q, r}
end
local function quot_Integer(a, b)
    a = __force(a); b = __force(b)
    if b[1] == 0 then error("divide by zero: `quot` has no result when the divisor is 0") end
    local q, _r = __int_qr_trunc(a, b)
    return q
end
local function rem_Integer(a, b)
    a = __force(a); b = __force(b)
    if b[1] == 0 then error("divide by zero: `rem` has no result when the divisor is 0") end
    local _q, r = __int_qr_trunc(a, b)
    return r
end
-- Flooring div/mod (remainder takes the DIVISOR's sign), from the truncating
-- pair: they agree unless the signs differ and the division is inexact.
local function divMod_Integer(a, b)
    a = __force(a); b = __force(b)
    if b[1] == 0 then error("divide by zero: `divMod` has no result when the divisor is 0") end
    local q, r = __int_qr_trunc(a, b)
    if r[1] ~= 0 and (a[1] < 0) ~= (b[1] < 0) then
        q = sub_Integer(q, fromInteger_Integer(1))
        r = add_Integer(r, b)
    end
    return {q, r}
end
local function div_Integer(a, b) local p = divMod_Integer(a, b); return p[1] end
mod_Integer = function(a, b) local p = divMod_Integer(a, b); return p[2] end

local function show_Integer(x) return __int_tostring(__force(x)) end
local function eq_Integer(a, b) return __int_cmp(__force(a), __force(b)) == 0 end
local function ord_lt__Integer(a, b) return __int_cmp(__force(a), __force(b)) < 0 end
local function ord_gt__Integer(a, b) return __int_cmp(__force(a), __force(b)) > 0 end
local function ord_le__Integer(a, b) return __int_cmp(__force(a), __force(b)) <= 0 end
local function ord_ge__Integer(a, b) return __int_cmp(__force(a), __force(b)) >= 0 end
-- GHC default bodies: ties return max's second argument, min's first.
local function ord_max__Integer(a, b) a = __force(a); b = __force(b); if __int_cmp(a, b) <= 0 then return b else return a end end
local function ord_min__Integer(a, b) a = __force(a); b = __force(b); if __int_cmp(a, b) <= 0 then return a else return b end end
local function ord_compare__Integer(a, b)
    local c = __int_cmp(__force(a), __force(b))
    if c < 0 then return 1 elseif c > 0 then return 3 else return 2 end
end
-- `toInteger` at Integer is the identity; `fromInteger` too (already an Integer).
local function toInteger_Integer(x) return __force(x) end
-- read @Integer: tolerate surrounding space and one layer of parentheses.
local function read_Integer(s)
    s = __force(s)
    s = string.gsub(s, "^%s+", "")
    s = string.gsub(s, "%s+$", "")
    if string.byte(s, 1) == 40 then s = string.gsub(string.gsub(s, "^%(%s*", ""), "%s*%)$", "") end
    -- Validate BEFORE parsing: __int_from_decimal is the fast path for
    -- pre-validated literals and maps any byte through byte-48, so
    -- garbage ("12x", "") silently became a number. GHC: no parse.
    if not string.match(s, "^[%+%-]?%d+$") then error("Prelude.read: no parse") end
    return __int_from_decimal(s)
end
-- toInteger @Int: lift a machine Int to an Integer (bignum).
local function toInteger_Int(x) return fromInteger_Integer(__force(x)) end
-- Reconstruct the machine value of an Integer (high limb first). Limbs can
-- carry Lua's float subtype (carry propagation divides with `/`, which is
-- float division on 5.3+), though each limb's VALUE is an exact integer
-- below 2^24 — so the Int target re-anchors every limb with math.tointeger
-- and accumulates in integer arithmetic: exact through the full int64
-- range, and past it Lua's integer arithmetic wraps exactly like GHC's
-- Integer->Int narrowing. Float accumulation instead would round at 2^53
-- (maxBound :: Int came back off by one). On a doubles-only host the
-- identity fallback keeps the documented 2^53 exactness window.
local __int_limb = math.tointeger or function(x) return x end
local function __int_to_machine(x)
    local acc = 0
    for i = #x, 2, -1 do acc = acc * __INT_B + __int_limb(x[i]) end
    if x[1] < 0 then return -acc end
    return acc
end
-- The Number target accumulates in floats: a huge Integer approximates to
-- the nearest double (as GHC's fromInteger to Double does) instead of
-- wrapping at 2^64.
local function __int_to_double(x)
    local acc = 0.0
    for i = #x, 2, -1 do acc = acc * __INT_B + x[i] end
    if x[1] < 0 then return -acc end
    return acc
end
-- fromInteger @Int / @Number: narrow an Integer argument (an explicit
-- `fromInteger`/`fromIntegral`). A non-table argument is already the machine
-- value (defensive; the literal path never boxes an Int/Number).
local function fromInteger_Int(x)
    x = __force(x)
    if type(x) == "table" then return __int_to_machine(x) end
    return x
end
local function fromInteger_Number(x)
    x = __force(x)
    if type(x) == "table" then return __int_to_double(x) end
    return x
end
-- Numeric-literal pattern equality. The scrutinee may be a machine number (Int)
-- or a boxed Integer; Lua's `==` skips a metamethod for a mixed table/number
-- pair, so match type-directed: a boxed scrutinee compares the literal lifted to
-- an Integer, a machine scrutinee compares natively.
local function __mll_lit_eq(x, n)
    x = __force(x)
    if type(x) == "table" then
        if type(n) ~= "table" then n = fromInteger_Integer(n) end
        return __int_cmp(x, n) == 0
    end
    return x == n
end
