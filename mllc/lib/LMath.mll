-- MLL bindings for Lua 5.4 math library

-- Constants
pi :: LuaPure "math.pi" Number
huge :: LuaPure "math.huge" Number
maxinteger :: LuaPure "math.maxinteger" Integer
mininteger :: LuaPure "math.mininteger" Integer

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
logBase :: Number -> Number -> LuaPure "math.log" Number
sqrt :: Number -> LuaPure "math.sqrt" Number

-- Multi-return (packed into tuples)
frexp :: Number -> LuaPure "math.frexp" (Number, Integer)
modf :: Number -> LuaPure "math.modf" (Number, Number)

-- Rounding / remainder
abs :: Number -> LuaPure "math.abs" Number
ceil :: Number -> LuaPure "math.ceil" Integer
floor :: Number -> LuaPure "math.floor" Integer
fmod :: Number -> Number -> LuaPure "math.fmod" Number

-- Integer
tointeger :: Number -> LuaPure "math.tointeger" Integer
ult :: Integer -> Integer -> LuaPure "math.ult" Bool

-- Random (effectful)
random :: LuaIO "math.random" Number
randomInt :: Integer -> Integer -> LuaIO "math.random" Integer
randomseed :: Integer -> LuaIO "math.randomseed" ()
