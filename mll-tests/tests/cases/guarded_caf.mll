-- A guarded VALUE binding (zero patterns) is a CAF, not a nullary
-- function: its guard chain must lower to a thunked value so use sites
-- force it like any other CAF. This used to emit a nullary Lua function
-- while the slot was predicted WHNF — arithmetic on a function value.

cond :: Bool
cond = False

x :: Int
x | cond = 1
  | otherwise = 2

-- Guarded CAF that references another CAF defined LATER (forward slot).
y :: Int
y | x == 2 = x + 10
  | otherwise = 0

-- String result through a guard chain.
label :: String
label | x == 1 = "one"
      | x == 2 = "two"
      | otherwise = "many"

main :: IO ()
main = do
    assert (x + 1 == 3) "guarded CAF is a value, not a function"
    assert (y == 12) "guarded CAF may read other CAFs"
    assert (label == "two") "guard chain selects by order"
    putStrLn "ok"
