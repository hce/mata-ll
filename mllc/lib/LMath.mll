-- MLL bindings for Lua 5.4 math library

-- Constants
pi :: LuaPure "math.pi" Number
huge :: LuaPure "math.huge" Number
maxinteger :: LuaPure "math.maxinteger" Int
mininteger :: LuaPure "math.mininteger" Int

-- Trigonometric
sin :: Number -> LuaPure "math.sin" Number
cos :: Number -> LuaPure "math.cos" Number
tan :: Number -> LuaPure "math.tan" Number
asin :: Number -> LuaPure "math.asin" Number
acos :: Number -> LuaPure "math.acos" Number
atan :: Number -> LuaPure "math.atan" Number
atan2 :: Number -> Number -> LuaPure "math.atan" Number

-- Exponential / logarithmic
exp :: Number -> LuaPure "math.exp" Number
log :: Number -> LuaPure "math.log" Number
-- GHC argument order: `logBase base x` (a runtime shim reverses into
-- Lua's math.log(x, base) — binding math.log directly under this
-- GHC-evoking name silently meant log_x(base)).
logBase :: Number -> Number -> LuaPure "__mll_logbase" Number
sqrt :: Number -> LuaPure "math.sqrt" Number

-- Multi-return (packed into tuples)
-- Portable: math.frexp is compiled out of stock Lua 5.4/5.5
-- (LUA_COMPAT_MATHLIB); the shim uses it when present and computes the
-- mantissa/exponent pair otherwise.
frexp :: Number -> LuaPure "__mll_frexp" (Number, Int)
modf :: Number -> LuaPure "math.modf" (Number, Number)

-- Rounding / remainder
-- (No `abs` here: `abs` is the Prelude's Num method, which covers Number.
-- An LMath `abs :: Number -> Number` shadowed it for every importer, so
-- `abs (-3 :: Int)` failed to typecheck after `import LMath`.)
-- `floor` is the Prelude's `floor` primitive — the same declaration, so
-- importing LMath unqualified shadows nothing — and `ceil` is the
-- Prelude's `ceiling` under Lua's name. Both always yield an Int: Lua's
-- own math.floor/math.ceil return a float for NaN, an infinity or a
-- magnitude of 2^63 and beyond, which is no value of type Int; the
-- primitives give GHC's Int there (the exact value modulo 2^64, 0 for a
-- non-finite argument) and Lua's result for every other argument.
ceil :: Number -> LuaPure "__mll_ceil" Int
floor :: Number -> LuaPure "__mll_floor" Int
fmod :: Number -> Number -> LuaPure "math.fmod" Number

-- Int
tointeger :: Number -> LuaPure "math.tointeger" Int
ult :: Int -> Int -> LuaPure "math.ult" Bool

-- Random (effectful)
random :: LuaIO "math.random" Number
randomInt :: Int -> Int -> LuaIO "math.random" Int
randomseed :: Int -> LuaIO "math.randomseed" ()
