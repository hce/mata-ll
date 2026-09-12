-- Identifiers that are valid mata-ll names but Lua reserved words (`until`,
-- `elseif`, `local`, `nil`, `function`, `while`, ...) must be escaped in the
-- emitted Lua. They are exercised here as a function name, parameters, let and
-- where bindings, and record fields. (`repeat`, another Lua keyword, is
-- exercised by the Prelude's own `repeat`.)

-- Top-level name + parameters named after Lua keywords.
elseif :: Int -> Int -> Int
elseif until while = until + while

-- Keyword names as let / where bindings.
compute :: Int -> Int
compute x = local + goto
  where
    local = x * 2
    goto  = let function = x + 1 in function

-- Keyword names as record fields. (`until` is a Prelude function since the
-- GHC-parity additions of 2026-09-12, so it cannot be a field name — a field
-- defines a module-level accessor — and `nil` stands in for it here.)
data Cfg = Cfg { nil :: Int, while :: Int, function :: Int }

grab :: Cfg -> Int
grab c = nil c + while c + function c

main :: IO ()
main = do
  assert (elseif 3 4 == 7) "keyword function name + params"
  assert (compute 5 == 16) "keyword let/where bindings"
  assert (grab (Cfg 1 2 3) == 6) "keyword record fields"
