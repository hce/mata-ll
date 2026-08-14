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
-- Always-boxed (no small-int fast path yet — a later optimization).
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
local function __int_ushl1(m)  -- magnitude * 2
    local r, carry = {}, 0
    for i = 1, #m do
        local v = m[i] * 2 + carry
        if v >= __INT_B then r[i] = v - __INT_B; carry = 1 else r[i] = v; carry = 0 end
    end
    if carry > 0 then r[#m + 1] = carry end
    return r
end
local function __int_ubits(m)  -- position of highest set bit + 1 (0 for zero)
    local n = #m
    if n == 0 then return 0 end
    local top, bits = m[n], (n - 1) * 24
    while top > 0 do bits = bits + 1; top = (top - top % 2) / 2 end
    return bits
end
local function __int_ubit(m, i)  -- bit i (0-based)
    local v = m[math.floor(i / 24) + 1] or 0
    local p = 1
    for _ = 1, i % 24 do p = p * 2 end
    return math.floor(v / p) % 2
end
-- Binary long division of magnitudes (V nonzero): returns quotient, remainder.
-- O(bits) — correct and simple; a schoolbook base-2^24 divide is a later
-- optimization alongside the small-int fast path.
local function __int_udivmod(U, V)
    if __int_ucmp(U, V) < 0 then return {}, U end
    local q, r = {}, {}
    for i = 1, #U do q[i] = 0 end   -- dense: quotient has at most #U limbs
    for i = __int_ubits(U) - 1, 0, -1 do
        r = __int_ushl1(r)
        if __int_ubit(U, i) == 1 then r[1] = (r[1] or 0) + 1 end
        if __int_ucmp(r, V) >= 0 then
            r = __int_usub(r, V)
            local limb = math.floor(i / 24) + 1
            local p = 1
            for _ = 1, i % 24 do p = p * 2 end
            q[limb] = (q[limb] or 0) + p
        end
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
    local s = tostring(groups[#groups])
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
    __eq = function(a, b) return __int_cmp(a, b) == 0 end,
    __lt = function(a, b) return __int_cmp(a, b) < 0 end,
    __le = function(a, b) return __int_cmp(a, b) <= 0 end,
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
    return add_Integer(a, negate_Integer(b))
end
mul_Integer = function(a, b)
    a = __force(a); b = __force(b)
    local sa, sb = a[1], b[1]
    if sa == 0 or sb == 0 then return __int_zero end
    return __int_mk(sa * sb, __int_umul(__int_mag(a), __int_mag(b)))
end
-- Truncating quotient/remainder (remainder takes the DIVIDEND's sign).
local function __int_qr_trunc(a, b)
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
