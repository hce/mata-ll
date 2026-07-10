-- LuaCatch / LuaIOCatch: run a Lua function under pcall, capturing a raised
-- Lua error as `Left msg` and a normal return as `Right a`. `string.char`
-- succeeds for a byte in 0..255 and raises "value out of range" otherwise, so
-- it exercises both arms without a custom host function.

-- Pure pcall FFI.
charOf :: Integer -> LuaCatch "string.char" (Either String String)

-- Effectful pcall FFI (deferred as an IO action).
charOfIO :: Integer -> LuaIOCatch "string.char" (Either String String)

isRight :: Either String String -> Bool
isRight e = case e of
  Right _ -> True
  Left _  -> False

isLeft :: Either String String -> Bool
isLeft e = case e of
  Left _  -> True
  Right _ -> False

main :: IO ()
main = do
  -- Pure: success crosses back as Right, carrying the marshalled payload.
  assert (isRight (charOf 65)) "LuaCatch success is Right"
  assert (case charOf 65 of { Right c -> c == "A"; Left _ -> False }) "LuaCatch Right payload"
  -- Pure: a raised Lua error is captured as Left, not a crash.
  assert (isLeft (charOf (-1))) "LuaCatch raised error is Left"
  -- IO: same capture, but the action is run in the IO monad.
  r1 <- charOfIO 66
  assert (isRight r1) "LuaIOCatch success is Right"
  assert (case r1 of { Right c -> c == "B"; Left _ -> False }) "LuaIOCatch Right payload"
  r2 <- charOfIO 999
  assert (isLeft r2) "LuaIOCatch raised error is Left"
