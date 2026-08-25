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
abs :: Number -> LuaPure "math.abs" Number
ceil :: Number -> LuaPure "math.ceil" Int
floor :: Number -> LuaPure "math.floor" Int
fmod :: Number -> Number -> LuaPure "math.fmod" Number

-- Int
tointeger :: Number -> LuaPure "math.tointeger" Int
ult :: Int -> Int -> LuaPure "math.ult" Bool

-- Random (effectful)
random :: LuaIO "math.random" Number
randomInt :: Int -> Int -> LuaIO "math.random" Int
randomseed :: Int -> LuaIO "math.randomseed" ()
