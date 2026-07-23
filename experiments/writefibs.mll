export writeFibs :: (String -> LuaIO s ()) -> Int -> LuaIO s ()
writeFibs writer = loop 1 1
  where
    loop _ _ 0 = return ()
    loop curFib nextFib count = do
      writer (show curFib)
      loop nextFib (curFib + nextFib) (count - 1)
