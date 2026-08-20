-- `seq` on an ACTION VALUE forces it to WHNF only — the suspended
-- closure is built, never run — so nothing inside the action's chain is
-- demanded. Regression: the demand analyses claimed the chain's interior
-- demands for seq'd/strictly-passed actions (`act `seq` 42` marked the
-- enclosing function strict in x through act = print (x + 1)), so the
-- emitted entry-force evaluated an argument GHC never touches.

useSeq :: Int -> Int
useSeq x = act `seq` 42
  where act = print (x + 1)

useSeqBind :: Int -> Int
useSeqBind x = act `seq` 43
  where act = pure (x + 1) >>= print

useSeqPrefix :: Int -> Int
useSeqPrefix x = seq act 44
  where act = print (x + 1)

-- control: an action in RUN position still runs (and forces normally)
runIt :: Int -> IO ()
runIt x = act
  where act = print (x + 1)

main :: IO ()
main = do
    print (useSeq (error "boom"))
    print (useSeqBind (error "boom"))
    print (useSeqPrefix (error "boom"))
    runIt 7
