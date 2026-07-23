-- Identifiers that are valid mata-ll names but Lua reserved words (`until`,
-- `repeat`, `local`, `nil`, `function`, `while`, ...) must be escaped in the
-- emitted Lua. They are exercised here as a function name, parameters, let and
-- where bindings, and record fields.

-- Top-level name + parameters named after Lua keywords.
repeat :: Int -> Int -> Int
repeat until while = until + while

-- Keyword names as let / where bindings.
compute :: Int -> Int
compute x = local + goto
  where
    local = x * 2
    goto  = let function = x + 1 in function

-- Keyword names as record fields.
data Cfg = Cfg { until :: Int, while :: Int, function :: Int }

grab :: Cfg -> Int
grab c = until c + while c + function c

main :: IO ()
main = do
  assert (repeat 3 4 == 7) "keyword function name + params"
  assert (compute 5 == 16) "keyword let/where bindings"
  assert (grab (Cfg 1 2 3) == 6) "keyword record fields"
