-- Regression: a class constraint variable that appears ONLY in the result
-- type (or in a function-typed argument's codomain) must be pinned from the
-- expected type at the call site. Previously monomorphization's structural
-- matcher had no case for `m a` vs the sugared IO/List types, so `m` stayed
-- unbound and the single-parameter default wrongly resolved it to another
-- argument's type (String) -> "No instance for 'pure' on type 'String'".

konst :: Monad m => a -> m ()
konst _ = pure ()

apply1 :: Monad m => (a -> m b) -> a -> m ()
apply1 f x = f x >> pure ()

-- Result-only variable pinned by a PURE monad expected type (Maybe).
konstMaybe :: Maybe ()
konstMaybe = konst "y"

-- Result-only variable where the pinning monad is the list monad.
konstList :: [()]
konstList = konst (42 :: Integer)

main :: IO ()
main = do
  -- Pinned by the IO () expected type of the do-statement.
  konst "x"
  konst (1 :: Integer)
  -- Pinned via the codomain of a function-typed argument's use.
  apply1 putStrLn "hello from apply1"
  assert (konstMaybe == Just ()) "konst pinned to Maybe"
  assert (konstList == [()]) "konst pinned to []"
  assert (apply1 (\n -> if n > 0 then Just n else Nothing) 5 == Just ()) "apply1 in Maybe (ok)"
  assert (apply1 (\n -> if n > 0 then Just n else Nothing) (0 - 5) == Nothing) "apply1 in Maybe (fail)"
