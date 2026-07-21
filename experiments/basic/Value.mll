-- Runtime values and the coercions BASIC performs between them.
module Value (Value(..), showVal, asNum, asStr, isTrue, fromBool) where

import LMath (fmod, floor)

-- A BASIC value is either a number or a string. Which one a variable holds is
-- fixed by its name: a trailing '$' means string, otherwise number.
data Value = VNum Number | VStr String
  deriving (Show)

-- Render a value the way PRINT / STR$ do. Whole numbers print without a
-- decimal point (mata-ll's `show` already does this), and a non-negative
-- number carries a leading space where its sign would be -- a BASIC habit.
showVal :: Value -> String
showVal (VStr s) = s
showVal (VNum n) =
    -- Whole values print without a decimal point regardless of whether they
    -- arrived as a Lua integer or a float (e.g. SQR returns 12.0, not 12).
    let body = if fmod n 1.0 == 0.0 then show (floor n) else show n
    in if n < 0.0 then body else " " <> body

-- Coerce to a number, or fail with a type error (BASIC is loosely typed but
-- still rejects using a string where arithmetic is expected).
asNum :: Value -> Number
asNum (VNum n) = n
asNum (VStr _) = error "type mismatch: expected a number"

asStr :: Value -> String
asStr (VStr s) = s
asStr (VNum _) = error "type mismatch: expected a string"

-- BASIC truth: any non-zero number is true. Comparisons yield -1 / 0.
isTrue :: Value -> Bool
isTrue (VNum n) = n /= 0.0
isTrue (VStr _) = error "type mismatch: a condition must be numeric"

fromBool :: Bool -> Value
fromBool b = if b then VNum (0.0 - 1.0) else VNum 0.0
